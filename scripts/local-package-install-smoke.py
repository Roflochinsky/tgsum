"""Own-OS synthetic install/update qualification; never attaches to other apps.

cargo install --path src-tauri --debug --locked --root /tmp/tgsum-install-check
uv run --with websocket-client python scripts/local-package-install-smoke.py /tmp/tgsum-install-check
"""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from ui_webkit import Inspector

REPO = Path(__file__).resolve().parents[1]


def main():
    prefix = Path(sys.argv[1]).resolve()
    binary = prefix / "bin/tgsum"
    assert binary.is_file()
    root = Path(tempfile.mkdtemp(prefix="tgsum-install-profile-"))
    for name in ["data", "config", "cache", "input", "output", "system-data"]:
        (root / name).mkdir()
    (root / "input/result.json").write_text(json.dumps({"id": 1, "messages": [{"id": 1, "text": "Synthetic installed app password=INSTALL_SECRET"}]}))
    with socket.socket() as available:
        available.bind(("127.0.0.1", 0))
        port = available.getsockname()[1]
    env = dict(os.environ, XDG_DATA_HOME=str(root / "data"), XDG_CONFIG_HOME=str(root / "config"),
               XDG_CACHE_HOME=str(root / "cache"), CARGO_INSTALL_ROOT=str(prefix),
               WEBKIT_INSPECTOR_HTTP_SERVER=f"127.0.0.1:{port}")
    env.pop("TGSUM_DESKTOP_E2E_ROOT", None)
    env.pop("TGSUM_ANALYSIS_FIXTURE", None)
    def start():
        log = (root / "app.log").open("a")
        child = subprocess.Popen([str(binary)], cwd=REPO, env=env, stdout=log, stderr=log)
        log.close()
        try:
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                assert child.poll() is None, "Installed app exited"
                try:
                    ui = Inspector(port)
                    ui.wait('!!window.__TAURI__?.core && !!document.body')
                    return child, ui
                except (ConnectionError, OSError, AttributeError):
                    time.sleep(.2)
            raise AssertionError("Inspector startup timeout")
        except BaseException:
            child.terminate()
            child.wait(timeout=10)
            raise

    def invoke(ui, command, args=None):
        ui.evaluate('window.__installReply=null;window.__TAURI__.core.invoke(' + json.dumps(command) + ',' + json.dumps(args or {}) + ').then(value=>window.__installReply={value},error=>window.__installReply={error})')
        ui.wait('window.__installReply!==null')
        result = ui.evaluate('window.__installReply')
        assert "error" not in result, (command, result)
        return result["value"]

    child, ui = start()
    try:
        assert invoke(ui, "list_projects") == []
        assert invoke(ui, "analysis_catalog")["fixtures"] is False
        project = invoke(ui, "create_project", {"name": "Synthetic installed project"})
        pid = project["project_id"]
        project = invoke(ui, "update_project", {"projectId": pid, "expectedRevision": project["revision"], "change": {
            "kind": "source", "value": {"source_id": "synthetic", "connector_id": "telegram_json", "scope": {
                "platform": "telegram", "account_local_id": "synthetic", "conversation_id": "1"},
                "archive_path": str(root / "input/result.json"), "latest_snapshot_id": None}}})
        invoke(ui, "configure_local_package", {"projectId": pid, "expectedRevision": project["revision"], "settings": {
            "source_id": "synthetic", "input_directory": str(root / "input"), "output_directory": str(root / "output"),
            "automatic": False, "include_images": True, "include_office": True, "github_repository": None}})
        ready = invoke(ui, "refresh_local_package", {"projectId": pid})
        assert ready["phase"] == "ready"
        text = (Path(ready["ready"]["directory"]) / "context-00001.md").read_text()
        assert "INSTALL_SECRET" not in text
        invoke(ui, "cancel_local_package", {"projectId": pid})
        launcher = root / "data/applications/tgsum.desktop"
        deadline = time.monotonic() + 5
        while not launcher.exists() and time.monotonic() < deadline:
            time.sleep(.1)
        system_entries = [Path(d) / "applications/tgsum.desktop" for d in env.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share").split(":")]
        packaged = any(p.is_file() and "X-Tgsum-Generated=true" not in p.read_text() for p in system_entries)
        if packaged:
            assert not launcher.exists(), "Installed package's launcher must not be shadowed"
        else:
            assert str(binary) in launcher.read_text()
    finally:
        ui.ws.close()
        child.terminate()
        child.wait(timeout=10)
    subprocess.run(["cargo", "install", "--path", "src-tauri", "--debug", "--locked", "--offline", "--force", "--root", str(prefix)], cwd=REPO, check=True)
    child, ui = start()
    try:
        state = invoke(ui, "local_package_status", {"projectId": pid})
        assert state["ready"] == ready["ready"]
        assert state["phase"] == "paused" and state["settings"]["automatic"] is False
        repeat = invoke(ui, "refresh_local_package", {"projectId": pid})
        # Pause changes a watcher flag, so generation may change once; content stays equal.
        assert repeat["ready"]["content_sha256"] == ready["ready"]["content_sha256"]
        assert len(invoke(ui, "list_projects")) == 1
        ui.screenshot(root / "installed.png")
        (root / "report.json").write_text(json.dumps({"status": "passed", "prefix": str(prefix),
            "checks": ["cargo install debug", "ordinary production backend", "desktop launcher ownership respected", "local package privacy",
                       "cargo update offline", "restart preserves project and pause", "repeat keeps content"]}, indent=2))
        print("PASS install/update/restart: " + str(root / "report.json"), flush=True)
    finally:
        ui.ws.close()
        child.terminate()
        child.wait(timeout=10)


if __name__ == "__main__":
    main()
