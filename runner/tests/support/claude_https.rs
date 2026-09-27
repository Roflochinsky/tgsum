//! Test-only child of the production relay; no key or endpoint override.
#[cfg(target_os = "linux")]
mod hostile_probe;
#[cfg(target_os = "linux")]
use std::{path::Path, process::Command, time::Duration};

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The synthetic HTTPS namespace fixture requires the Linux relay");
    std::process::exit(125);
}

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--version"] {
        assert!(!Path::new("/gateway/proxy.sock").exists());
        assert!(!Path::new("/home/agent/.claude/.credentials.json").exists());
    } else {
        hostile_probe::inspect();
        assert_eq!(std::fs::read_dir("/context").unwrap().count(), 0);
        let proxy = std::env::var("HTTPS_PROXY").unwrap();
        assert!(proxy.starts_with("http://127.0.0.1:"));
        for name in ["HTTP_PROXY", "https_proxy", "http_proxy"] {
            assert_eq!(std::env::var(name).unwrap(), proxy);
        }
        for name in ["NO_PROXY", "no_proxy"] {
            assert_eq!(std::env::var(name).unwrap(), "");
        }
        for name in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ALL_PROXY",
            "all_proxy",
            "SSH_AUTH_SOCK",
            "NODE_TLS_REJECT_UNAUTHORIZED",
        ] {
            assert!(std::env::var_os(name).is_none());
        }
        assert_eq!(
            std::env::var("CLAUDE_CONFIG_DIR").unwrap(),
            "/home/agent/.claude"
        );
        assert_eq!(
            std::env::var("NODE_EXTRA_CA_CERTS").unwrap(),
            "/runtime/provider-ca.pem"
        );
        let config = Path::new("/home/agent/.claude");
        assert_eq!(std::fs::read_dir(config).unwrap().count(), 1);
        assert!(std::fs::write(config.join(".credentials.json"), "overwrite").is_err());
        assert!(std::fs::remove_file(config.join(".credentials.json")).is_err());
        let address: std::net::SocketAddr = std::fs::read_to_string("/runtime/peer-address")
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err()
        );
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let pinned = [
                "/gateway/proxy.sock",
                "/home/agent/.claude/.credentials.json",
            ]
            .map(|p| std::fs::metadata(p).unwrap());
            for path in ["/proc/self/fd", "/proc/1/fd"] {
                for entry in std::fs::read_dir(path).unwrap() {
                    if let Ok(fd) = std::fs::metadata(entry.unwrap().path()) {
                        assert!(pinned
                            .iter()
                            .all(|m| (m.dev(), m.ino()) != (fd.dev(), fd.ino())));
                    }
                }
            }
        }
    }
    let status = Command::new("/runtime/installed-claude")
        .args(args)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(125));
}
