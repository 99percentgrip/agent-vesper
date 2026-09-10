//! `sandbox_init` — the namespaces supervisor for `vesper-sandbox`
//! (VRO-13 PR-3, ADR-0022).
//!
//! This is the **only** production component in the workspace that performs
//! raw syscalls. Every crate and module keeps `#![forbid(unsafe_code)]`;
//! this binary is the reviewed dedicated-module exception required by
//! `crates/AGENTS.md` and recorded in ADR-0022. The invariant it preserves:
//! **the `vesper_sandbox` library is 100% safe code** — it never links or
//! runs unsafe code itself; it spawns this binary through safe
//! `std::process`, and all kernel interaction happens in here, inside
//! namespaces the parent never entered.
//!
//! # Protocol (both modes write diagnostics to stderr, never stdout)
//!
//! * `probe` — capture caller IDs, create user+mount+PID+net namespaces,
//!   exercise a private tmpfs root, chroot and irreversible capability drops.
//!   Print `linux-namespaces available available available` only on success.
//! * `hold --root <dir> [--env K=V]…` — construct a namespace-local tmpfs
//!   root with a non-recursive workspace bind and read-only system trees.
//!   Print `ready <pid>`, read one `<cwd><US>argv0<US>argv1…` run line, and
//!   fork the PID-namespace init. The child enters the private root, mounts
//!   its own procfs, drops all capabilities and applies no-new-privileges
//!   before executing the absolute payload with exactly the allowlisted env.
//!   Killing the supervisor chains PDEATHSIG into PID-1 death and namespace
//!   descendant termination. Empty staging directories remain in artifacts;
//!   namespace mounts vanish when their last process exits.
//! * anything else — usage on stderr, exit 2. There is no fallback shell.

#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

#[cfg(target_os = "linux")]
use std::ffi::CString;

/// ASCII unit separator used by the run-line protocol.
#[cfg(target_os = "linux")]
const US: char = '\x1f';

/// Linux namespace supervisor entry point.
#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or_default();
    let outcome = match mode {
        "probe" => sys::probe(),
        "hold" => match HoldArgs::parse(&args[1..]) {
            Ok(hold) => sys::hold(hold),
            Err(message) => Err(format!("hold arguments: {message}")),
        },
        _ => Err(format!(
            "usage: sandbox_init probe | hold --root <dir> [--env K=V]… (got {mode:?})"
        )),
    };
    if let Err(message) = outcome {
        eprintln!("sandbox_init: {message}");
        std::process::exit(1);
    }
}

/// The supervisor has no implementation away from Linux. Keeping an inert
/// executable target lets workspace-wide test builds exercise the honest
/// library stub on those platforms without linking Linux syscalls.
#[cfg(not(target_os = "linux"))]
fn main() {}

/// Parsed `hold` arguments.
#[cfg(target_os = "linux")]
struct HoldArgs {
    root: CString,
    env: Vec<(CString, CString)>,
}

#[cfg(target_os = "linux")]
impl HoldArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut root: Option<CString> = None;
        let mut env: Vec<(CString, CString)> = Vec::new();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--root" => {
                    index += 1;
                    let value = args.get(index).ok_or("--root needs a value")?;
                    root = Some(
                        CString::new(value.as_str())
                            .map_err(|_| format!("--root {value:?} contains a NUL byte"))?,
                    );
                }
                "--env" => {
                    index += 1;
                    let value = args.get(index).ok_or("--env needs a K=V value")?;
                    let (name, assigned) = value
                        .split_once('=')
                        .ok_or_else(|| format!("--env {value:?} is not K=V"))?;
                    if name.is_empty() || name.contains('\0') || assigned.contains('\0') {
                        return Err(format!("--env {value:?} has invalid bytes"));
                    }
                    env.push((
                        CString::new(name).map_err(|_| "env name NUL")?,
                        CString::new(assigned).map_err(|_| "env value NUL")?,
                    ));
                }
                other => return Err(format!("unknown argument {other:?}")),
            }
            index += 1;
        }
        Ok(Self {
            root: root.ok_or("--root is required")?,
            env,
        })
    }
}

