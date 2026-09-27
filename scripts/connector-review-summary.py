"""Render an offline --reminders report for a local file or CI job summary.

No source fetching, issue creation or registry writes. Python standard library.
"""
import html
import json
import sys
from pathlib import Path


def cell(value):
    return html.escape(str(value)).replace("|", "&#124;").replace("\n", " ").replace("\r", " ")


def main():
    report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    rows = report["reminders"]
    print("## Connector review reminders\n")
    print(f"As of {cell(report['as_of'])}; upcoming horizon: {cell(report['within_days'])} days.\n")
    print("This checks recorded dates and repository evidence. It does not fetch platform sources or record a successful review.\n")
    if rows:
        print("| Connector | Owner | Review | Due | Days until due | Compiled implementation |")
        print("| --- | --- | --- | --- | --- | --- |")
        labels = {"current": "upcoming", "due": "due / overdue", "unknown": "no current successful review"}
        for row in rows:
            values = [row["connector"], row["owner"], labels[row["state"]],
                      row["next_review_due_at"], row["days_until_due"],
                      "yes" if row["shipping_implementation"] else "no"]
            print("| " + " | ".join(cell(value) for value in values) + " |")
        print()
    else:
        print("No due, unknown or upcoming reviews in this window.\n")
    findings = report["validation"]["findings"]
    if findings:
        print("### Repository validation\n")
        for finding in findings:
            print(f"- {cell(finding['severity'])}: {cell(finding['connector'])} — {cell(finding['code'])}: {cell(finding['message'])}")
        print()
    print("Use `docs/connectors/review-template.md` and record the result in Beads. Expiry requires maintainer review before release; local import remains available.")


if __name__ == "__main__":
    main()
