import copy
import contextlib
import datetime as dt
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import pipeline as p


def row(letter="a", text="Сделать проверку", revision="b"):
    return {"schema_version": 1, "source": "source_" + "d" * 64,
        "evidence": {"id": "e_" + letter * 64, "revision": "r_" + revision * 64},
        "text": text, "topic": None, "topic_known": False, "sender": "Person 1"}


def result(record):
    ref = dict(record["evidence"], quote=record["text"])
    task = {"anchor": ref["id"], "slot": 0, "title": "Проверить", "details": "Описание",
        "horizon": "week", "owner_suggestion": "", "deadline_suggestion": "", "evidence": [ref]}
    reports = {name: {"text": "", "evidence": []} for name in ["summary", "call", "feedback", "retro"]}
    reports["summary"] = {"text": "Нужна проверка", "evidence": [ref]}
    return {"tasks": [task], "reports": reports}


def task(record=None):
    record = record or row()
    return p.validate_result(result(record), [record], [])["tasks"][0]


class FakeNotion:
    def __init__(self, token=None):
        self.calls = []
        self.pages = []
        self.fail_create = False
    def lookup(self, source, key):
        self.calls.append(("lookup", source, key))
        return self.pages
    def request(self, method, path, body=None, create=False):
        self.calls.append((method, path, copy.deepcopy(body)))
        if path.startswith("data_sources/") and method == "GET":
            props = {"Название": "title", "Статус": "select", "Плановая дата": "date", "Ответственный": "people",
                "Дедлайн": "date", "Проект": "rich_text", "Основание": "rich_text", "Источник": "url", "Ключ синхронизации": "rich_text"}
            return {"properties": {k: {"type": v} for k,v in props.items()}}
        if path == "pages" and method == "POST":
            if self.fail_create:
                raise p.Stop("fixture lost response")
            return {"id": "fixture-page"}
        if path.endswith("/children") and method == "PATCH":
            return {"results": [{"id": "fixture-block"}]}
        return {"results": [], "has_more": False}


CONFIG = {"enabled": True, "automatic": False, "package": "project-FIXTURE", "project_title": "Fixture",
    "notion_data_source": "fixture-source", "notion_views": {}, "source_url": "https://github.com/fixture/private",
    "min_interval_seconds": 0, "oauth_account_sha256": "fixture-account"}


