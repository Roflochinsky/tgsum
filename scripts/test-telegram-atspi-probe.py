"""Offline fake-tree checks; never connects to a desktop or Telegram process."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

PATH = Path(__file__).with_name("telegram-atspi-probe.py")
SPEC = importlib.util.spec_from_file_location("telegram_atspi_probe", PATH)
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


class FakeStates:
    def contains(self, value):
        return value == "selected"


class FakeAction:
    def __init__(self, names):
        self.names = names

    def get_n_actions(self):
        return len(self.names)

    def get_action_name(self, index):
        return self.names[index]

    def do_action(self, _index):
        raise AssertionError("probe must never invoke an action")


class FakeNode:
    def __init__(self, role, identifier, actions=(), children=()):
        self.role = role
        self.identifier = identifier
        self.actions = FakeAction(actions)
        self.children = children

    def get_role_name(self):
        return self.role

    def get_accessible_id(self):
        return self.identifier

    def get_action(self):
        return self.actions

    def get_state_set(self):
        return FakeStates()

    def get_child_count(self):
        return len(self.children)

    def get_child_at_index(self, index):
        return self.children[index]

    def get_name(self):
        raise AssertionError("probe must never read a chat/control name")

    def get_text(self):
        raise AssertionError("probe must never read message text")


class FakeApplication(FakeNode):
    def __init__(self, pid, toolkit, children=()):
        super().__init__("application", "", children=children)
        self.pid = pid
        self.toolkit = toolkit

    def get_process_id(self):
        return self.pid

    def get_toolkit_name(self):
        return self.toolkit

    def get_toolkit_version(self):
        return "6.11.2" if self.toolkit == "Qt" else "3.24.52"


class ProbeTests(unittest.TestCase):
    def observe_fake_applications(self, applications):
        desktop = FakeNode("application", "", children=applications)
        atspi = SimpleNamespace(
            get_desktop=lambda _: desktop,
            set_timeout=lambda *_: None,
            StateType=SimpleNamespace(ACTIVE="active", FOCUSED="focused",
                                     SELECTED="selected", SHOWING="showing", ENABLED="enabled"),
        )
        with patch.dict(sys.modules, {
            "gi": SimpleNamespace(require_version=lambda *_: None),
            "gi.repository": SimpleNamespace(Atspi=atspi),
        }):
            return probe.observe(os.getpid(), Path(sys.executable).resolve(), "settings")

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux pidfd required")
    def test_observe_selects_qt_from_same_process_gtk_and_qt_roots(self):
        gtk = FakeApplication(os.getpid(), "gtk", [FakeNode("window", "")])
        qt = FakeApplication(os.getpid(), "Qt", [
            FakeNode("push button", "private-id", actions=("press",)),
        ])
        other_process = FakeApplication(os.getpid() + 1, "Qt")
        for applications in ([gtk, qt, other_process], [other_process, qt, gtk]):
            with self.subTest(order=[a.toolkit for a in applications]):
                report = self.observe_fake_applications(applications)
                self.assertEqual(report["toolkit_name"], "Qt")
                self.assertEqual(report["nodes"][1]["actions"], ["press"])
                self.assertNotIn("private-id", json.dumps(report))

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux pidfd required")
    def test_observe_rejects_ambiguous_toolkits_for_selected_process(self):
        for toolkits in (("Qt", "Qt"), ("gtk", "gtk"), ("Qt", "unknown")):
            with self.subTest(toolkits=toolkits):
                applications = [FakeApplication(os.getpid(), t) for t in toolkits]
                with self.assertRaisesRegex(probe.ProbeError, "application_not_unique_or_not_accessible"):
                    self.observe_fake_applications(applications)

    def test_report_contains_only_structure_and_whitelisted_actions(self):
        secret = "Private Customer Alice message 123"
        tree = FakeNode("application", "session-private", children=[
            FakeNode("push button", "123456789", actions=("press", secret)),
            FakeNode("StaticText", "chat-private", actions=()),
        ])
        report = probe.capture_tree(tree, {"selected": "selected", "focused": "focused"})
        body = json.dumps(report)
        self.assertNotIn(secret, body)
        self.assertNotIn("123456789", body)
        self.assertNotIn("session-private", body)
        self.assertNotIn("chat-private", body)
        self.assertEqual(report["nodes"][1]["actions"], ["press", "other"])
        self.assertEqual(report["nodes"][1]["id_kind"], "numeric")
        self.assertEqual(report["nodes"][2]["role"], "other")

    def test_large_and_deep_trees_stop_without_invoking_controls(self):
        leaf = FakeNode("push button", "", actions=("press",))
        wide = FakeNode("application", "", children=[leaf] * (probe.MAX_CHILDREN + 100))
        report = probe.capture_tree(wide, {})
        self.assertTrue(report["truncated"])
        self.assertEqual(len(report["nodes"]), probe.MAX_CHILDREN + 1)
        chain = leaf
        for _ in range(probe.MAX_DEPTH + 2):
            chain = FakeNode("panel", "", children=[chain])
        report = probe.capture_tree(chain, {})
        self.assertTrue(report["truncated"])
        self.assertEqual(len(report["nodes"]), probe.MAX_DEPTH + 1)

    def test_unreadable_child_marks_observation_incomplete(self):
        class BrokenNode(FakeNode):
            def get_child_at_index(self, _index):
                raise RuntimeError("private content must not appear in the report")

        tree = BrokenNode("application", "", children=[FakeNode("panel", "")])
        report = probe.capture_tree(tree, {})
        self.assertTrue(report["truncated"])
        self.assertEqual(len(report["nodes"]), 1)
        self.assertNotIn("private content", json.dumps(report))

    def test_toolkit_version_and_identifiers_never_echo_arbitrary_values(self):
        self.assertEqual(probe.toolkit_version("6.11.2"), "6.11.2")
        self.assertEqual(probe.toolkit_version("Qt; token=private"), "unknown")
        self.assertEqual(probe.kind_of_id("chat-42"), "opaque")
        self.assertEqual(probe.kind_of_id(""), "empty")

    def test_provider_errors_do_not_echo_data(self):
        class Queue:
            result = None

            def put(self, value):
                self.result = value

        original = probe.observe
        probe.observe = lambda *_: (_ for _ in ()).throw(ValueError("secret chat name"))
        try:
            queue = Queue()
            probe.worker(queue, 7, Path("/synthetic"), "chat")
            self.assertEqual(queue.result, {"error": "probe_failed"})
        finally:
            probe.observe = original

    @unittest.skipUnless(sys.platform.startswith("linux") and shutil.which("sleep"), "Linux synthetic process required")
    def test_explicit_pid_is_bound_to_the_chosen_executable(self):
        executable = Path(shutil.which("sleep")).resolve()
        child = subprocess.Popen([str(executable), "10"], stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        try:
            fd = probe.selected_process(child.pid, executable)
            os.close(fd)
            with self.assertRaises(probe.ProbeError):
                probe.selected_process(child.pid, Path(sys.executable).resolve())
        finally:
            child.terminate()
            child.wait(timeout=2)


if __name__ == "__main__":
    unittest.main()
