use std::io;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const CAPTURE_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    Exited,
    TimedOut,
    Cancelled,
}

#[derive(Debug)]
pub struct Observation {
    pub pid: u32,
    pub process_group: i32,
    pub terminal: Terminal,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_total: usize,
    pub stderr_total: usize,
    pub post_termination_output_bytes: usize,
    pub kill_to_reap: Option<Duration>,
    pub group_survived: bool,
}

struct Drain {
    bytes: Vec<u8>,
    total: usize,
}

async fn drain<R: AsyncRead + Unpin>(
    mut reader: R,
    count: Arc<AtomicUsize>,
    cancel: CancellationToken,
) -> io::Result<Drain> {
    let mut kept = Vec::with_capacity(CAPTURE_LIMIT);
    let mut total = 0usize;
    let mut counting = true;
    let mut buf = [0u8; 8192];
    loop {
        // While still counting, race each read against cancellation. The
        // instant the token fires (biased so cancellation wins a tie) we
        // freeze the captured byte count: a child can keep writing into the
        // OS pipe buffer between the cancel signal and the SIGKILL that reaps
        // its process group, and those post-cancellation bytes must NOT
        // inflate `post_termination_output_bytes`. We keep reading to EOF
        // afterwards — discarding bytes — so the pipe still drains and the
        // child can be reaped instead of stalling on a full buffer.
        let n = if counting {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    counting = false;
                    continue;
                }
                read = reader.read(&mut buf) => read?,
            }
        } else {
            reader.read(&mut buf).await?
        };
        if n == 0 {
            break;
        }
        if counting {
            total += n;
            count.store(total, Ordering::SeqCst);
            let room = CAPTURE_LIMIT.saturating_sub(kept.len());
            kept.extend_from_slice(&buf[..n.min(room)]);
        }
    }
    Ok(Drain { bytes: kept, total })
}

#[cfg(unix)]
fn configure_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn configure_group(_command: &mut Command) {}

#[cfg(unix)]
fn signal_group(group: i32, signal: i32) {
    unsafe {
        libc::kill(-group, signal);
    }
}

#[cfg(not(unix))]
fn signal_group(_group: i32, _signal: i32) {}

#[cfg(unix)]
fn group_exists(group: i32) -> bool {
    let rc = unsafe { libc::kill(-group, 0) };
    rc == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn group_exists(_group: i32) -> bool {
    false
}

async fn execute(
    program: String,
    mode: String,
    timeout: Duration,
    cancel: CancellationToken,
    started: tokio::sync::oneshot::Sender<(u32, i32)>,
) -> io::Result<Observation> {
    let mut command = Command::new(program);
    command
        .arg(mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    configure_group(&mut command);
    let mut child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("missing child pid"))?;
    let group = pid as i32;
    let _ = started.send((pid, group));

    let stdout_count = Arc::new(AtomicUsize::new(0));
    let stderr_count = Arc::new(AtomicUsize::new(0));
    let stdout_task = tokio::spawn(drain(
        child.stdout.take().unwrap(),
        Arc::clone(&stdout_count),
        cancel.clone(),
    ));
    let stderr_task = tokio::spawn(drain(
        child.stderr.take().unwrap(),
        Arc::clone(&stderr_count),
        cancel.clone(),
    ));

    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let terminal = tokio::select! {
        status = child.wait() => {
            status?;
            Terminal::Exited
        }
        _ = cancel.cancelled() => Terminal::Cancelled,
        _ = &mut deadline => Terminal::TimedOut,
    };

    let mut kill_to_reap = None;
    let before_kill_output =
        stdout_count.load(Ordering::SeqCst) + stderr_count.load(Ordering::SeqCst);
    let descendant_cleanup_needed = group_exists(group);
    if terminal != Terminal::Exited || descendant_cleanup_needed {
        let kill_started = Instant::now();
        #[cfg(unix)]
        signal_group(group, libc::SIGTERM);
        tokio::time::sleep(Duration::from_millis(75)).await;
        #[cfg(unix)]
        signal_group(group, libc::SIGKILL);
        #[cfg(not(unix))]
        child.start_kill()?;
        if terminal != Terminal::Exited {
            let _ = child.wait().await?;
        }
        kill_to_reap = Some(kill_started.elapsed());
    }

    let stdout = stdout_task.await.map_err(io::Error::other)??;
    let stderr = stderr_task.await.map_err(io::Error::other)??;
    let total_after = stdout.total + stderr.total;
    tokio::time::sleep(Duration::from_millis(25)).await;

    Ok(Observation {
        pid,
        process_group: group,
        terminal,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        stdout_total: stdout.total,
        stderr_total: stderr.total,
        post_termination_output_bytes: total_after.saturating_sub(before_kill_output),
        kill_to_reap,
        group_survived: group_exists(group),
    })
}

pub struct SupervisedRun {
    pub pid: u32,
    pub process_group: i32,
    cancel: CancellationToken,
    task: Option<JoinHandle<io::Result<Observation>>>,
}

impl SupervisedRun {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub async fn wait(mut self) -> io::Result<Observation> {
        let task = self.task.take().unwrap();
        task.await.map_err(io::Error::other)?
    }
}

impl Drop for SupervisedRun {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub async fn spawn_supervised(
    program: impl Into<String>,
    mode: impl Into<String>,
    timeout: Duration,
) -> io::Result<SupervisedRun> {
    let cancel = CancellationToken::new();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(execute(
        program.into(),
        mode.into(),
        timeout,
        cancel.clone(),
        started_tx,
    ));
    let (pid, process_group) = started_rx.await.map_err(io::Error::other)?;
    Ok(SupervisedRun {
        pid,
        process_group,
        cancel,
        task: Some(task),
    })
}

pub fn process_group_exists(group: i32) -> bool {
    group_exists(group)
}

pub fn require_bubblewrap(binary: &str) -> io::Result<()> {
    let status = std::process::Command::new(binary)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(io::Error::other("Bubblewrap probe failed")),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("required Bubblewrap unavailable: {error}"),
        )),
    }
}
