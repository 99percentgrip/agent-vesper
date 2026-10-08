#![cfg(unix)]
#![allow(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;

const WIDTH: usize = 120;
const HEIGHT: usize = 36;

#[test]
fn rrc_child_output_is_captured_sanitized_and_confined_in_a_real_pty() {
    let fixture = tempfile::tempdir().expect("fixture");
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(fixture.path())
            .status()
            .expect("git init")
            .success()
    );
    let producer = fixture.path().join("noisy-release-gate.sh");
    fs::write(
        &producer,
        r#"#!/bin/sh
index=0
while [ "$index" -lt 48 ]; do
  printf 'Compiling crate-%02d with a deliberately overlong path /workspace/target/debug/deps/abcdefghijklmnopqrstuvwxyz0123456789\n' "$index"
  printf 'Running test-%02d...\n' "$index" >&2
  index=$((index + 1))
  sleep 0.01
done
printf '\033[2JCHILD_CLEAR_MARKER\n'
printf '\033]0;CHILD_TITLE\007OSC_MARKER\n' >&2
printf 'Progress 10%%\rProgress 90%%\rProgress 100%%\n'
printf 'control:\001\002\003:end\n' >&2
# End each concurrently drained stream with the same generic telemetry marker,
# then a terminal-control and carriage-return boundary. Either valid reader
# ordering therefore leaves every compact proof in the bounded RUN tail.
printf 'TELEMETRY_CAPTURE\n'
printf '\033[2JANSI_CAPTURE CR_CAPTURE\rANSI_CAPTURE CR_CAPTURE\n'
printf 'TELEMETRY_CAPTURE\n' >&2
printf '\033]0;CHILD_TITLE\007ANSI_CAPTURE CR_CAPTURE\rANSI_CAPTURE CR_CAPTURE\n' >&2
sleep 0.40
"#,
    )
    .expect("producer");
    fs::set_permissions(&producer, fs::Permissions::from_mode(0o755)).expect("executable");

    let (master, slave) = open_pty(WIDTH as u16, HEIGHT as u16).expect("open pty");
    let stdin = slave.try_clone().expect("stdin clone");
    let stdout = slave.try_clone().expect("stdout clone");
    let stderr = slave;
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-vesper-tui"));
    command
        .arg("--rrc-terminal-ownership-probe")
        .arg(&producer)
        .current_dir(fixture.path())
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    // SAFETY: only async-signal-safe syscalls execute between fork and exec.
    // Stdio has already duplicated the fixture slave onto stdin. A new
    // session must own that slave so crossterm cannot query or change the
    // invoking release host's controlling terminal through /dev/tty.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("launch TUI in owned PTY");
    drop(command); // Close the parent's slave handles so reader EOF can settle.
    let reader = thread::spawn(move || read_pty(master));
    let status = child.wait().expect("wait for TUI");
    let bytes = reader
        .join()
        .expect("PTY reader thread")
        .expect("PTY bytes");
    assert!(
        status.success(),
        "probe failed: {status}\n{}",
        String::from_utf8_lossy(&bytes)
    );

    assert!(
        bytes.windows(8).any(|part| part == b"\x1b[?1049h"),
        "alternate screen was not entered"
    );
    assert!(
        !contains(&bytes, b"\x1b[2JCHILD_CLEAR_MARKER")
            && !contains(&bytes, b"\x1b[2JCHILD_TERMINAL_CAPTURE_MARKER"),
        "child CSI reached terminal verbatim"
    );
    assert!(
        !contains(&bytes, b"\x1b]0;CHILD_TITLE"),
        "child OSC reached terminal verbatim"
    );
    assert!(
        !contains(&bytes, b"\rProgress"),
        "child carriage-return progress reached terminal verbatim"
    );
    assert!(
        !contains(&bytes, b"\x01\x02\x03"),
        "child control bytes reached terminal verbatim"
    );

    let trace = TerminalTrace::parse(&bytes, WIDTH, HEIGHT);
    let run = trace
        .writes
        .iter()
        .find(|write| write.text == "RUN")
        .unwrap_or_else(|| {
            panic!(
                "RUN heading was not rendered: {}",
                parsed_write_tail(&trace)
            )
        });
    let active = trace
        .writes
        .iter()
        .find(|write| write.text.contains("Release recovery"))
        .unwrap_or_else(|| {
            panic!(
                "registered release worker was not rendered: {}",
                parsed_write_tail(&trace)
            )
        });
    let telemetry = trace
        .writes
        .iter()
        .filter(|write| write.text.contains("TELEMETRY_CAPTURE"))
        .collect::<Vec<_>>();
    assert!(
        !telemetry.is_empty(),
        "paired child marker never reached telemetry: {}",
        parsed_write_tail(&trace)
    );
    assert!(
        telemetry
            .iter()
            .all(|write| write.row >= run.row && write.col >= run.col),
        "telemetry escaped RUN rect: RUN=({}, {}), marker writes={telemetry:?}; {}",
        run.row,
        run.col,
        parsed_write_tail(&trace)
    );
    assert!(
        trace
            .writes
            .iter()
            .any(|write| write.text.contains("ANSI_CAPTURE")),
        "sanitized ANSI payload was not rendered as inert telemetry text: {}",
        parsed_write_tail(&trace)
    );
    assert!(
        trace
            .writes
            .iter()
            .any(|write| write.text.contains("CR_CAPTURE")),
        "carriage-return progress was not captured as bounded telemetry: {}",
        parsed_write_tail(&trace)
    );

    let first_active = trace
        .writes
        .iter()
        .position(|write| write.text.contains("Release recovery"))
        .unwrap();
    let first_ready = trace
        .writes
        .iter()
        .position(|write| write.text.contains("Ready"))
        .unwrap_or_else(|| {
            panic!(
                "idle Ready was not rendered after settlement: {}",
                parsed_write_tail(&trace)
            )
        });
    assert!(
        first_ready > first_active,
        "Ready appeared before the registered worker settled: {}",
        parsed_write_tail(&trace)
    );
    assert!(active.col >= run.col, "active state escaped sidebar");

    assert!(
        trace
            .writes
            .iter()
            .any(|write| write.text.contains("Session")),
        "session layout was never rendered: {}",
        parsed_write_tail(&trace)
    );
    let last_telemetry = trace
        .writes
        .iter()
        .rposition(|write| write.text.contains("TELEMETRY_CAPTURE"))
        .expect("paired telemetry marker write");
    assert!(
        last_telemetry < first_ready,
        "old telemetry was redrawn after the idle settlement frame: {}",
        parsed_write_tail(&trace)
    );
}

