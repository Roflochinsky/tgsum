"""Observe only an owned fake GTK process through the real AT-SPI transport."""

import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time


ROOT = Path(__file__).parent
FIXTURE = ROOT / "fixtures" / "atspi-test-window.py"
PROBE = ROOT / "telegram-atspi-probe.py"
PRIVATE = ("Private Customer Alice", "Another private label", "session-private", "123456789")


def call_probe(pid, executable):
    return subprocess.run(
        [sys.executable, str(PROBE), "--pid", str(pid), "--executable", str(executable),
         "--stage", "chat", "--observe"],
        capture_output=True, text=True, timeout=40, check=False,
    )


def main():
    fake = subprocess.Popen(
        [sys.executable, str(FIXTURE)], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, env={**os.environ, "NO_AT_BRIDGE": "0"},
    )
    try:
        ready, _, _ = select.select([fake.stdout], [], [], 15)
        if not ready:
            raise AssertionError("synthetic GTK window did not become ready")
        target = json.loads(fake.stdout.readline())
        assert target["pid"] == fake.pid and Path(target["executable"]).is_file()
        result = None
        for _ in range(10):
            result = call_probe(fake.pid, target["executable"])
            if result.returncode == 0:
                break
            if fake.poll() is not None:
                raise AssertionError("synthetic GTK window exited")
            time.sleep(0.3)
        assert result is not None and result.returncode == 0, "AT-SPI probe failed on fake window"
        report = json.loads(result.stdout)
        assert report["ok"]["schema_version"] == 1
        assert report["ok"]["nodes"], "AT-SPI report is empty"
        body = json.dumps(report)
        assert not any(value in body for value in PRIVATE), "private text or raw ID leaked"

        wrong_executable = call_probe(fake.pid, "/usr/bin/true")
        assert wrong_executable.returncode != 0
        assert json.loads(wrong_executable.stdout)["error"] == "executable_mismatch"
        wrong_pid = call_probe(os.getpid(), target["executable"])
        assert wrong_pid.returncode != 0
        assert json.loads(wrong_pid.stdout)["error"] == "application_not_unique_or_not_accessible"
        print("Linux AT-SPI synthetic native-window transport: PASS")
    finally:
        fake.terminate()
        try:
            fake.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            fake.kill()
            fake.communicate(timeout=3)


if __name__ == "__main__":
    main()
