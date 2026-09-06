//! Bounded duplex transport. The sandbox handle outlives its pipe process.
#[cfg(any(feature = "docker", test))]
use std::io::{BufRead, Read};
#[cfg(feature = "docker")]
use std::io::{BufReader, Write};
use std::process::Child;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use crate::{SandboxError, SandboxHandle};

/// Maximum incoming protocol frame; ordinary command output retains its
/// independent 64 KiB cap. DOM snapshots require a larger bounded frame.
#[cfg(any(feature = "docker", test))]
const FRAME_CAP: usize = 16 * 1024 * 1024;
const COMMAND_CAP: usize = 512 * 1024;
type WriteRequest = (Vec<u8>, mpsc::SyncSender<Result<(), String>>);

/// An ephemeral sandbox's NUL-framed command/event channel.
/// No protocol bytes or command values are logged or included in errors.
pub struct SandboxPipe {
    child: Child,
    handle: Option<Arc<SandboxHandle>>,
    writer: Option<mpsc::SyncSender<WriteRequest>>,
    reader: Option<mpsc::Receiver<Result<Vec<u8>, String>>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl SandboxPipe {
    #[cfg(feature = "docker")]
    pub(crate) fn new(mut child: Child, handle: Arc<SandboxHandle>) -> Result<Self, SandboxError> {
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let (Some(mut stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SandboxError::Run("duplex process has no pipes".into()));
        };
        let (writer, requests) = mpsc::sync_channel::<WriteRequest>(1);
        let write_thread = std::thread::spawn(move || {
            while let Ok((bytes, reply)) = requests.recv() {
                let result = stdin
                    .write_all(&bytes)
                    .and_then(|()| stdin.flush())
                    .map_err(|_| "sandbox pipe write failed".to_owned());
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    break;
                }
            }
        });
        // One queued frame bounds memory even if the browser floods events.
        let (events, reader) = mpsc::sync_channel(1);
        let read_thread = std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let result = read_frame(&mut stdout);
                let failed = result.is_err();
                if events.send(result).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            handle: Some(handle),
            writer: Some(writer),
            reader: Some(reader),
            threads: vec![write_thread, read_thread],
        })
    }

    /// Send a complete frame before the caller's absolute action deadline.
    pub fn send(&self, mut bytes: Vec<u8>, deadline: Instant) -> Result<(), SandboxError> {
        if bytes.len() > COMMAND_CAP || bytes.contains(&0) {
            return Err(SandboxError::Run(
                "invalid or oversized pipe command".into(),
            ));
        }
        bytes.push(0);
        let (reply, result) = mpsc::sync_channel(1);
        self.writer
            .as_ref()
            .ok_or_else(closed)?
            .try_send((bytes, reply))
            .map_err(|_| closed())?;
        result
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| timeout())?
            .map_err(SandboxError::Run)
    }

    /// Receive one bounded frame before an absolute action deadline.
    pub fn receive(&self, deadline: Instant) -> Result<Vec<u8>, SandboxError> {
        self.reader
            .as_ref()
            .ok_or_else(closed)?
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => timeout(),
                mpsc::RecvTimeoutError::Disconnected => closed(),
            })?
            .map_err(SandboxError::Run)
    }
}

fn closed() -> SandboxError {
    SandboxError::Run("sandbox pipe closed; reopen session".into())
}
fn timeout() -> SandboxError {
    SandboxError::Run("timed_out: sandbox pipe deadline".into())
}

#[cfg(any(feature = "docker", test))]
fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((FRAME_CAP + 1) as u64)
        .read_until(0, &mut bytes)
        .map_err(|_| "sandbox pipe read failed".to_owned())?;
    if bytes.len() > FRAME_CAP {
        return Err("budget_exceeded: sandbox pipe frame".into());
    }
    if bytes.pop() != Some(0) {
        return Err("sandbox pipe closed mid-frame".into());
    }
    Ok(bytes)
}

impl Drop for SandboxPipe {
    fn drop(&mut self) {
        self.writer.take();
        self.reader.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Remove the container before joining I/O readers. Closing the local
        // exec client alone is not a promise that the remote process died.
        self.handle.take();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_preserve_boundaries_and_reject_eof() {
        let mut input = &b"one\0two\0partial"[..];
        assert_eq!(read_frame(&mut input).unwrap(), b"one");
        assert_eq!(read_frame(&mut input).unwrap(), b"two");
        assert!(read_frame(&mut input).is_err());
    }
    #[test]
    fn oversized_frames_fail_closed() {
        assert!(read_frame(&mut &vec![b'x'; FRAME_CAP + 1][..]).is_err());
    }
}
