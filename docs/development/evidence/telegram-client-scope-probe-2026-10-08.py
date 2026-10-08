"""Own synthetic user scopes only. Never launch or inspect Telegram."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid


def run(expand_disabled):
    unit = f"tgsum-telegram-scope-check-{uuid.uuid4().hex}.scope"
    literal = "Ж 😀/${TGSUM_SCOPE_LITERAL}"
    with tempfile.TemporaryDirectory(prefix="tgsum-client-scope-") as root:
        root = Path(root)
        profile = root / literal
        profile.mkdir(parents=True)
        receipt = root / "receipt.json"
        program = (
            "import json,os,sys,time;from pathlib import Path;"
            "Path(sys.argv[1]).write_text(json.dumps({'pid':os.getpid(),"
            "'argument':sys.argv[2],'cwd':os.getcwd(),"
            "'cgroup':Path('/proc/self/cgroup').read_text()}));time.sleep(2)"
        )
        command = [
            "/usr/bin/systemd-run", "--user", "--scope", "--quiet",
            "--collect", "--no-ask-password",
        ]
        if expand_disabled:
            command.append("--expand-environment=no")
        command += [f"--unit={unit}", "--", sys.executable, "-c", program,
                    str(receipt), str(profile)]
        environment = os.environ.copy()
        environment["XDG_RUNTIME_DIR"] = f"/run/user/{os.getuid()}"
        environment["DBUS_SESSION_BUS_ADDRESS"] = (
            f"unix:path=/run/user/{os.getuid()}/bus"
        )
        environment["TGSUM_SCOPE_LITERAL"] = "synthetic-expanded-value"
        process = subprocess.Popen(
            command, cwd=profile, env=environment,
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            deadline = time.monotonic() + 8
            while not receipt.exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("Owned synthetic scope did not start")
                time.sleep(0.02)
            observed = json.loads(receipt.read_text())
            caller_group = Path("/proc/self/cgroup").read_text()
            result = {
                "command_pid_equals_spawn_pid": observed["pid"] == process.pid,
                "command_in_separate_scope": unit in observed["cgroup"],
                "caller_outside_scope": unit not in caller_group,
                "working_directory_preserved": observed["cwd"] == str(profile),
                "literal_argument_preserved": observed["argument"] == str(profile),
            }
            if process.wait(timeout=8) != 0:
                raise RuntimeError("Owned synthetic scope exited unsuccessfully")
            return result
        finally:
            subprocess.run(
                ["/usr/bin/systemctl", "--user", "stop", unit],
                env=environment, stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                timeout=8, check=False,
            )
            if process.poll() is None:
                process.kill()
            process.wait(timeout=8)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    before = run(False)
    after = run(True)
    assert not before["literal_argument_preserved"], before
    assert all(after.values()), after
    report = {
        "date": "2026-10-08",
        "scope": "Own synthetic Python processes and transient user scopes on Arch; no Telegram/account/user configuration changes",
        "status": "passed",
        "without_expand_environment_no": before,
        "with_expand_environment_no": after,
        "mutation": "Literal argument assertion RED without --expand-environment=no; GREEN with the fix",
        "systemd_version": subprocess.check_output(
            ["/usr/bin/systemd-run", "--version"], text=True
        ).splitlines()[0],
        "limitations": [
            "This proves local systemd scope execution, PID preservation, separate cgroup and literal arguments; it does not qualify real Telegram lifecycle or TGSUM runtime integration.",
        ],
        "cleanup": "Both uniquely named own scopes stopped and helpers reaped; temporary profiles removed",
    }
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"status": report["status"], "checks": after}, ensure_ascii=False))


if __name__ == "__main__":
    main()