class Contracts(unittest.TestCase):
    def test_github_auth_error_reports_operation_and_status_without_stderr(self):
        failure = Mock(returncode=1, stdout=b"fixture-private-output", stderr=b"gh: fixture-secret private-id (HTTP 403)")
        success = Mock(returncode=0, stdout=b'{"created_at":"2026-10-08T12:00:00Z"}')
        for commands, label in [([failure], "сведения запуска"), ([success, failure], "метаданные OAuth-секрета")]:
            with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {"CODEX_AUTH_WRITER_TOKEN": "fixture", "GITHUB_TOKEN": "fixture", "GITHUB_RUN_ID": "1"}):
                oauth = p.OAuth("fixture/private", Path(root) / "session", "fixture")
                with patch.object(p.subprocess, "run", side_effect=commands), self.assertRaises(p.Stop) as error:
                    oauth.restore()
            message = str(error.exception)
            self.assertIn(label, message)
            self.assertIn("HTTP 403", message)
            for private in ["fixture-secret", "private-id", "fixture-private-output", "repos/fixture/private"]:
                self.assertNotIn(private, message)

    def test_notion_refusal_identifies_status_without_private_response_or_id(self):
        notion = p.Notion("fixture-secret")
        for status in [400, 401, 404]:
            error = p.urllib.error.HTTPError("https://api.notion.com/v1/data_sources/private-source-id", status,
                "fixture-private-error", {}, io.BytesIO(b"fixture-private-body"))
            with patch.object(notion.http, "open", side_effect=error):
                with self.assertRaises(p.Stop) as failure:
                    notion.request("GET", "data_sources/private-source-id")
            message = str(failure.exception)
            self.assertIn(f"HTTP {status}", message)
            self.assertIn("data_sources", message)
            for private in ["private-source-id", "fixture-private-body", "fixture-private-error", "fixture-secret"]:
                self.assertNotIn(private, message)

    def test_cli_notice_is_pinned_preturn_and_tools_remain_forbidden(self):
        before = [{"type": "thread.started", "thread_id": "fixture"},
            {"type": "item.completed", "item": {"id": "item_0", "type": "error", "message": p.CODE_MODE_DISABLED}}]
        turn = [{"type": "turn.started"}, {"type": "item.completed", "item": {"id": "item_1", "type": "agent_message", "text": "{}"}},
            {"type": "turn.completed", "usage": {}}]
        stream = lambda values: b"\n".join(map(p.encoded, values))
        self.assertEqual(p.check_events(stream(before + turn)), "{}")
        for bad in [before + before[1:] + turn, before + turn[:1] + before[1:] + turn[1:],
                    before + turn[:-1], before + turn + [{"type": "turn.started"}],
                    before + turn[:1] + [{"type": "item.completed", "item": {"id": "item_2", "type": "command_execution", "command": "fixture"}}] + turn[1:]]:
            with self.assertRaises(p.Stop): p.check_events(stream(bad))

    def test_stable_key_changes_revision_without_title_identity(self):
        old = task()
        changed = task(row(text="Изменить проверку", revision="c"))
        self.assertEqual(old["key"], changed["key"])
        self.assertNotEqual(old["evidence"], changed["evidence"])

    def test_reports_and_tasks_reject_fabricated_citations_and_owners(self):
        record = row()
        good = result(record)
        for field in ["tasks", "reports"]:
            bad = copy.deepcopy(good)
            ref = bad["tasks"][0]["evidence"][0] if field == "tasks" else bad["reports"]["summary"]["evidence"][0]
            ref["revision"] = "r_" + "f" * 64
            with self.assertRaises(p.Stop): p.validate_result(bad, [record], [])
        bad = copy.deepcopy(good)
        bad["tasks"][0]["owner_suggestion"] = "Invented owner"
        with self.assertRaises(p.Stop): p.validate_result(bad, [record], [])
        bad = copy.deepcopy(good)
        bad["tasks"][0]["evidence"][0]["quote"] = "invented"
        with self.assertRaises(p.Stop): p.validate_result(bad, [record], [])

    def test_rows_reject_duplicate_ids_and_unknown_topic_guess(self):
        record = row()
        with self.assertRaises(p.Stop): p.rows(p.encoded(record) + b"\n" + p.encoded(record))
        record["topic_known"] = True
        with self.assertRaises(p.Stop): p.rows(p.encoded(record))

    def test_create_intent_durable_before_post_and_no_blind_retry(self):
        value = task()
        state = {"tasks": {value["key"]: value}, "active_tasks": [value["key"]], "notion": {}}
        notion = FakeNotion()
        notion.fail_create = True
        persisted = []
        with self.assertRaises(p.Stop):
            p.sync_tasks(notion, state, CONFIG, lambda s: persisted.append(copy.deepcopy(s)))
        self.assertEqual(persisted[0]["notion"][value["key"]]["intent"], "create_started")
        before = len([call for call in notion.calls if call[:2] == ("POST", "pages")])
        with self.assertRaises(p.Stop): p.sync_tasks(notion, state, CONFIG, lambda s: None)
        self.assertEqual(before, len([call for call in notion.calls if call[:2] == ("POST", "pages")]))
        notion.pages = [{"id": "reconciled-page"}]
        p.sync_tasks(notion, state, CONFIG, lambda s: None)
        self.assertEqual(state["notion"][value["key"]]["page_id"], "reconciled-page")

    def test_failed_intent_commit_prevents_post(self):
        value = task()
        state = {"tasks": {value["key"]: value}, "active_tasks": [value["key"]]}
        notion = FakeNotion()
        def failure(state): raise p.Stop("fixture commit refused")
        with self.assertRaises(p.Stop): p.sync_tasks(notion, state, CONFIG, failure)
        self.assertFalse(any(c[:2] == ("POST", "pages") for c in notion.calls))

    def test_patch_never_sends_human_fields_and_new_recipient_stops(self):
        value = task()
        state = {"tasks": {value["key"]: value}, "active_tasks": [value["key"]],
            "notion": {value["key"]: {"page_id": "human-edited-page"}}}
        notion = FakeNotion()
        p.sync_tasks(notion, state, CONFIG, lambda s: None)
        body = [c[2] for c in notion.calls if c[:2] == ("PATCH", "pages/human-edited-page")][0]
        self.assertTrue(set(body["properties"]).isdisjoint({"Статус", "Ответственный", "Плановая дата", "Дедлайн"}))
        prior_calls = len(notion.calls)
        with self.assertRaises(p.Stop): p.sync_tasks(notion, state, dict(CONFIG, notion_data_source="other-source"), lambda s: None)
        self.assertEqual(len(notion.calls), prior_calls)
        p.sync_tasks(notion, state, CONFIG, lambda s: None)
        self.assertEqual(len(notion.calls), prior_calls)

    def test_calendar_rolls_week_month_year_in_moscow(self):
        notion = FakeNotion()
        p.roll_views(notion, dict(CONFIG, notion_views={"today": "t", "week": "w", "month": "m"}), dt.date(2027, 1, 1))
        self.assertIn("2026-12-28–2027-01-03", notion.calls[1][2]["name"])
        self.assertIn("2027-01-01–2027-01-31", notion.calls[2][2]["name"])

    def test_report_append_intent_survives_lost_reply(self):
        state = {"reports": result(row())["reports"]}
        config = dict(CONFIG, notion_reports={"summary": "report-page"})
        notion = FakeNotion()
        original = notion.request
        def failure(method, path, body=None, create=False):
            if create: raise p.Stop("lost append")
            return original(method, path, body, create)
        notion.request = failure
        with self.assertRaises(p.Stop): p.sync_reports(notion, state, config, lambda s: None)
        notion.request = original
        with self.assertRaises(p.Stop): p.sync_reports(notion, state, config, lambda s: None)
        self.assertFalse(any(c[:2] == ("PATCH", "blocks/report-page/children") for c in notion.calls))

    def test_stale_queued_oauth_is_rejected_before_auth_file(self):
        with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {"PATH": "/bin", "CODEX_AUTH_WRITER_TOKEN": "fixture", "GITHUB_TOKEN": "fixture", "GITHUB_RUN_ID": "1"}):
            oauth = p.OAuth("fixture/private", Path(root) / "session", "fixture")
            replies = [{"created_at": "2026-10-08T12:00:00Z"}, {"updated_at": "2026-10-08T12:00:00Z"}]
            with patch.object(p, "gh_json", side_effect=replies), self.assertRaises(p.Stop): oauth.restore()
            self.assertFalse(oauth.home.exists())

    def test_oauth_writeback_refuses_changed_secret_and_keeps_api_out(self):
        with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {"CODEX_AUTH_WRITER_TOKEN": "fixture"}):
            oauth = p.OAuth("fixture/private", Path(root), "fixture")
            oauth.version = "2026-10-08T12:00:00Z"
            p.atomic_json(Path(root) / "auth.json", {"auth_mode": "chatgpt", "tokens": {"refresh_token": "synthetic"}})
            with patch.object(p, "gh_json", return_value={"updated_at": "2026-10-08T12:00:01Z"}), patch.object(p, "run") as mocked, self.assertRaises(p.Stop): oauth.persist()
            mocked.assert_not_called()