#[cfg(target_os = "linux")]
mod sys {
    //! Raw syscall wrappers. Every `unsafe` block carries a `SAFETY:` comment
    //! stating the invariant that makes the FFI call sound at that point.

    use std::ffi::{CStr, CString, c_char, c_void};
    use std::fs;
    use std::io::{Read as _, Write as _};

    use super::{HoldArgs, US};

    pub(super) const CLONE_NEWNS: i32 = 0x0002_0000;
    pub(super) const CLONE_NEWPID: i32 = 0x2000_0000;
    pub(super) const CLONE_NEWNET: i32 = 0x4000_0000;
    pub(super) const CLONE_NEWUSER: i32 = 0x1000_0000;

    pub(super) const MS_BIND: u64 = 4096;
    pub(super) const MS_REMOUNT: u64 = 32;
    pub(super) const MS_RDONLY: u64 = 1;
    pub(super) const MS_REC: u64 = 16384;
    pub(super) const MS_PRIVATE: u64 = 1 << 18;

    pub(super) const PR_SET_PDEATHSIG: i32 = 1;
    pub(super) const SIGKILL: i32 = 9;

    unsafe extern "C" {
        fn unshare(flags: i32) -> i32;
        fn mount(
            source: *const c_char,
            target: *const c_char,
            fstype: *const c_char,
            flags: u64,
            data: *const c_void,
        ) -> i32;
        fn getuid() -> u32;
        fn getgid() -> u32;
        fn prctl(option: i32, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> i32;
        fn fork() -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn execve(
            path: *const c_char,
            argv: *const *const c_char,
            envp: *const *const c_char,
        ) -> i32;
    }

    /// Writes one small proc file inside the fresh user namespace.
    fn write_proc(path: &str, contents: &str) -> Result<(), String> {
        fs::write(path, contents).map_err(|error| format!("write {path}: {error}"))
    }

    /// `unshare(USER|NS|PID|NET)` plus one-shot uid/gid mapping. After this
    /// returns Ok the caller is root inside its own user namespace and every
    /// subsequent mount is confined to the private mount namespace.
    fn enter_namespaces() -> Result<(), String> {
        // Capture IDs before unshare: unmapped IDs become the overflow ID in
        // the new namespace and cannot be used to map the actual caller.
        // SAFETY: plain integer reads with no preconditions.
        let outer_uid = unsafe { getuid() };
        // SAFETY: plain integer read with no preconditions.
        let outer_gid = unsafe { getgid() };
        let flags = CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET;
        // SAFETY: `unshare` takes a plain integer flag set; no pointers, no
        // memory the caller must keep alive. Failure is a -1 return.
        let rc = unsafe { unshare(flags) };
        if rc == -1 {
            return Err("unshare(CLONE_NEWUSER|NEWNS|NEWPID|NEWNET) failed".into());
        }
        write_proc("/proc/self/setgroups", "deny")?;
        write_proc("/proc/self/uid_map", &format!("0 {} 1\n", outer_uid))?;
        write_proc("/proc/self/gid_map", &format!("0 {} 1\n", outer_gid))?;
        Ok(())
    }

    /// Makes the whole mount tree private so nothing propagates to the host.
    fn make_mounts_private() -> Result<(), String> {
        let root = CString::new("/").map_err(|_| "root path NUL")?;
        // SAFETY: valid C string owned for the call; NULL fstype and data are
        // accepted by mount(2) for remount/private operations.
        let rc = unsafe {
            mount(
                std::ptr::null(),
                root.as_ptr(),
                std::ptr::null(),
                MS_REC | MS_PRIVATE,
                std::ptr::null(),
            )
        };
        if rc == -1 {
            return Err("mount(/, MS_REC|MS_PRIVATE) failed".into());
        }
        Ok(())
    }

    /// Non-recursive bind: nested host mounts never enter the private root.
    fn bind(source: &CStr, path: &CStr, read_only: bool) -> Result<(), String> {
        // SAFETY: caller-owned valid C string used for both source and
        // target; NULL fstype and data are valid for bind mounts.
        let rc = unsafe {
            mount(
                source.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                MS_BIND,
                std::ptr::null(),
            )
        };
        if rc == -1 {
            return Err(format!(
                "bind {} onto itself failed",
                path.to_string_lossy()
            ));
        }
        if read_only {
            // SAFETY: same valid pointers; the remount flag set is the
            // documented way to tighten an existing bind mount read-only.
            let rc = unsafe {
                mount(
                    std::ptr::null(),
                    path.as_ptr(),
                    std::ptr::null(),
                    MS_BIND | MS_REMOUNT | MS_RDONLY,
                    std::ptr::null(),
                )
            };
            if rc == -1 {
                return Err(format!(
                    "remount {} read-only failed",
                    path.to_string_lossy()
                ));
            }
        }
        Ok(())
    }

    /// Mounts a fresh tmpfs at `path`.
    fn mount_tmpfs(path: &CStr) -> Result<(), String> {
        // SAFETY: valid C strings for source/target/fstype; `data` NULL.
        let rc = unsafe {
            mount(
                c"none".as_ptr(),
                path.as_ptr(),
                c"tmpfs".as_ptr(),
                0,
                std::ptr::null(),
            )
        };
        if rc == -1 {
            return Err(format!("tmpfs at {} failed", path.to_string_lossy()));
        }
        Ok(())
    }

    /// Mounts a fresh procfs at `/proc`.
    fn mount_proc() -> Result<(), String> {
        let target = CString::new("/proc").map_err(|_| "proc path NUL")?;
        // SAFETY: valid C strings; fstype "proc"; NULL data.
        let rc = unsafe {
            mount(
                c"none".as_ptr(),
                target.as_ptr(),
                c"proc".as_ptr(),
                0,
                std::ptr::null(),
            )
        };
        if rc == -1 {
            return Err("proc mount failed".into());
        }
        Ok(())
    }

    /// Build a private root on a namespace-local tmpfs. Only the granted
    /// workspace and selected system paths are mounted into it. The empty
    /// staging directory remains among the explicitly retained worker artifacts.
    fn private_root(workspace: &CStr) -> Result<CString, String> {
        let root = std::path::Path::new(workspace.to_str().map_err(|_| "root UTF-8")?);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "clock before epoch")?
            .as_nanos();
        let staging = root.join(format!(".vesper-ns-{}-{stamp}", std::process::id()));
        fs::create_dir(&staging).map_err(|error| format!("create private root: {error}"))?;
        let jail = CString::new(staging.to_str().ok_or("root UTF-8")?).map_err(|_| "root NUL")?;
        mount_tmpfs(&jail)?;
        for name in [
            "workspace",
            "usr",
            "bin",
            "lib",
            "lib64",
            "etc",
            "dev",
            "tmp",
            "proc",
        ] {
            fs::create_dir(staging.join(name)).map_err(|error| error.to_string())?;
        }
        let target = CString::new(staging.join("workspace").to_str().ok_or("root UTF-8")?)
            .map_err(|_| "root NUL")?;
        bind(workspace, &target, false)?;
        for source in ["/usr", "/bin", "/lib", "/lib64", "/etc"] {
            if !std::path::Path::new(source).exists() {
                continue;
            }
            let target = CString::new(format!("{}{source}", jail.to_string_lossy()))
                .map_err(|_| "mount NUL")?;
            bind(
                &CString::new(source).map_err(|_| "mount NUL")?,
                &target,
                true,
            )?;
        }
        for source in ["/dev/null", "/dev/zero", "/dev/random", "/dev/urandom"] {
            let target = staging.join(source.trim_start_matches('/'));
            fs::File::create(&target).map_err(|error| error.to_string())?;
            bind(
                &CString::new(source).map_err(|_| "device NUL")?,
                &CString::new(target.to_str().ok_or("device UTF-8")?).map_err(|_| "device NUL")?,
                false,
            )?;
        }
        Ok(jail)
    }

