"""Real Tauri WebView smoke; Python 3.12+ standard library only.

Build: cargo build --locked -p tgsum --features desktop-e2e
Run: python scripts/desktop-e2e.py [--cargo-run] [--report-dir DIR]
On headless Linux use xvfb-run -a. This launches its own isolated application;
it never attaches to existing apps. Pickers, agents and Telegram refresh are synthetic.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import platform
import socket
import subprocess
import tempfile
import time
import traceback
import urllib.error
import urllib.request
import uuid

REPO = Path(__file__).resolve().parents[1]
ELEMENT = "element-6066-11e4-a52e-4f735466cecf"


class WebView:
    def __init__(self, port):
        self.url = f"http://127.0.0.1:{port}"
        self.session = None
        # Ignore proxy env for our owned loopback server.
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(self, method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.url + path, data=data, method=method,
                                         headers={"Content-Type": "application/json"})
        try:
            with self.http.open(request, timeout=35) as response:
                value = json.load(response)["value"]
        except urllib.error.HTTPError as error:
            raise RuntimeError(error.read().decode()) from error
        if isinstance(value, dict) and "error" in value:
            raise RuntimeError(value)
        return value

    def command(self, path, body):
        return self.request("POST", f"/session/{self.session}/{path}", body)

    def evaluate(self, expression):
        return self.command("execute/sync", {"script": "return (" + expression + ")", "args": []})

    def wait(self, expression, timeout=35):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = self.evaluate(expression)
            if value:
                return value
            time.sleep(0.1)
        raise AssertionError("Timed out: " + expression)

    def click(self, selector):
        self.wait("document.querySelector(" + json.dumps(selector) + ") !== null")
        element = self.command("element", {"using": "css selector", "value": selector})
        self.command(f"element/{element[ELEMENT]}/click", {})

    def value(self, selector, value):
        self.command("execute/sync", {"script": """
            const e = document.querySelector(arguments[0]);
            e.value = arguments[1];
            e.dispatchEvent(new Event('input', {bubbles:true}));
            e.dispatchEvent(new Event('change', {bubbles:true}));
        """, "args": [selector, value]})

    def invoke(self, command, args=None):
        reply = self.command("execute/async", {"script": """
            const done = arguments[arguments.length - 1];
            window.__TAURI__.core.invoke(arguments[0], arguments[1])
              .then(value => done({value}), error => done({failure:error}));
        """, "args": [command, args or {}]})
        assert "failure" not in reply, (command, reply)
        return reply["value"]

    def idle(self):
        self.wait("document.body.dataset.screen==='projects' && document.querySelector('#screen-projects').getAttribute('aria-busy')==='false'")

    def stage(self, name):
        self.idle()
        assert self.evaluate("document.querySelector('#project-steps [aria-current]').dataset.go") == name
        assert self.evaluate("[...document.querySelectorAll('[data-project-panel]')].filter(p=>!p.hidden).length") == 1

    def screenshot(self, path):
        png = self.request("GET", f"/session/{self.session}/screenshot")
        path.write_bytes(base64.b64decode(png, validate=True))


def prepare(root, nonce, port):
    full = json.loads((REPO / "core/tests/fixtures/sample-export.json").read_text())
    full["chats"]["list"][0]["messages"][0]["text"] = "hi bob password=SYNTHETIC_E2E_PASSWORD"
    (root / "full.json").write_text(json.dumps(full), encoding="utf-8")
    (root / "single.json").write_text(json.dumps(full["chats"]["list"][1]), encoding="utf-8")
    (root / "malformed.json").write_text('{"chats": [', encoding="utf-8")
    for directory in ["bundle-output", "single-output", "data", "config", "cache"]:
        (root / directory).mkdir()
    picks = [("export", "malformed.json"), ("export", "full.json"),
             ("export", "full.json"), ("output", "bundle-output"),
             ("export", "single.json"), ("export", "single.json"),
             ("output", "single-output"), ("export", "malformed.json")]
    if platform.system() == "Linux":
        picks.insert(5, ("export", "full.json"))
    (root / "harness.json").write_text(json.dumps({"nonce": nonce, "port": port,
        "refresh_fixture": True,
        "picks": [{"kind": kind, "path": path} for kind, path in picks]}), encoding="utf-8")
    (root / "refresh-control.json").write_text(json.dumps({"mode": "locked", "now": 100}), encoding="utf-8")


def exercise(ui, root, report, report_dir):
    def passed(name):
        report["checks"].append(name)
        print("PASS " + name, flush=True)

    assert ui.invoke("analysis_catalog")["fixtures"] is True
    assert ui.invoke("list_projects") == []
    assert ui.evaluate("localStorage.getItem('tgsum.ui.v1')") is None
    ui.command("execute/sync", {"script": """
      window.__e2eErrors=[];
      addEventListener('error',e=>__e2eErrors.push(e.message));
      addEventListener('unhandledrejection',e=>__e2eErrors.push(String(e.reason)));
    """, "args": []})
    ui.wait("document.body.dataset.screen==='start'")
    ui.screenshot(report_dir / "start.png")
    ui.click("#btn-help")
    ui.wait("document.querySelector('#onboarding').open")
    ui.click("#onboarding-next")
    ui.click("#onboarding-next")
    ui.click("#onboarding details summary")
    ui.wait("document.querySelectorAll('#onboarding-agent option').length===3")
    assert ui.evaluate("document.querySelector('#onboarding-agent').value") == "export"
    ui.click("#onboarding-next")
    ui.click("#btn-projects")
    ui.idle()
    ui.value("#new-project-name", "Synthetic desktop qualification")
    ui.click("#new-project-form button[type=submit]")
    ui.stage("source")
    passed("isolated onboarding and Project creation via controls")

    # An unsuccessful project import must not capture the next one-off export.
    ui.click("#btn-project-add-source")
    ui.wait("document.body.dataset.screen==='start' && !document.querySelector('#start-error').hidden")
    ui.wait("document.querySelector('#screen-projects').getAttribute('aria-busy')==='false'")
    assert ui.evaluate("document.querySelector('#project-attach-options').hidden"), "Failed import left project connection active"
    ui.click("#dropzone")
    ui.wait("document.body.dataset.screen==='select'")
    assert ui.evaluate("document.querySelector('#list [tabindex=\"0\"]')!==null")
    ui.evaluate("(() => { const search=document.querySelector('#search'); search.focus(); search.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowDown', bubbles:true})); return true })()")
    ui.evaluate("document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowDown', bubbles:true}))")
    assert ui.evaluate("document.activeElement.dataset.key==='c:222' && document.activeElement.tabIndex===0"), "Arrow navigation lost the list tab stop"
    ui.evaluate("(() => { const search=document.querySelector('#search'); search.focus(); search.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowDown', bubbles:true})); return true })()")
    assert ui.evaluate("document.activeElement.dataset.key==='c:111' && document.activeElement.tabIndex===0"), "Search navigation lost the list tab stop"
    ui.click('[data-key="c:111"]')
    assert ui.evaluate("document.querySelector('[data-key=\"c:111\"]').getAttribute('aria-selected')==='true'")
    ui.click("#btn-next")
    ui.wait("document.body.dataset.screen==='save'")
    assert ui.invoke("list_projects")[0]["project"]["sources"] == []
    ui.click("#btn-projects")
    ui.idle()
    initial = ui.invoke("list_projects")[0]["project"]
    ui.click('[data-project="' + initial["project_id"] + '"]')
    ui.stage("source")
    passed("failed project import resets connection; next one-off export leaves project unchanged")

    assert "прочитает выбранный JSON" in ui.evaluate("document.querySelector('#source-import-boundary').textContent")
    ui.click("#btn-project-add-source")
    ui.wait("document.body.dataset.screen==='select'")
    ui.click('[data-key="c:111"]')
    ui.click('[data-chev="222"]')
    ui.click('[data-key="t:222:100"]')
    account_label = 'synthetic <img src=x onerror="window.__scopeExecuted=true">'
    ui.value("#project-account-label", account_label)
    ui.click("#btn-next")
    ui.stage("source")
    project = ui.invoke("list_projects")[0]["project"]
    pid = project["project_id"]
    assert len(project["sources"]) == 2
    forum = next(s for s in project["sources"] if s["scope"]["conversation_id"] == "222")
    assert forum["selection"]["filter"]["topic_ids"] == ["100"]
    card = '[data-source="' + forum["source_id"] + '"]'
    assert ui.evaluate("document.querySelectorAll('.source-access[data-access=local_archive]').length") == 2
    ui.click(card + " .source-access-details summary")
    access = ui.evaluate("document.querySelector(" + json.dumps(card + " .source-access") + ").innerText")
    assert account_label in access and "ID 222" in access
    for fact in ["JSON целиком", "локальная копия чата хранит исходные", "не получает сессию",
                 "человек подтверждает", "Автовыгрузка не включена", "Полнота истории не подтверждена",
                 "Автоматического скачивания нет"]:
        assert fact in access, fact
    assert ui.evaluate("document.querySelector('.source-access img')===null && !window.__scopeExecuted")
    assert "Telegram Desktop" in ui.evaluate("document.querySelector(" + json.dumps(card + " .assisted-export summary") + ").textContent")
    ui.screenshot(report_dir / "source-access.png")
    ui.click(card + " .source-access-details summary")
    passed("source access distinguishes whole-file read, stored chat, context scope and manual refresh")
    direct_source = next(s for s in project["sources"] if s["scope"]["conversation_id"] == "111")
    draft_card = '[data-source="' + direct_source["source_id"] + '"]'
    ui.value(draft_card + " [name=from]", "2026-06-18")
    ui.value(card + " [name=from]", "2026-06-20")
    ui.value(card + " [name=through]", "2026-06-20")
    assert ui.evaluate("document.querySelector('#btn-project-review').disabled")
    ui.click(card + " button[type=submit]")
    ui.stage("source")
    assert ui.evaluate("document.querySelector(" + json.dumps(draft_card + " [name=from]") + ").value") == "2026-06-18", "Saving another chat discarded draft dates"
    assert ui.evaluate("!document.querySelector('#project-unsaved').hidden && document.querySelector('#btn-project-review').disabled")
    saved = ui.invoke("open_project", {"projectId": pid})
    assert next(s for s in saved["sources"] if s["source_id"] == direct_source["source_id"])["selection"]["filter"]["dates"] is None
    ui.click(draft_card + " button[type=submit]")
    ui.stage("source")
    assert ui.evaluate("document.querySelector('#project-unsaved').hidden")
    passed("saving one chat preserves other drafts and blocks preparation until all are saved")
    ui.value("#privacy-preset", "people")
    ui.click("#btn-project-review")
    ui.stage("privacy")
    preview = ui.evaluate("document.querySelector('#project-review-preview').textContent")
    assert "SYNTHETIC_E2E_PASSWORD" not in preview
    assert "Alice" not in preview and "Bob" not in preview
    assert "fixing it" in preview and "found a bug" not in preview and "general hello" not in preview
    assert "сообщений: 3" in ui.evaluate("document.querySelector('#project-review-summary').textContent")
    passed("full JSON, chat/topic/date scope, saved privacy and sanitized Review")

    ui.click("#btn-project-destination")
    ui.stage("analyze")
    ui.value("#analysis-agent", "export")
    assert "Файлы сохранятся в выбранную папку" in ui.evaluate("document.querySelector('#analysis-status').textContent")
    ui.click("#btn-project-export")
    ui.stage("result")
    assert "Файлы сохранены" in ui.evaluate("document.querySelector('#project-export-result').textContent")
    outputs = list((root / "bundle-output").rglob("*.md"))
    assert outputs, "No actual bundle Markdown written"
    assert all("SYNTHETIC_E2E_PASSWORD" not in p.read_text(encoding="utf-8") for p in outputs)
    passed("Export only writes sanitized bundle through real IPC")

    for agent in ["codex", "claude"]:
        ui.click('#project-steps [data-go=source]')
        ui.click("#btn-project-review")
        ui.stage("privacy")
        ui.click("#btn-project-destination")
        ui.value("#analysis-agent", agent)
        ui.value("#analysis-model", "fixture-success")
        ui.click("#btn-analysis-prepare")
        ui.stage("review")
        assert "synthetic-" + agent in ui.evaluate("document.querySelector('#analysis-review-summary').textContent")
        assert "локальный тестовый стенд" in ui.evaluate("document.querySelector('#analysis-review-destination').textContent")
        ui.click("#btn-analysis-run")
        ui.stage("result")
        assert "Готово" in ui.evaluate("document.querySelector('#analysis-result-status').textContent")
        assert ui.evaluate("document.querySelector('#analysis-result-body').textContent.trim().length > 0")
        assert ui.evaluate("document.querySelector('#analysis-result-body img')===null")
        ui.screenshot(report_dir / (agent + "-result.png"))
        passed(agent + " synthetic agent through Review, explicit Run and rendered result")

    ui.click('#project-steps [data-go=source]')
    ui.click(card + " [data-relink]")
    ui.stage("source")
    project = ui.invoke("open_project", {"projectId": pid})
    forum = next(s for s in project["sources"] if s["scope"]["conversation_id"] == "222")
    # Rust canonicalization uses the extended-length prefix on Windows.
    assert os.path.samefile(forum["archive_path"], root / "single.json")
    assert forum["selection"]["filter"]["topic_ids"] == ["100"]
    assert ui.evaluate("document.querySelector('#project-steps [data-go=review]').disabled")
    ui.click("#btn-projects")
    ui.idle()
    ui.click('[data-project="' + pid + '"]')
    ui.stage("source")
    assert ui.evaluate("document.querySelector(" + json.dumps(card + " [name=from]") + ").value") == "2026-06-20"
    passed("single JSON refresh preserves saved scope, invalidates Review and reopens Project")

    # The selected-folder watcher surfaces a candidate, never a completion.
    ui.command("execute/async", {"script": """
        const done = arguments[arguments.length - 1];
        window.__exportCandidateEvents = [];
        window.__TAURI__.event.listen('assisted-export-candidate', e => __exportCandidateEvents.push(e.payload))
          .then(() => done(true));
    """, "args": []})
    export_dir = root / "telegram-exports"
    nested = export_dir / "ChatExport_synthetic"
    nested.mkdir(parents=True)
    exported = json.loads((root / "single.json").read_text(encoding="utf-8"))
    exported["messages"][0]["text"] = "Changed in repeated export"
    exported["messages"].append({"id": 103, "type": "message", "date": "2026-06-20T12:00:00",
                                 "from": "Synthetic", "from_id": "user-synthetic", "text": "Added in repeated export"})
    new_archive = nested / "result.json"
    new_archive.write_text(json.dumps(exported), encoding="utf-8")
    project = ui.invoke("update_project", {"projectId": pid, "expectedRevision": project["revision"],
        "change": {"kind": "assisted_export", "value": {"source_id": forum["source_id"],
            "settings": {"directory": str(export_dir), "client": None}}}})
    old_snapshot = next(s for s in project["sources"] if s["source_id"] == forum["source_id"])["latest_snapshot_id"]
    old_analysis = project["analysis_run"]
    ui.click("#btn-projects")
    ui.idle()
    ui.wait("window.__exportCandidateEvents.some(e=>e.project_id===" + json.dumps(pid) + ")", timeout=16)
    assert ui.invoke("open_project", {"projectId": pid})["revision"] == project["revision"], "Background observation cannot publish"
    ui.click('[data-project="' + pid + '"]')
    ui.stage("source")
    ui.click(card + " .assisted-export summary")
    ui.wait("document.querySelector(" + json.dumps(card + " [data-assisted-candidate-list] button") + ") !== null", timeout=12)
    assert ui.evaluate("document.querySelector(" + json.dumps(card + " [data-assisted-archive]") + ").value") == ""
    assert not ui.evaluate("document.querySelector(" + json.dumps(card + " [data-assisted-confirm]") + ").checked")
    ui.screenshot(report_dir / "telegram-watcher-candidate.png")
    observed = ui.invoke("open_project", {"projectId": pid})
    assert next(s for s in observed["sources"] if s["source_id"] == forum["source_id"])["latest_snapshot_id"] == old_snapshot
    ui.click(card + " [data-assisted-candidate-list] button")
    assert os.path.samefile(ui.evaluate("document.querySelector(" + json.dumps(card + " [data-assisted-archive]") + ").value"), new_archive)
    assert ui.evaluate("document.querySelector(" + json.dumps(card + " [data-assisted-import]") + ").disabled")
    ui.click(card + " [data-assisted-confirm]")
    ui.click(card + " [data-assisted-import]")
    ui.stage("source")
    project = ui.invoke("open_project", {"projectId": pid})
    forum = next(s for s in project["sources"] if s["source_id"] == forum["source_id"])
    assert forum["latest_snapshot_id"] != old_snapshot
    assert os.path.samefile(forum["archive_path"], new_archive)
    assert project["analysis_run"] == old_analysis, "Watcher/import must not run the agent"
    passed("selected-folder candidate requires confirmation; stable repeated Telegram import updates snapshot without Run")

    # Two connected chats may receive files in separate watched folders. A
    # candidate for one source is never permission to import the other chat.
    direct = next(s for s in project["sources"] if s["scope"]["conversation_id"] == "111")
    direct_card = '[data-source="' + direct["source_id"] + '"]'
    direct_snapshot = direct["latest_snapshot_id"]
    forum_snapshot = forum["latest_snapshot_id"]
    direct_dir = root / "direct-exports"
    direct_dir.mkdir()
    direct_archive = direct_dir / "result.json"
    direct_archive.write_bytes(new_archive.read_bytes())  # wrong native chat ID 222
    project = ui.invoke("update_project", {"projectId": pid, "expectedRevision": project["revision"],
        "change": {"kind": "assisted_export", "value": {"source_id": direct["source_id"],
            "settings": {"directory": str(direct_dir), "client": None}}}})
    ui.click("#btn-projects")
    ui.idle()
    ui.click('[data-project="' + pid + '"]')
    ui.stage("source")
    ui.click(direct_card + " .assisted-export summary")
    ui.wait("document.querySelector(" + json.dumps(direct_card + " [data-assisted-candidate-list] button") + ") !== null", timeout=12)
    ui.click(direct_card + " [data-assisted-candidate-list] button")
    ui.click(direct_card + " [data-assisted-confirm]")
    ui.click(direct_card + " [data-assisted-import]")
    ui.stage("source")
    assert "Импорт не завершён" in ui.evaluate("document.querySelector(" + json.dumps(direct_card + " [data-assisted-status]") + ").textContent")
    rejected = ui.invoke("open_project", {"projectId": pid})
    assert rejected["revision"] == project["revision"]
    assert next(s for s in rejected["sources"] if s["source_id"] == direct["source_id"])["latest_snapshot_id"] == direct_snapshot
    assert next(s for s in rejected["sources"] if s["source_id"] == forum["source_id"])["latest_snapshot_id"] == forum_snapshot
    assert not ui.evaluate("document.querySelector(" + json.dumps(direct_card + " [data-assisted-confirm]") + ").checked")

    # Losing a selected file and receiving a truncated JSON are separate
    # failures. Neither is a completed client export or a reason to advance
    # either connected source; the person must confirm each retry again.
    for failure in ["missing", "truncated"]:
        if failure == "missing":
            direct_archive.unlink()
        else:
            direct_archive.write_text('{"id":111,"messages":[', encoding="utf-8")
        ui.click(direct_card + " [data-assisted-confirm]")
        ui.click(direct_card + " [data-assisted-import]")
        ui.stage("source")
        assert "Предыдущая копия сообщений сохранена" in ui.evaluate("document.querySelector(" + json.dumps(direct_card + " [data-assisted-status]") + ").textContent")
        assert not ui.evaluate("document.querySelector(" + json.dumps(direct_card + " [data-assisted-confirm]") + ").checked")
        failed = ui.invoke("open_project", {"projectId": pid})
        assert failed["revision"] == project["revision"], failure
        assert next(s for s in failed["sources"] if s["source_id"] == direct["source_id"])["latest_snapshot_id"] == direct_snapshot
        assert next(s for s in failed["sources"] if s["source_id"] == forum["source_id"])["latest_snapshot_id"] == forum_snapshot
        assert failed["analysis_run"] == old_analysis
    passed("missing and truncated assisted JSON require a new confirmation and preserve both snapshots")

    correct = json.loads((root / "full.json").read_text(encoding="utf-8"))["chats"]["list"][0]
    correct["messages"][1]["text"] = "Changed in direct chat"
    correct["messages"].pop(0)  # absent in this snapshot, not a deletion claim
    correct["messages"].append({"id": 4, "type": "message", "date": "2026-06-20T13:00:00",
                                 "from": "Synthetic", "from_id": "user-synthetic", "text": "Added to direct chat"})
    direct_archive.write_text(json.dumps(correct), encoding="utf-8")
    ui.click(direct_card + " [data-assisted-confirm]")
    ui.click(direct_card + " [data-assisted-import]")
    ui.stage("source")
    assert "+1 новых · 1 изменённых · 1 отсутствуют" in ui.evaluate("document.querySelector('#toast').textContent")
    accepted = ui.invoke("open_project", {"projectId": pid})
    assert next(s for s in accepted["sources"] if s["source_id"] == direct["source_id"])["latest_snapshot_id"] != direct_snapshot
    assert next(s for s in accepted["sources"] if s["source_id"] == forum["source_id"])["latest_snapshot_id"] == forum_snapshot
    assert accepted["analysis_run"] == old_analysis
    passed("two assisted Telegram sources reject wrong chat then retry one source without analysis")

    refresh = direct_card + ' [data-telegram-refresh]'
    def refresh_control(mode, now):
        temporary = root / "refresh-control.tmp"
        temporary.write_text(json.dumps({"mode": mode, "now": now}), encoding="utf-8")
        temporary.replace(root / "refresh-control.json")

    ui.click(refresh + ' summary')
    ui.wait('document.querySelector(' + json.dumps(refresh + ' [data-refresh-cadence]') + ')?.disabled === false')
    ui.value(refresh + ' [data-refresh-cadence]', 'on_start')
    ui.click(refresh + ' [data-refresh-save]')
    ui.idle()
    ui.wait("document.querySelector(" + json.dumps(refresh + ' [data-refresh-status]') + ")?.textContent.includes('разблокированная')")
    assert not (root / "refresh-starts.txt").exists()
    passed("schedule opt-in persists and a locked session never starts the synthetic client")

    refresh_control("success", 100)
    ui.wait('document.querySelector(' + json.dumps(refresh) + ')?.dataset.state === "ready"')
    ui.wait('document.querySelector(' + json.dumps(refresh + ' [data-refresh-now]') + ')?.disabled === false')
    refreshed = ui.invoke("open_project", {"projectId": pid})
    refreshed_direct = next(s for s in refreshed["sources"] if s["source_id"] == direct["source_id"])
    assert Path(refreshed_direct["archive_path"]).parent.name.startswith('refresh-')
    assert refreshed["analysis_run"] == accepted["analysis_run"]
    assert next(s for s in refreshed["sources"] if s["source_id"] == forum["source_id"])["latest_snapshot_id"] == forum_snapshot
    assert len((root / "refresh-starts.txt").read_text().splitlines()) == 1
    ui.screenshot(report_dir / "scheduled-refresh.png")
    passed("unlocked timer imports the correlated output path once and preserves the other chat and analysis")

    refresh_control("hold", 10000)
    ui.click(refresh + ' [data-refresh-now]')
    ui.wait('document.querySelector(' + json.dumps(refresh) + ')?.dataset.state === "exporting"')
    ui.click(refresh + ' [data-refresh-cancel]')
    ui.wait('document.querySelector(' + json.dumps(refresh) + ')?.dataset.state === "cancelled"')
    cancelled_project = ui.invoke("open_project", {"projectId": pid})
    assert cancelled_project["sources"] == refreshed["sources"]
    ui.click(refresh + ' [data-refresh-reload]')
    ui.idle()
    passed("refresh cancellation waits for the fake client terminal event and preserves snapshots")

    refresh_control("hold", 20000)
    ui.wait('document.querySelector(' + json.dumps(refresh + ' [data-refresh-now]') + ')?.disabled === false')
    ui.click(refresh + ' [data-refresh-now]')
    ui.wait('document.querySelector(' + json.dumps(refresh) + ')?.dataset.state === "exporting"')
    refresh_control("locked", 20000)
    ui.wait('document.querySelector(' + json.dumps(refresh) + ')?.dataset.state === "needs_user_action"')
    assert ui.evaluate('document.querySelector(' + json.dumps(refresh + ' [data-refresh-resolve]') + ').disabled')
    ui.click(refresh + ' [data-refresh-confirm]')
    ui.click(refresh + ' [data-refresh-resolve]')
    ui.idle()
    ui.wait('document.querySelector(' + json.dumps(refresh) + ')?.dataset.state === "idle"')
    recovered_project = ui.invoke("open_project", {"projectId": pid})
    assert recovered_project["sources"] == refreshed["sources"]
    assert recovered_project["telegram_refresh"][direct["source_id"]]["checkpoint"]["unresolved_attempt"] is False
    ui.value(refresh + ' [data-refresh-cadence]', 'manual')
    ui.click(refresh + ' [data-refresh-save]')
    ui.idle()
    assert len((root / "refresh-starts.txt").read_text().splitlines()) == 3
    passed("lost active session requires explicit review and manual cadence can stop future scheduling")

    if platform.system() == "Linux":
        inbox = root / "local-package-input"
        output = root / "local-package-output"
        inbox.mkdir()
        output.mkdir()
        archive = {"id": 333, "messages": [{"id": 1, "text": "Hello 🌍 password=SYNTHETIC_PACKAGE_SECRET"}]}
        (inbox / "result.json").write_text(json.dumps(archive), encoding="utf-8")
        package_project = ui.invoke("create_project", {"name": "Local package fixture"})
        package_id = package_project["project_id"]
        ui.invoke("update_project", {"projectId": package_id, "expectedRevision": package_project["revision"],
            "change": {"kind": "source", "value": {"source_id": "local-chat", "connector_id": "telegram_json",
                "scope": {"platform": "telegram", "account_local_id": "synthetic", "conversation_id": "333"},
                "archive_path": str(inbox / "result.json"), "latest_snapshot_id": None}}})
        ui.click("#btn-projects")
        ui.idle()
        ui.click('[data-project="' + package_id + '"]')
        ui.stage("source")
        package = '[data-local-package="project"]'
        ui.click(package + ' summary')
        ui.value(package + ' [data-package-input]', str(inbox))
        ui.value(package + ' [data-package-output]', str(output))
        ui.click(package + ' [data-package-save]')
        ui.idle()
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "paused"')
        ui.click(package + ' [data-package-refresh]')
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "ready"')
        first = ui.invoke("local_package_status", {"projectId": package_id})
        ready = Path(first["ready"]["directory"])
        assert "SYNTHETIC_PACKAGE_SECRET" not in (ready / "context-00001.md").read_text()
        assert "Hello 🌍" in (ready / "context-00001.md").read_text()
        (inbox / "result.json").write_text('{"messages":[', encoding="utf-8")
        ui.click(package + ' [data-package-refresh]')
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "error"')
        assert ui.invoke("local_package_status", {"projectId": package_id})["ready"] == first["ready"]
        ui.screenshot(report_dir / "local-package-error.png")
        archive["messages"].append({"id": 2, "text": "Updated local export"})
        (inbox / "result.json").write_text(json.dumps(archive), encoding="utf-8")
        ui.click(package + ' [data-package-auto]')
        ui.click(package + ' [data-package-save]')
        ui.idle()
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "ready"', timeout=55)
        updated = ui.invoke("local_package_status", {"projectId": package_id})
        assert updated["ready"]["messages"] == 2
        assert not (ready.parent / first["ready"]["generation"]).exists()
        assert updated["settings"]["automatic"] is True
        ui.click(package + ' [data-package-stop]')
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "paused"')
        stopped = ui.invoke("local_package_status", {"projectId": package_id})
        assert stopped["settings"]["automatic"] is False
        assert stopped["ready"] == updated["ready"]
        ui.screenshot(report_dir / "local-package-ready-paused.png")
        passed("local package controls: privacy, invalid input recovery, automatic local refresh, cleanup and durable pause")

    if platform.system() == "Linux":
        shared_project = ui.invoke("create_project", {"name": "Multiple selected chats"})
        shared_id = shared_project["project_id"]
        ui.click("#btn-projects")
        ui.idle()
        ui.click('[data-project="' + shared_id + '"]')
        ui.stage("source")
        ui.click("#btn-project-add-source")
        ui.wait("document.body.dataset.screen==='select'")
        ui.click('[data-key="c:111"]')
        ui.click('[data-chev="222"]')
        ui.click('[data-key="t:222:100"]')
        ui.value("#project-account-label", "synthetic")
        ui.click("#btn-next")
        ui.stage("source")
        ui.wait("document.querySelectorAll('[data-local-package]').length===1")
        package = '[data-local-package="project"]'
        ui.click(package + ' summary')
        assert "Выбрано чатов: 2" in ui.evaluate('document.querySelector(' + json.dumps(package + ' [data-package-selection]') + ').textContent')
        inbox = root / "shared-input"
        exported = inbox / "DataExport_synthetic"
        output = root / "shared-output"
        exported.mkdir(parents=True)
        output.mkdir()
        data = json.loads((root / "full.json").read_text())
        data["chats"]["list"].append({"id": 444, "messages": [{"id": 1, "text": "unselected entire chat", "photo": "absent.png"}]})
        path = exported / "result.json"
        path.write_text(json.dumps(data), encoding="utf-8")
        ui.value(package + ' [data-package-input]', str(inbox))
        ui.value(package + ' [data-package-output]', str(output))
        ui.click(package + ' [data-package-save]')
        ui.idle()
        ui.click(package + ' [data-package-refresh]')
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "ready"')
        first = ui.invoke("local_package_status", {"projectId": shared_id})
        assert first["ready"]["conversations"] == 2 and first["ready"]["messages"] == 4
        assert len(first["settings"]["source_ids"]) == 2
        ready = Path(first["ready"]["directory"])
        text = "".join(p.read_text() for p in ready.glob("context-*.md"))
        assert "found a bug" in text and "fixing it" in text
        assert "general hello" not in text and "unselected entire chat" not in text
        assert "SYNTHETIC_E2E_PASSWORD" not in text
        before = ui.invoke("open_project", {"projectId": shared_id})
        path.write_text(json.dumps(data["chats"]["list"][0]), encoding="utf-8")
        ui.click(package + ' [data-package-refresh]')
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "error"')
        assert ui.invoke("open_project", {"projectId": shared_id})["sources"] == before["sources"]
        assert ui.invoke("local_package_status", {"projectId": shared_id})["ready"] == first["ready"]
        data["chats"]["list"][0]["messages"].append({"id": 4, "text": "Updated direct conversation"})
        data["chats"]["list"][1]["messages"].append({"id": 103, "reply_to_message_id": 102, "text": "Updated chosen topic"})
        path.write_text(json.dumps(data), encoding="utf-8")
        ui.click(package + ' [data-package-auto]')
        ui.click(package + ' [data-package-save]')
        ui.idle()
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "ready"', timeout=55)
        updated = ui.invoke("local_package_status", {"projectId": shared_id})
        assert updated["ready"]["messages"] == 6
        assert updated["ready"]["conversations"] == 2
        assert not (ready.parent / first["ready"]["generation"]).exists()
        ui.click(package + ' [data-package-stop]')
        ui.wait('document.querySelector(' + json.dumps(package) + ')?.dataset.phase === "paused"')
        ui.screenshot(report_dir / "shared-package-ready.png")
        passed("one common export: UI selects multiple chats/topics, one package control, missing-chat rollback and automatic combined update")

    # A persisted future connector is metadata only, not an OAuth authorization.
    # Set up that unavailable source via real IPC, then exercise its rendered UI.
    future = ui.invoke("create_project", {"name": "Unimplemented connector fixture"})
    ui.invoke("update_project", {"projectId": future["project_id"], "expectedRevision": future["revision"],
        "change": {"kind": "source", "value": {"source_id": "future", "connector_id": "teams_graph",
            "scope": {"platform": "teams", "account_local_id": "synthetic", "conversation_id": "111"},
            "archive_path": None, "latest_snapshot_id": None}}})
    ui.click("#btn-projects")
    ui.idle()
    ui.click('[data-project="' + future["project_id"] + '"]')
    ui.stage("source")
    assert "ещё не настроено чтение данных" in ui.evaluate("document.querySelector('.source-access[data-access=unverified]').textContent")
    assert ui.evaluate("document.querySelector('[data-refresh]').disabled && document.querySelector('[data-relink]').disabled")
    assert ui.evaluate("document.querySelector('.assisted-export')===null")
    assert "teams" in ui.evaluate("document.querySelector('.project-source').textContent")
    ui.screenshot(report_dir / "unverified-source.png")
    ui.click('#project-steps [data-go=result]')
    ui.stage("result")
    assert ui.evaluate("document.querySelector('#project-export-result').hidden && document.querySelector('#btn-project-open-export').hidden"), "Another project showed previous project's export"
    passed("unimplemented source has no inferred grant; another project cannot show stale output")

    ui.click("#btn-projects-back")
    ui.click("#dropzone")
    ui.wait("document.body.dataset.screen==='select'")
    assert ui.evaluate("document.querySelectorAll('#list [data-key^=\"c:\"]').length") == 1
    ui.click('[data-chev="222"]')
    ui.click('[data-key="t:222:100"]')
    ui.click("#btn-next")
    ui.wait("document.body.dataset.screen==='save'")
    ui.click("#btn-pick-dir")
    ui.wait("document.querySelector('#out-dir').title.includes('single-output')")
    ui.click("#btn-export")
    ui.wait("document.body.dataset.screen==='done'")
    outputs = list((root / "single-output").glob("*.md"))
    assert len(outputs) == 1
    text = outputs[0].read_text(encoding="utf-8")
    assert "found a bug" in text and "fixing it" in text and "general hello" not in text
    ui.screenshot(report_dir / "saved-files.png")
    passed("single-chat topic selection and actual one-off Markdown output")

    ui.click("#btn-new-file")
    ui.wait("document.body.dataset.screen==='start' && !document.querySelector('#start-error').hidden")
    assert ui.evaluate("document.querySelector('#start-error').textContent.length > 0")
    assert ui.evaluate("window.__e2eErrors") == []
    passed("malformed archive displays an error; no uncaught renderer failures")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-run", action="store_true", help="launch with cargo run -p tgsum")
    parser.add_argument("--report-dir", type=Path, default=REPO / "desktop-e2e-report")
    args = parser.parse_args()
    report_dir = args.report_dir.resolve()
    report_dir.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="tgsum-desktop-e2e-")).resolve()
    print("Owned synthetic root: " + str(root), flush=True)
    nonce = uuid.uuid4().hex
    with socket.socket() as available:
        available.bind(("127.0.0.1", 0))
        port = available.getsockname()[1]
    prepare(root, nonce, port)
    binary = REPO / "target/debug" / ("tgsum.exe" if os.name == "nt" else "tgsum")
    command = ["cargo", "run", "--locked", "-p", "tgsum", "--features", "desktop-e2e"] if args.cargo_run else [str(binary)]
    env = dict(os.environ, TGSUM_DESKTOP_E2E_ROOT=str(root), TGSUM_ANALYSIS_FIXTURE="1",
               XDG_DATA_HOME=str(root / "data"), XDG_CONFIG_HOME=str(root / "config"),
               XDG_CACHE_HOME=str(root / "cache"), GTK_CSD="1")
    # No inspector endpoint is needed; WebDriver belongs to this child only.
    env.pop("WEBKIT_INSPECTOR_HTTP_SERVER", None)
    report = {"status": "failed", "checks": [], "platform": platform.platform(),
              "architecture": platform.machine(), "python": platform.python_version(),
              "runner_image": os.environ.get("ImageVersion"), "root": str(root),
              "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
              "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO, text=True).strip()),
              "rust": subprocess.check_output(["rustc", "--version", "--verbose"], cwd=REPO, text=True).strip(),
              "picker": "simulated native response", "agents": "synthetic",
              "telegram_refresh": "synthetic driver, clock and session; no messenger process",
              "driver": "tauri-plugin-wdio-webdriver=1.4.0"}
    ui = WebView(port)
    child = None
    try:
        with (report_dir / "app.log").open("w", encoding="utf-8") as log:
            child = subprocess.Popen(command, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 120
            while True:
                assert child.poll() is None, "Application exited; see app.log"
                try:
                    if (root / "webview-ready").exists() and ui.request("GET", "/status")["ready"]:
                        break
                except (urllib.error.URLError, ConnectionError):
                    pass
                assert time.monotonic() < deadline, "WebDriver startup timeout"
                time.sleep(0.25)
            ui.session = ui.request("POST", "/session", {"capabilities": {"alwaysMatch": {}}})["sessionId"]
            ui.command("timeouts", {"script": 30000, "implicit": 0, "pageLoad": 30000})
            info = ui.wait("window.__TGSUM_E2E__")
            assert info["nonce"] == nonce, "Refusing a WebDriver from another process"
            report["webview"] = info
            report["user_agent"] = ui.evaluate("navigator.userAgent")
            exercise(ui, root, report, report_dir)
            ui.screenshot(report_dir / "malformed-archive.png")
            report["status"] = "passed"
    except Exception:
        report["failure"] = traceback.format_exc()
        if ui.session:
            try:
                ui.screenshot(report_dir / "failure.png")
            except Exception as error:
                report["screenshot_error"] = str(error)
        raise
    finally:
        if ui.session:
            try:
                ui.request("DELETE", f"/session/{ui.session}")
            except Exception:
                pass
        if child is not None and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=10)
        (report_dir / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print("Report: " + str(report_dir / "report.json"), flush=True)


if __name__ == "__main__":
    main()
