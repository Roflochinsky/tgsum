//! Deliberate read/write/exec attempts inside the production egress namespace.
//! Host paths refer exclusively to throwaway synthetic test data.
use std::{fs, path::Path};

pub fn inspect() {
    let spec: serde_json::Value =
        serde_json::from_slice(&fs::read("/runtime/hostile-host.json").unwrap()).unwrap();
    for path in spec["paths"].as_array().unwrap() {
        let path = path.as_str().unwrap();
        assert!(!Path::new(path).exists(), "host path is visible");
        assert!(fs::read(path).is_err(), "host path is readable");
        // A same-named file may be CREATED in the private /tmp. Opening an
        // existing host file for overwrite must fail; paths do not share inodes.
        assert!(
            fs::OpenOptions::new().write(true).open(path).is_err(),
            "host path is writable"
        );
        for prefix in ["/proc/1/root", "/proc/self/root"] {
            assert!(
                fs::read(format!("{prefix}{path}")).is_err(),
                "proc exposes host path"
            );
        }
    }
    assert!(std::os::unix::net::UnixStream::connect(spec["socket"].as_str().unwrap()).is_err());
    for path in [
        "/context/AGENTS.md",
        "/context/CLAUDE.md",
        "/context/.mcp.json",
        "/context/.codex/config.toml",
        "/context/.claude/settings.json",
        "/home/agent/.codex/config.toml",
        "/home/agent/.claude/settings.json",
    ] {
        assert!(
            !Path::new(path).exists(),
            "host instructions/config inherited"
        );
    }
    for entry in fs::read_dir("/context").unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("HOST_CONFIG_MUST_NOT_LOAD"));
        assert!(fs::write(&path, "corrupt").is_err());
    }
    for path in [
        "/context/new.md",
        "/runtime/hostile-host.json",
        "/root-leak",
    ] {
        assert!(
            fs::write(path, "corrupt").is_err(),
            "read-only mount writable"
        );
    }
    for key in [
        "LD_PRELOAD",
        "NODE_OPTIONS",
        "BASH_ENV",
        "TGSUM_SECRET_SENTINEL",
    ] {
        assert!(std::env::var_os(key).is_none(), "inherited environment");
    }
    assert!(std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 0")
        .status()
        .is_err());
    assert!(std::process::Command::new("git")
        .arg("status")
        .status()
        .is_err());
}
