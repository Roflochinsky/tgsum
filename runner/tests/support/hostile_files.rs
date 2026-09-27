//! Host-side synthetic trap files for native network qualification.
use std::{fs, os::unix::net::UnixListener, path::Path};

pub fn stage(root: &Path, project: &Path) -> UnixListener {
    let repo = root.join("hostile-repo");
    for name in [
        "AGENTS.md",
        "CLAUDE.md",
        ".mcp.json",
        ".git/config",
        ".codex/config.toml",
        ".claude/settings.json",
        "private.txt",
    ] {
        let path = repo.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "HOST_CONFIG_MUST_NOT_LOAD").unwrap();
    }
    let socket = root.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::write(
        root.join("hostile-host.json"),
        serde_json::json!({
            "paths":[repo.join("private.txt"),repo.join(".codex/config.toml"),
                     repo.join(".claude/settings.json"),repo.join(".mcp.json"),project],
            "socket":socket
        })
        .to_string(),
    )
    .unwrap();
    listener
}
