//! Synthetic process fixture. No provider SDKs, account access or host discovery.
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("tgsum-fixture 1"),
        Some("echo") => {
            let mut bytes = Vec::new();
            io::stdin().read_to_end(&mut bytes).unwrap();
            io::stdout().write_all(&bytes).unwrap();
            io::stderr().write_all(b"diagnostic").unwrap();
        }
        Some("args") => {
            for arg in &args[1..] {
                println!("{}:{arg}", arg.len());
            }
        }
        Some("exit") => std::process::exit(17),
        Some("sleep") => hold(),
        Some("flood") => loop {
            let block = [b'x'; 8192];
            io::stdout().write_all(&block).unwrap();
            io::stderr().write_all(&block).unwrap();
        },
        Some("stderr-flood") => loop {
            io::stderr().write_all(&[b'e'; 8192]).unwrap();
        },
        Some("orphan") | Some("tree-sleep") => {
            Command::new("/runtime/agent")
                .arg("detach")
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
            let started = std::time::Instant::now();
            while !std::path::Path::new("/tmp/descendant-ready").exists() {
                assert!(started.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(1));
            }
            // Both modes leave a descendant which retains stdout and stderr.
            if args[0] == "tree-sleep" {
                hold();
            }
        }
        Some("detach") => orphan_fixture(),
        Some("hold") => {
            #[cfg(target_os = "linux")]
            rustix::process::setsid().unwrap();
            println!("descendant-ready");
            io::stdout().flush().unwrap();
            std::fs::write("/tmp/descendant-ready", "ready").unwrap();
            hold();
        }
        Some("inspect") => {
            let cwd = std::env::current_dir().unwrap();
            assert_eq!(cwd, std::path::Path::new("/context"));
            let manifest = std::fs::read_to_string("manifest.json").unwrap();
            assert!(manifest.contains("sanitizer_version"));
            let mut selected = false;
            for entry in std::fs::read_dir(".").unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().into_string().unwrap();
                assert!(name == "manifest.json" || name.starts_with("context-"));
                let text = std::fs::read_to_string(entry.path()).unwrap();
                assert!(!text.contains("SYNTHETIC_SECRET"));
                assert!(!text.contains("private-account"));
                selected |= text.contains("SELECTED");
            }
            assert!(selected);
            assert!(std::fs::write("manifest.json", "corrupt").is_err());
            assert!(std::fs::write("unselected.md", "add").is_err());
            assert!(std::fs::write("/root-leak", "write").is_err());
            for path in &args[1..] {
                assert!(
                    !std::path::Path::new(path).exists(),
                    "host path visible: {path}"
                );
                assert!(std::fs::read(path).is_err(), "host path accessible: {path}");
            }
            for path in ["/proc", "/sys", "/run", "/dev", "/etc"] {
                assert!(!std::path::Path::new(path).exists());
            }
            assert_eq!(std::env::var("HOME").unwrap(), "/home/agent");
            for (key, _) in std::env::vars() {
                assert!(
                    ["HOME", "TMPDIR", "PATH", "LANG", "PWD"].contains(&key.as_str()),
                    "unexpected env {key}"
                );
            }
            assert!(std::fs::read_dir("/home/agent").unwrap().next().is_none());
            assert!(std::fs::read_dir("/tmp").unwrap().next().is_none());
            std::fs::write("/home/agent/private", "scratch").unwrap();
            std::fs::write("/tmp/private", "scratch").unwrap();
            println!("isolated");
        }
        Some("network") => {
            let address = args[1].parse().unwrap();
            assert!(
                std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_err()
            );
            #[cfg(unix)]
            assert!(std::os::unix::net::UnixStream::connect(&args[2]).is_err());
            println!("no host sockets");
        }
        #[cfg(target_os = "linux")]
        Some("fd") => check_closed_fd(args[1].parse().unwrap()),
        _ => std::process::exit(2),
    }
}

fn hold() {
    std::thread::sleep(Duration::from_secs(30));
}

// An intentionally orphaned grandchild tests PID-namespace cleanup, including
// a new session. This behavior is confined to the adversarial process fixture.
#[allow(clippy::zombie_processes)]
fn orphan_fixture() {
    let _child = Command::new("/runtime/agent")
        .arg("hold")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
}

// SAFETY: fcntl accepts any integer descriptor and returns EBADF when closed;
// no Rust BorrowedFd is constructed for a possibly invalid handle. The number
// is an intentionally inheritable synthetic sentinel owned by the test parent.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn check_closed_fd(fd: i32) {
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    println!("fd closed");
}
