//! Safe native observations for macOS and Windows; shared admission stays in RRC.
use super::{HostCapacity, LinuxPressureSignals, ProcessTreeUsage};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug)]
pub(super) struct NativeObserver {
    #[cfg(windows)]
    job: Option<windows_job::Probe>,
}

impl NativeObserver {
    pub(super) fn new() -> Self {
        Self {
            #[cfg(windows)]
            job: None,
        }
    }

    pub(super) fn snapshot(
        &mut self,
        root_pid: Option<u32>,
        cancelled: Option<&std::sync::atomic::AtomicBool>,
    ) -> io::Result<(HostCapacity, ProcessTreeUsage)> {
        if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "native observation cancelled",
            ));
        }
        // A failed native refresh must not reuse yesterday's successful values.
        let mut system = System::new();
        system.refresh_memory();
        let total = system.total_memory();
        let available = system.available_memory();
        if total == 0 || available > total || system.free_swap() > system.total_swap() {
            return Err(io::Error::other("invalid native memory observation"));
        }
        let mut capacity = HostCapacity {
            host_memory_total_bytes: total,
            memory_available_bytes: available,
            swap_total_bytes: system.total_swap(),
            swap_free_bytes: system.free_swap(),
            cgroup_memory_limit_bytes: None,
            cgroup_memory_current_bytes: None,
            native_memory_limit_bytes: None,
            native_memory_current_bound_bytes: None,
            logical_cpus: std::thread::available_parallelism()
                .map(|count| u32::try_from(count.get()).unwrap_or(u32::MAX))?,
            linux: LinuxPressureSignals::default(),
        };
        #[cfg(windows)]
        {
            if self.job.is_none() {
                self.job = Some(windows_job::Probe::start()?);
            }
            let observation = self
                .job
                .as_mut()
                .expect("initialized probe")
                .sample(cancelled);
            let (limit, current_bound) = match observation {
                Ok(bounds) => bounds,
                Err(error) => {
                    self.job.take();
                    return Err(error);
                }
            };
            capacity.native_memory_limit_bytes = limit;
            capacity.native_memory_current_bound_bytes = current_bound;
        }
        // The Linux test path verifies the native-library contract too; production
        // Linux retains its stricter procfs and ancestor-cgroup observer.
        #[cfg(not(windows))]
        let _ = &mut capacity;
        let usage = if let Some(root) = root_pid {
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_memory().without_tasks(),
            );
            let rows = system
                .processes()
                .iter()
                .map(|(pid, process)| {
                    (
                        pid.as_u32(),
                        Row {
                            parent: process.parent().map(|parent| parent.as_u32()),
                            rss: process.memory(),
                            rustc: process.name() == "rustc" || process.name() == "rustc.exe",
                        },
                    )
                })
                .collect();
            descendant_usage(root, &rows)?
        } else {
            ProcessTreeUsage::default()
        };
        Ok((capacity, usage))
    }
}

#[derive(Debug)]
struct Row {
    parent: Option<u32>,
    rss: u64,
    rustc: bool,
}

fn descendant_usage(root: u32, rows: &BTreeMap<u32, Row>) -> io::Result<ProcessTreeUsage> {
    if !rows.contains_key(&root) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "owned process missing from native observation",
        ));
    }
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (&pid, row) in rows {
        if let Some(parent) = row.parent {
            children.entry(parent).or_default().push(pid);
        }
    }
    let mut seen = BTreeSet::new();
    let mut pending = vec![root];
    let mut usage = ProcessTreeUsage::default();
    while let Some(pid) = pending.pop() {
        if !seen.insert(pid) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cyclic native process inventory",
            ));
        }
        let row = &rows[&pid];
        usage.rss_bytes = usage.rss_bytes.saturating_add(row.rss);
        usage.process_count = usage.process_count.saturating_add(1);
        usage.rustc_count = usage.rustc_count.saturating_add(u32::from(row.rustc));
        if let Some(descendants) = children.get(&pid) {
            pending.extend(descendants);
        }
    }
    Ok(usage)
}

#[cfg(any(windows, test))]
fn await_job_observation(
    rows: &std::sync::mpsc::Receiver<io::Result<String>>,
    timeout: std::time::Duration,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> io::Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Windows job observation cancelled",
            ));
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Windows job observation did not settle",
            ));
        }
        match rows.recv_timeout(remaining.min(std::time::Duration::from_millis(20))) {
            Ok(row) => return row,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("Windows job observer exited"));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(any(windows, test))]