fn parsed_write_tail(trace: &TerminalTrace) -> String {
    let start = trace.writes.len().saturating_sub(32);
    trace.writes[start..]
        .iter()
        .map(|write| {
            let text = write.text.chars().take(160).collect::<String>();
            format!("({}, {}) {text:?}", write.row, write.col)
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn open_pty(width: u16, height: u16) -> io::Result<(File, File)> {
    let mut master = -1;
    let mut slave = -1;
    #[cfg(target_os = "macos")]
    let mut size = libc::winsize {
        ws_row: height,
        ws_col: width,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    #[cfg(not(target_os = "macos"))]
    let size = libc::winsize {
        ws_row: height,
        ws_col: width,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    #[cfg(target_os = "macos")]
    let size_ptr = &raw mut size;
    #[cfg(not(target_os = "macos"))]
    let size_ptr = &raw const size;
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            size_ptr,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((unsafe { File::from_raw_fd(master) }, unsafe {
        File::from_raw_fd(slave)
    }))
}

fn read_pty(mut master: File) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                output.extend_from_slice(&buffer[..read]);
                if output.ends_with(b"\x1b[6n") {
                    master.write_all(b"\x1b[1;1R")?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) => return Err(error),
        }
    }
    Ok(output)
}

#[derive(Debug)]
struct TextWrite {
    row: usize,
    col: usize,
    text: String,
}

struct TerminalTrace {
    screen: Vec<Vec<char>>,
    writes: Vec<TextWrite>,
}

impl TerminalTrace {
    fn parse(bytes: &[u8], width: usize, height: usize) -> Self {
        let mut trace = Self {
            screen: vec![vec![' '; width]; height],
            writes: Vec::new(),
        };
        let (mut row, mut col) = (0_usize, 0_usize);
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\x1b' if bytes.get(index + 1) == Some(&b'[') => {
                    let start = index + 2;
                    index = start;
                    while index < bytes.len() && !(0x40..=0x7e).contains(&bytes[index]) {
                        index += 1;
                    }
                    if index >= bytes.len() {
                        break;
                    }
                    let final_byte = bytes[index];
                    let params = String::from_utf8_lossy(&bytes[start..index]);
                    let numbers = params
                        .trim_start_matches('?')
                        .split(';')
                        .map(|value| value.parse::<usize>().unwrap_or(1))
                        .collect::<Vec<_>>();
                    match final_byte {
                        b'H' | b'f' => {
                            row = numbers
                                .first()
                                .copied()
                                .unwrap_or(1)
                                .saturating_sub(1)
                                .min(height - 1);
                            col = numbers
                                .get(1)
                                .copied()
                                .unwrap_or(1)
                                .saturating_sub(1)
                                .min(width - 1);
                        }
                        b'G' => {
                            col = numbers
                                .first()
                                .copied()
                                .unwrap_or(1)
                                .saturating_sub(1)
                                .min(width - 1)
                        }
                        b'd' => {
                            row = numbers
                                .first()
                                .copied()
                                .unwrap_or(1)
                                .saturating_sub(1)
                                .min(height - 1)
                        }
                        b'A' => row = row.saturating_sub(numbers.first().copied().unwrap_or(1)),
                        b'B' => row = (row + numbers.first().copied().unwrap_or(1)).min(height - 1),
                        b'C' => col = (col + numbers.first().copied().unwrap_or(1)).min(width - 1),
                        b'D' => col = col.saturating_sub(numbers.first().copied().unwrap_or(1)),
                        b'J' if numbers.first().copied().unwrap_or(0) == 2 => {
                            trace.screen.iter_mut().for_each(|line| line.fill(' '));
                        }
                        b'K' => trace.screen[row][col..].fill(' '),
                        _ => {}
                    }
                    index += 1;
                }
                b'\x1b' => {
                    index += if index + 1 < bytes.len() { 2 } else { 1 };
                }
                b'\r' => {
                    col = 0;
                    index += 1;
                }
                b'\n' => {
                    row = (row + 1).min(height - 1);
                    index += 1;
                }
                byte if byte < 0x20 || byte == 0x7f => {
                    index += 1;
                }
                _ => {
                    let start_row = row;
                    let start_col = col;
                    let mut text = String::new();
                    while index < bytes.len()
                        && bytes[index] >= 0x20
                        && bytes[index] != 0x7f
                        && bytes[index] != b'\x1b'
                    {
                        let tail = &bytes[index..];
                        let Ok(decoded) = std::str::from_utf8(tail) else {
                            index += 1;
                            continue;
                        };
                        let Some(ch) = decoded.chars().next() else {
                            break;
                        };
                        text.push(ch);
                        if row < height && col < width {
                            trace.screen[row][col] = ch;
                        }
                        col += 1;
                        if col >= width {
                            col = 0;
                            row = (row + 1).min(height - 1);
                        }
                        index += ch.len_utf8();
                    }
                    if !text.is_empty() {
                        trace.writes.push(TextWrite {
                            row: start_row,
                            col: start_col,
                            text,
                        });
                    }
                }
            }
        }
        trace
    }
}