class History(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.repo = Path(self.root.name)
        p.git(self.repo, "init", "-b", "main")
        p.git(self.repo, "config", "user.name", "Fixture")
        p.git(self.repo, "config", "user.email", "fixture@localhost")
        self.prefix = "packages/project-FIXTURE"
        self.path = self.prefix + "/messages.jsonl"
        (self.repo / self.prefix).mkdir(parents=True)
        self.env = patch.dict(os.environ, {"NOTION_TOKEN": "synthetic", "GITHUB_REPOSITORY": "fixture/private", "GITHUB_EVENT_NAME": "workflow_dispatch"})
        self.env.start()
    def tearDown(self):
        self.env.stop()
        self.root.cleanup()
    def commit(self, records, selection="a"):
        (self.repo / self.path).write_bytes(b"\n".join(map(p.encoded, records)))
        p.atomic_json(self.repo / self.prefix / "package.json", {"cloud_processing": True, "local_only": False})
        p.atomic_json(self.repo / self.prefix / "coverage.json", {"selection": "selection_" + selection * 64})
        p.git(self.repo, "add", ".")
        p.git(self.repo, "commit", "-m", "Synthetic messages")
        return p.git(self.repo, "rev-parse", "HEAD")
    def test_all_intermediate_edits_and_cross_chat_ids_survive(self):
        base = self.commit([row(text="A")])
        self.commit([row(text="B", revision="c")])
        self.commit([row(text="C", revision="d"), row("b", "Other message")])
        values = p.changes(self.repo, self.path, base, p.git(self.repo, "rev-parse", "HEAD"))
        self.assertEqual([r["text"] for r in values], ["B", "C", "Other message"])
    def test_check_only_verifies_oauth_independently_of_notion_without_analysis(self):
        self.commit([row()])
        calls = []
        class Auth:
            def __init__(self, *args): pass
            def restore(self): calls.append("oauth_restore")
            def persist(self): calls.append("oauth_writeback")
        class Refused(FakeNotion):
            def request(self, *args, **kwargs):
                calls.append("notion_check")
                raise p.Stop("Notion HTTP 401 fixture")
        analysis = Mock(side_effect=AssertionError("check-only must not analyze"))
        saver = lambda repo,path,value: p.atomic_json(repo / path,value)
        with patch.dict(os.environ, {"TGSUM_CHECK_ONLY": "true"}), contextlib.redirect_stdout(io.StringIO()) as output:
            with self.assertRaisesRegex(p.Stop, "Notion HTTP 401"):
                p.worker(self.repo, CONFIG, Auth, analysis, Refused, saver)
        self.assertEqual(calls, ["oauth_restore", "oauth_writeback", "notion_check"])
        self.assertIn("Inference ещё не выполнялся", output.getvalue())
        analysis.assert_not_called()
        state = p.read_json(self.repo / "state/project-FIXTURE.json")
        self.assertNotIn("last_attempt", state)
        self.assertFalse((self.repo / "results").exists())
    def test_narrow_selection_cancels_old_wide_range(self):
        a, b = row("a", "Allowed"), row("b", "Removed sentinel")
        wide = self.commit([a, b])
        state_path = self.repo / "state/project-FIXTURE.json"
        p.atomic_json(state_path, {"schema_version": 1, "tasks": {}, "notion": {}, "selection": "selection_" + "a" * 64,
            "range": {"base": None, "target": wide, "offset": 1}})
        self.commit([a], "b")
        received = []
        class Auth:
            def __init__(self, *args): pass
            def restore(self): pass
            def persist(self): pass
        def analysis(batch, *args):
            received.extend(batch)
            return p.validate_result(result(a), [a], [])
        saver = lambda repo, path, value: p.atomic_json(repo / path, value)
        p.worker(self.repo, CONFIG, Auth, analysis, FakeNotion, saver)
        self.assertEqual([r["text"] for r in received], ["Allowed"])
    def test_notion_failure_reuses_analysis_result_without_inference(self):
        a = row()
        self.commit([a])
        calls = []
        class Auth:
            def __init__(self, *args): pass
            def restore(self): pass
            def persist(self): calls.append("writeback")
        def analysis(batch, *args):
            calls.append("analysis")
            return p.validate_result(result(a), [a], [])
        notion = FakeNotion()
        notion.fail_create = True
        saver = lambda repo, path, value: p.atomic_json(repo / path, value)
        with self.assertRaises(p.Stop): p.worker(self.repo, CONFIG, Auth, analysis, lambda _: notion, saver)
        self.assertEqual(calls, ["analysis", "writeback"])
        notion.pages = [{"id": "reconciled"}]
        p.worker(self.repo, CONFIG, Auth, analysis, lambda _: notion, saver)
        self.assertEqual(calls, ["analysis", "writeback"])

    def test_privacy_change_does_not_reuse_old_derived_text_with_same_refs(self):
        old = row(text="SENSITIVE_SENTINEL сделать проверку")
        old_task = task(old)
        old_task["details"] = "SENSITIVE_SENTINEL"
        self.commit([old])
        p.atomic_json(self.repo / "state/project-FIXTURE.json", {
            "schema_version": 1, "tasks": {old_task["key"]: old_task}, "active_tasks": [old_task["key"]],
            "selection": "selection_" + "a" * 64, "notion": {old_task["key"]: {"page_id": "old-page"}}})
        current = row(text="Person 1 сделать проверку")  # same native content revision, newly masked view
        self.commit([current], "b")
        class Auth:
            def __init__(self, *args): pass
            def restore(self): pass
            def persist(self): pass
        notion = FakeNotion()
        def analysis(batch, previous, home, coverage, context):
            self.assertEqual(previous, [])
            self.assertNotIn("SENSITIVE_SENTINEL", p.encoded([batch, previous, context]).decode())
            self.assertFalse(any(c[0] == "PATCH" and c[1].startswith("pages/") for c in notion.calls))
            return p.validate_result(result(current), [current], [])
        p.worker(self.repo, CONFIG, Auth, analysis, lambda _: notion, lambda repo,path,value: p.atomic_json(repo / path,value))

    def test_failed_analysis_has_durable_hourly_cooldown_and_auth_writeback(self):
        self.commit([row()])
        calls = []
        class Auth:
            def __init__(self, *args): pass
            def restore(self): pass
            def persist(self): calls.append("writeback")
        def failure(*args):
            calls.append("analysis")
            raise p.Stop("fixture quota")
        saver = lambda repo,path,value: p.atomic_json(repo / path,value)
        config = dict(CONFIG, min_interval_seconds=3600)
        with self.assertRaises(p.Stop): p.worker(self.repo,config,Auth,failure,FakeNotion,saver)
        p.worker(self.repo,config,Auth,failure,FakeNotion,saver)
        self.assertEqual(calls,["analysis","writeback"])

    def test_context_and_cached_result_are_bound_to_pinned_target(self):
        a = row("a", "Pinned allowed")
        target = self.commit([a])
        p.atomic_json(self.repo / "state/project-FIXTURE.json", {
            "schema_version":1,"tasks":{},"notion":{},"active_tasks":[],"selection":"selection_"+"a"*64,
            "range":{"base":None,"target":target,"offset":0}})
        self.commit([a,row("b","LATER_HEAD_SENTINEL")])
        calls=[]
        class Auth:
            def __init__(self,*args):pass
            def restore(self):pass
            def persist(self):pass
        def analysis(batch,previous,home,coverage,context):
            self.assertEqual(coverage["input_sha"],target)
            self.assertNotIn("LATER_HEAD_SENTINEL",p.encoded([batch,context]).decode())
            calls.append("analysis")
            return p.validate_result(result(a),batch+context,[])
        def interrupted(repo,path,value):
            if path.startswith('state/') and value.get('tasks'):
                raise p.Stop('fixture failed checkpoint after durable result')
            p.atomic_json(repo/path,value)
        with self.assertRaises(p.Stop):p.worker(self.repo,CONFIG,Auth,analysis,FakeNotion,interrupted)
        paths=list((self.repo/'results/project-FIXTURE').glob('*.json'))
        self.assertEqual(len(paths),1)
        poisoned=p.read_json(paths[0])
        poisoned['tasks'][0]['evidence'][0]['quote']='FABRICATED_CACHED_SENTINEL'
        p.atomic_json(paths[0],poisoned)
        notion=FakeNotion()
        with self.assertRaises(p.Stop):p.worker(self.repo,CONFIG,Auth,analysis,lambda _:notion,lambda r,path,v:p.atomic_json(r/path,v))
        self.assertFalse(any(c[:2]==('POST','pages') for c in notion.calls))
        self.assertEqual(calls,['analysis'])


if __name__ == "__main__":
    unittest.main()
