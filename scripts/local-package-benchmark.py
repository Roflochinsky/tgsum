"""Generate synthetic fixtures, then measure the already built local-package probe.

cargo build --locked -p tgsum-core --example local_package_probe
python scripts/local-package-benchmark.py --report /tmp/tgsum-package-benchmark.json
"""
import argparse
import json
from pathlib import Path
import platform
import subprocess
import tempfile
import zipfile

REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    report = {"platform": platform.platform(), "build": "debug, locked",
              "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
              "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO)), "runs": []}
    with tempfile.TemporaryDirectory(prefix="tgsum-package-benchmark-") as temp:
        root = Path(temp)
        text = "Synthetic body 🌍. " * 32
        for case in ["large-chat", "many-chats", "mixed-files"]:
            folder = root / case
            folder.mkdir()
            messages = [{"id": i + 1, "text": text} for i in range(20000 if case == "large-chat" else 50)]
            if case == "large-chat":
                messages[0]["text"] = "Long message 🌍. " * 65536
            if case == "mixed-files":
                for i in range(100):
                    name = f"image-{i}.png"
                    (folder / name).write_bytes(b"SYNTHETIC_IMAGE" * 16384)
                    messages.append({"id": len(messages) + 1, "text": "picture", "photo": name})
                for i in range(30):
                    name = f"note-{i}.txt"
                    (folder / name).write_text("Text attachment password=SYNTHETIC_SECRET\n" * 512)
                    messages.append({"id": len(messages) + 1, "text": "note", "file": name})
                for ext, part in [("docx", "word/document.xml"), ("xlsx", "xl/workbook.xml")]:
                    name = f"document.{ext}"
                    with zipfile.ZipFile(folder / name, "w", zipfile.ZIP_DEFLATED) as office:
                        office.writestr("[Content_Types].xml", "<Types/>")
                        office.writestr(part, "<document><p><t>Иванов Иван Иванович</t></p></document>")
                    messages.append({"id": len(messages) + 1, "text": "office", "file": name})
            archive = {"id": 1, "messages": messages}
            if case == "many-chats":
                archive = {"chats": {"list": [{"id": i + 1, "messages": messages} for i in range(500)]}}
            source = folder / "result.json"
            source.write_text(json.dumps(archive, ensure_ascii=False), encoding="utf-8")
            for mode in (["ready", "cancel"] if case == "large-chat" else ["ready"]):
                command = [str(REPO / "target/debug/examples/local_package_probe"), str(folder)]
                if mode == "cancel":
                    command.append("cancel")
                result = subprocess.run(command, cwd=REPO, check=True, capture_output=True, text=True)
                record = dict(json.loads(result.stdout), case=case, input_json_bytes=source.stat().st_size)
                report["runs"].append(record)
                print(json.dumps(record), flush=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