fn parse_job_bounds(row: &str) -> io::Result<(Option<u64>, Option<u64>)> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Observation {
        in_job: bool,
        limit: u64,
        current_bound: u64,
    }
    let observation: Observation = serde_json::from_str(row)
        .map_err(|_| io::Error::other("invalid Windows job observation"))?;
    if (!observation.in_job && observation.limit != 0)
        || (observation.limit == 0 && observation.current_bound != 0)
    {
        return Err(io::Error::other("incomplete Windows job accounting"));
    }
    Ok(if observation.limit == 0 {
        (None, None)
    } else {
        (Some(observation.limit), Some(observation.current_bound))
    })
}

#[cfg(windows)]
mod windows_job {
    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    // Fixed read-only OS queries. The helper inherits the invoking host's Job
    // Object; no job creation, limits, breakaway, credentials or user input.
    const SCRIPT: &str = include_str!("host_resources_windows_job.ps1");

    #[derive(Debug)]
    pub(super) struct Probe {
        child: Child,
        input: Option<ChildStdin>,
        rows: Receiver<io::Result<String>>,
        reader: Option<JoinHandle<()>>,
        first: bool,
        _scratch: tempfile::TempDir,
    }

    impl Probe {
        pub(super) fn start() -> io::Result<Self> {
            let system_root = std::env::var_os("SystemRoot")
                .ok_or_else(|| io::Error::other("Windows SystemRoot is unavailable"))?;
            let executable = std::path::PathBuf::from(system_root)
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let scratch = tempfile::tempdir()?;
            let mut child = Command::new(executable)
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    SCRIPT,
                ])
                .env("TEMP", scratch.path())
                .env("TMP", scratch.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            let input = child.stdin.take();
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("native probe stdout missing"))?;
            let (sender, rows) = mpsc::channel();
            let reader = thread::spawn(move || {
                let mut source = BufReader::new(stdout);
                loop {
                    let mut row = String::new();
                    let result = source.by_ref().take(8192).read_line(&mut row);
                    match result {
                        Ok(0) => break,
                        Ok(_) if row.ends_with('\n') => {
                            if sender.send(Ok(row)).is_err() {
                                break;
                            }
                        }
                        _ => {
                            let _ = sender.send(Err(io::Error::other(
                                "invalid bounded Windows job observation",
                            )));
                            break;
                        }
                    }
                }
            });
            Ok(Self {
                child,
                input,
                rows,
                reader: Some(reader),
                first: true,
                _scratch: scratch,
            })
        }

        pub(super) fn sample(
            &mut self,
            cancelled: Option<&std::sync::atomic::AtomicBool>,
        ) -> io::Result<(Option<u64>, Option<u64>)> {
            self.input
                .as_mut()
                .ok_or_else(|| io::Error::other("native probe is closed"))?
                .write_all(b"sample\n")?;
            let deadline = if self.first {
                Duration::from_secs(20)
            } else {
                Duration::from_millis(500)
            };
            self.first = false;
            let row = super::await_job_observation(&self.rows, deadline, cancelled)?;
            super::parse_job_bounds(&row)
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.input.take();
            // Include any compiler child from PowerShell's fixed Add-Type setup.
            if self.child.try_wait().ok().flatten().is_none() {
                if let Ok(mut cleanup) = Command::new("taskkill")
                    .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    let deadline = std::time::Instant::now() + Duration::from_secs(3);
                    while cleanup.try_wait().ok().flatten().is_none()
                        && std::time::Instant::now() < deadline
                    {
                        thread::sleep(Duration::from_millis(20));
                    }
                    let _ = cleanup.kill();
                    let _ = cleanup.wait();
                }
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capacity_and_owned_process_observation_is_real() {
        let mut observer = NativeObserver::new();
        let (capacity, usage) = observer.snapshot(Some(std::process::id()), None).unwrap();
        assert!(capacity.host_memory_total_bytes > 0);
        assert!(capacity.memory_available_bytes <= capacity.host_memory_total_bytes);
        assert!(capacity.logical_cpus > 0);
        assert!(usage.process_count >= 1);
        assert!(usage.rss_bytes > 0);
    }

    #[test]
    fn native_owned_descendants_are_observed_and_reaped() {
        use command_group::CommandGroup;
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "host_resources::native_backend::tests::native_process_fixture",
                "--nocapture",
            ])
            .env("AGENT_VESPER_NATIVE_PROCESS_FIXTURE", "parent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.group_spawn().unwrap();
        let stdout = child.inner().stdout.take().unwrap();
        let (sender, rows) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for row in BufReader::new(stdout).lines() {
                if row.is_ok_and(|row| row.contains("native-descendant-ready")) {
                    let _ = sender.send(());
                }
            }
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rows.recv_timeout(Duration::from_secs(15)).unwrap();
            let mut observer = NativeObserver::new();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let (_, usage) = observer.snapshot(Some(child.inner().id()), None).unwrap();
                if usage.process_count >= 2 {
                    assert!(usage.rss_bytes > 0);
                    break;
                }
                assert!(Instant::now() < deadline, "missing owned descendant");
                std::thread::sleep(Duration::from_millis(50));
            }
        }));
        child.kill().unwrap();
        child.wait().unwrap();
        reader.join().unwrap();
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn native_process_fixture() {
        let Ok(role) = std::env::var("AGENT_VESPER_NATIVE_PROCESS_FIXTURE") else {
            return;
        };
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        let mut descendant = if role == "parent" {
            Some(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "host_resources::native_backend::tests::native_process_fixture",
                        "--nocapture",
                    ])
                    .env("AGENT_VESPER_NATIVE_PROCESS_FIXTURE", "descendant")
                    .stdin(Stdio::piped())
                    .spawn()
                    .unwrap(),
            )
        } else {
            None
        };
        if role == "descendant" {
            println!("native-descendant-ready");
            std::io::stdout().flush().unwrap();
        }
        let _ = std::io::stdin().read(&mut [0_u8]);
        if let Some(child) = descendant.as_mut() {
            child.stdin.take();
            let _ = child.wait();
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_fixture() {
        if std::env::var("AGENT_VESPER_WINDOWS_JOB_FIXTURE").as_deref() != Ok("1") {
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let governor = super::super::HostResourceGovernor::new(
            super::super::ResourcePolicy::default(),
            temporary.path(),
            temporary.path().join("target"),
        )
        .unwrap();
        let observed = governor.snapshot(Some(std::process::id())).unwrap();
        assert_eq!(
            observed.native_memory_limit_bytes,
            Some(2 * 1024 * 1024 * 1024)
        );
        assert!(
            observed
                .native_memory_current_bound_bytes
                .is_some_and(|bytes| bytes > 0)
        );
        assert!(observed.memory_available_bytes < 2 * 1024 * 1024 * 1024);
        assert_ne!(observed.pressure, super::super::ResourcePressure::Normal);
        assert!(matches!(
            governor.preflight(super::super::GateCost::Expensive),
            Err(super::super::ResourceGovernorError::Pressure(_)
                | super::super::ResourceGovernorError::Critical(_)
                | super::super::ResourceGovernorError::Disk(_))
        ));
        println!(
            "WINDOWS_JOB_OBSERVATION={}",
            serde_json::to_string(&observed).unwrap()
        );
    }

    #[test]
    fn native_observation_cancellation_interrupts_a_stalled_job_probe() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};
        let (sender, rows) = std::sync::mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancelled);
        let notifier = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            signal.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let error =
            await_job_observation(&rows, Duration::from_secs(20), Some(&cancelled)).unwrap_err();
        notifier.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(1));
        let mut observer = NativeObserver::new();
        assert_eq!(
            observer
                .snapshot(None, Some(&cancelled))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        drop(sender);
    }

    #[test]
    fn windows_job_observations_reject_missing_malformed_and_inconsistent_values() {
        assert_eq!(
            parse_job_bounds(r#"{"in_job":false,"limit":0,"current_bound":0}"#).unwrap(),
            (None, None)
        );
        assert_eq!(
            parse_job_bounds(r#"{"in_job":true,"limit":100,"current_bound":25}"#).unwrap(),
            (Some(100), Some(25))
        );
        for row in [
            "{}",
            "not json",
            r#"{"in_job":false,"limit":100,"current_bound":25}"#,
            r#"{"in_job":true,"limit":0,"current_bound":25}"#,
            r#"{"in_job":true,"limit":100}"#,
        ] {
            assert!(parse_job_bounds(row).is_err(), "{row}");
        }
    }

    #[test]
    fn native_tree_counts_descendants_excludes_siblings_and_refuses_cycles() {
        let mut rows = BTreeMap::from([
            (
                1,
                Row {
                    parent: None,
                    rss: 10,
                    rustc: false,
                },
            ),
            (
                2,
                Row {
                    parent: Some(1),
                    rss: 20,
                    rustc: true,
                },
            ),
            (
                3,
                Row {
                    parent: Some(2),
                    rss: 30,
                    rustc: false,
                },
            ),
            (
                4,
                Row {
                    parent: None,
                    rss: 400,
                    rustc: true,
                },
            ),
        ]);
        assert_eq!(
            descendant_usage(1, &rows).unwrap(),
            ProcessTreeUsage {
                rss_bytes: 60,
                process_count: 3,
                rustc_count: 1
            }
        );
        assert!(descendant_usage(99, &rows).is_err());
        rows.get_mut(&1).unwrap().parent = Some(3);
        assert!(descendant_usage(1, &rows).is_err());
    }
}
