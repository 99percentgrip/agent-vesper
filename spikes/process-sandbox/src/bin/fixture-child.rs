use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "direct".into());
    match mode.as_str() {
        "direct" => println!("direct-child"),
        "silent" => sleep_forever(),
        "ticker" => loop {
            println!("tick");
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(10));
        },
        "huge" => {
            let block = vec![b'x'; 16 * 1024];
            for _ in 0..256 {
                io::stdout().write_all(&block).unwrap();
                io::stderr().write_all(&block).unwrap();
            }
        }
        "tree" | "detached-looking" => {
            let exe = std::env::current_exe().unwrap();
            let mut child = Command::new(exe)
                .arg("silent")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            println!("descendant_pid={}", child.id());
            io::stdout().flush().unwrap();
            let _ = child.wait();
        }
        "hold-stdout" => {
            let exe = std::env::current_exe().unwrap();
            let child = Command::new(exe)
                .arg("silent")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            println!("descendant_pid={}", child.id());
        }
        "hold-stderr" => {
            let exe = std::env::current_exe().unwrap();
            let child = Command::new(exe)
                .arg("silent")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            eprintln!("descendant_pid={}", child.id());
        }
        "ignore-term" => {
            #[cfg(unix)]
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
            }
            println!("ignoring-term");
            io::stdout().flush().unwrap();
            sleep_forever();
        }
        other => {
            eprintln!("unknown mode: {other}");
            std::process::exit(2);
        }
    }
}
