//! Test-only child of the production relay. All inputs/accounts are synthetic.
#[cfg(target_os = "linux")]
mod hostile_probe;
#[cfg(target_os = "linux")]
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The synthetic HTTPS namespace fixture requires the Linux relay");
    std::process::exit(125);
}

#[cfg(target_os = "linux")]
fn main() {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--version"] {
        assert!(!Path::new("/gateway/proxy.sock").exists());
        assert!(!Path::new("/home/agent/.codex/auth.json").exists());
        std::process::exit(
            Command::new("/runtime/installed-codex")
                .args(args)
                .status()
                .unwrap()
                .code()
                .unwrap_or(1),
        );
    }
    let proxy = std::env::var("HTTPS_PROXY").unwrap();
    hostile_probe::inspect();
    assert!(proxy.starts_with("http://127.0.0.1:"));
    for key in ["HTTP_PROXY", "https_proxy", "http_proxy"] {
        assert_eq!(std::env::var(key).unwrap(), proxy);
    }
    for key in ["NO_PROXY", "no_proxy"] {
        assert_eq!(std::env::var(key).unwrap(), "");
    }
    for key in [
        "ALL_PROXY",
        "all_proxy",
        "OPENAI_API_KEY",
        "CODEX_REFRESH_TOKEN_URL_OVERRIDE",
        "SSH_AUTH_SOCK",
    ] {
        assert!(std::env::var_os(key).is_none());
    }
    let setup: serde_json::Value =
        serde_json::from_slice(&std::fs::read("/runtime/https-test.json").unwrap()).unwrap();
    let address: std::net::SocketAddr = setup["address"].as_str().unwrap().parse().unwrap();
    assert!(
        std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(100))
            .is_err(),
        "host loopback must be unreachable directly"
    );
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let mut inodes = vec![std::fs::metadata("/gateway/proxy.sock").unwrap()];
        if let Ok(auth) = std::fs::metadata("/home/agent/.codex/auth.json") {
            inodes.push(auth);
        }
        for path in ["/proc/self/fd", "/proc/1/fd"] {
            for entry in std::fs::read_dir(path).unwrap() {
                if let Ok(fd) = std::fs::metadata(entry.unwrap().path()) {
                    assert!(inodes
                        .iter()
                        .all(|inode| (fd.dev(), fd.ino()) != (inode.dev(), inode.ino())));
                }
            }
        }
    }
    if setup["mode"] == "refresh-probe" {
        let mut socket =
            std::net::TcpStream::connect(proxy.strip_prefix("http://").unwrap()).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        socket
            .write_all(b"CONNECT auth.openai.com:443 HTTP/1.1\r\nHost: auth.openai.com:443\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        let _ = socket.read_to_end(&mut response);
        assert!(response.is_empty());
        std::process::exit(42);
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut input)
        .unwrap();
    assert!(input.len() <= 1024 * 1024);
    let auth = Path::new("/home/agent/.codex/auth.json").exists();
    if auth {
        assert!(std::fs::write("/home/agent/.codex/auth.json", "overwrite").is_err());
        assert!(!Path::new("/home/agent/.codex/config.toml").exists());
    }
    assert_eq!(args.pop().unwrap(), "-");
    if !auth {
        // Synthetic-auth case retains the production provider configuration.
        let provider = "model_providers.tgsum_fixture={name=\"TGSUM HTTPS fixture\",base_url=\"https://api.openai.com/v1\",wire_api=\"responses\",requires_openai_auth=false,supports_websockets=false,request_max_retries=0,stream_max_retries=0,stream_idle_timeout_ms=30000}";
        args.extend([
            "--config".into(),
            "model_provider=\"tgsum_fixture\"".into(),
            "--config".into(),
            provider.into(),
        ]);
    }
    args.push("-".into());
    let mut child = Command::new("/runtime/installed-codex")
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    std::process::exit(child.wait().unwrap().code().unwrap_or(1));
}