    fn enter_root(jail: &CStr) -> Result<(), String> {
        // SAFETY: jail is an owned valid NUL-terminated pathname; this is the
        // single-threaded supervisor child, never the host/library process.
        if unsafe { libc::chroot(jail.as_ptr()) } != 0 {
            return Err("private root entry failed".into());
        }
        std::env::set_current_dir("/").map_err(|error| error.to_string())
    }

    fn drop_privileges() -> Result<(), String> {
        // SAFETY: prctl takes only scalar constants for these operations.
        if unsafe { prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err("no-new-privileges failed".into());
        }
        for capability in 0..64 {
            // SAFETY: scalar capability number, no pointer arguments.
            if unsafe { prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) } != 0
                && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINVAL)
            {
                return Err("capability bounding-set drop failed".into());
            }
        }
        #[repr(C)]
        struct Header {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Data {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let header = Header {
            version: 0x2008_0522,
            pid: 0,
        };
        let data = [Data {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        // SAFETY: Linux capability v3 requires the initialized header and two
        // contiguous data records; both buffers outlive this synchronous call.
        if unsafe { libc::syscall(libc::SYS_capset, &header, data.as_ptr()) } != 0 {
            return Err("capability drop failed".into());
        }
        Ok(())
    }

    /// Reads the entire stdin run line (single line, unit-separated).
    fn read_run_line() -> Result<String, String> {
        let mut line = String::new();
        std::io::stdin()
            .read_to_string(&mut line)
            .map_err(|error| format!("read run line: {error}"))?;
        Ok(line)
    }

    /// Parses `<cwd><US>argv0<US>argv1…` into (cwd, argv) C strings.
    fn parse_run_line(line: &str) -> Result<(CString, Vec<CString>), String> {
        let mut parts = line.trim_end_matches(['\n', '\r']).split(US);
        let cwd = parts
            .next()
            .ok_or("run line is missing a working directory")?;
        let mut argv = Vec::new();
        for argument in parts {
            argv.push(
                CString::new(argument)
                    .map_err(|_| format!("argument {argument:?} contains a NUL byte"))?,
            );
        }
        if argv.is_empty() {
            return Err("run line is missing argv".into());
        }
        let cwd = CString::new(cwd).map_err(|_| "cwd contains a NUL byte")?;
        Ok((cwd, argv))
    }

    /// `probe` mode: prove every namespace provisions, mount a tmpfs, and
    /// report honestly. Never executes a payload.
    pub(super) fn probe() -> Result<(), String> {
        enter_namespaces()?;
        make_mounts_private()?;
        let tmp = CString::new("/tmp").map_err(|_| "tmp path NUL")?;
        mount_tmpfs(&tmp)?;
        // SAFETY: plain integer read with no preconditions or side effects.
        let uid = unsafe { getuid() };
        if uid != 0 {
            return Err(format!("userns mapping failed: uid is {uid}, expected 0"));
        }
        fs::create_dir("/tmp/probe-work").map_err(|error| error.to_string())?;
        let jail = private_root(c"/tmp/probe-work")?;
        enter_root(&jail)?;
        drop_privileges()?;
        println!("linux-namespaces available available available");
        Ok(())
    }

    /// `hold` mode: provision everything, report ready, run one payload as
    /// PID 1 of the PID namespace, and relay its exit status.
    pub(super) fn hold(args: HoldArgs) -> Result<(), String> {
        enter_namespaces()?;
        make_mounts_private()?;

        let jail = private_root(&args.root)?;

        // Report readiness BEFORE reading stdin: the library treats the
        // `ready <pid>` line as the provision-success handshake.
        println!("ready {}", std::process::id());
        let _ = std::io::stdout().flush();

        let line = read_run_line()?;
        if line.is_empty() {
            // stdin closed with no command: exit quietly; the library sees
            // EOF-driven teardown as normal teardown, not an error.
            return Ok(());
        }
        let (cwd, argv) = parse_run_line(&line)?;
        let root_path = std::path::Path::new(args.root.to_str().map_err(|_| "root UTF-8")?);
        let cwd_path = std::path::Path::new(cwd.to_str().map_err(|_| "cwd UTF-8")?);
        let relative = cwd_path
            .strip_prefix(root_path)
            .map_err(|_| "cwd outside grant")?;
        if relative.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            return Err("cwd traverses outside grant".into());
        }
        let cwd = std::path::Path::new("/workspace").join(relative);
        let parent_pid = std::process::id();

        // Build the payload's exact environment: the allowlist only.
        let mut env_cstrings: Vec<CString> = Vec::with_capacity(args.env.len());
        for (name, value) in &args.env {
            let mut joined = name.clone().into_bytes();
            joined.push(b'=');
            joined.extend_from_slice(value.as_bytes());
            env_cstrings.push(CString::new(joined).map_err(|_| "env NUL")?);
        }

        // execve wants NULL-terminated argv/envp arrays of C pointers.
        let mut argv_ptrs: Vec<*const c_char> = argv.iter().map(|a| a.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        let mut envp_ptrs: Vec<*const c_char> = env_cstrings.iter().map(|e| e.as_ptr()).collect();
        envp_ptrs.push(std::ptr::null());

        // SAFETY: the child dies with this process (PR_SET_PDEATHSIG
        // installed below), which makes PID-namespace-init death the
        // guaranteed teardown path; fork has no other preconditions.
        let pid = unsafe { fork() };
        if pid == -1 {
            return Err("fork failed".into());
        }
        if pid == 0 {
            // Child: PID 1 of the new PID namespace.
            // SAFETY: plain prctl constant + signal number.
            let rc = unsafe { prctl(PR_SET_PDEATHSIG, SIGKILL as usize, 0, 0, 0) };
            if rc == -1 {
                // Cannot arrange parent-death cleanup: refuse to run.
                // SAFETY: immediate process termination; no state to clean.
                unsafe { _exit(127) };
            }
            // If the parent died before PDEATHSIG was installed, refuse
            // before mounting procfs replaces the outer PID view.
            if !std::path::Path::new(&format!("/proc/{parent_pid}")).exists()
                || enter_root(&jail).is_err()
                || mount_proc().is_err()
                || drop_privileges().is_err()
                || std::env::set_current_dir(&cwd).is_err()
            {
                // SAFETY: immediate exit before any payload is executed.
                unsafe { _exit(126) };
            }
            // SAFETY: argv/envp are NULL-terminated arrays of valid C
            // pointers into CStrings that outlive the call; execve only
            // returns on failure (-1), which we convert to an exit code.
            let rc = unsafe { execve(argv_ptrs[0], argv_ptrs.as_ptr(), envp_ptrs.as_ptr()) };
            let _ = writeln!(
                std::io::stderr(),
                "sandbox_init: exec {}: rc={rc}",
                argv_ptrs[0] as i64
            );
            // SAFETY: exec failed; exiting is the only sound action.
            unsafe { _exit(127) };
        }
        // Parent: wait for the payload (PID 1) and relay its status.
        let mut status: i32 = 0;
        // SAFETY: `status` is a valid out-pointer; pid is our child.
        let reaped = unsafe { waitpid(pid, &mut status, 0) };
        if reaped == -1 {
            return Err("waitpid failed".into());
        }
        if libc_wifexited(status) {
            let code = (status >> 8) & 0xff;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        } else {
            // Signalled payload (including our own SIGKILL teardown):
            // mirror the signal-shaped exit without inventing a code.
            std::process::exit(128 + (status & 0x7f));
        }
    }

    /// POSIX WIFEXITED without libc bindings: status came from waitpid.
    fn libc_wifexited(status: i32) -> bool {
        (status & 0x7f) == 0
    }
}

/// Unit test for the run-line parser lives behind `#[cfg(test)]` so the
/// supervisor binary carries its own protocol regression check.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::US;

    #[test]
    fn run_line_round_trips_paths_with_spaces() {
        let line = format!("/tmp/some dir{US}/bin/sh{US}-c{US}echo 'a b'");
        let parts: Vec<&str> = line.split(US).collect();
        assert_eq!(parts[0], "/tmp/some dir");
        assert_eq!(parts[1], "/bin/sh");
        assert_eq!(parts[3], "echo 'a b'");
    }
}
