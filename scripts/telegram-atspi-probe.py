"""Read-only, bounded Linux AT-SPI observation for a user-controlled PoC.

This tool never launches Telegram, invokes accessibility actions, reads names,
text, descriptions, credentials or process memory, or records raw accessible IDs.
It observes only the explicit same-user PID/executable supplied by the operator.
"""

import argparse
from collections import deque
import json
import multiprocessing
import os
from pathlib import Path
import re
import select
import sys

MAX_APPLICATIONS = 128
MAX_NODES = 256
MAX_DEPTH = 6
MAX_CHILDREN = 64
MAX_ACTIONS = 16
WORKER_SECONDS = 30

ROLES = frozenset({
    "application", "frame", "dialog", "panel", "menu", "menu item", "push button",
    "radio button", "check box", "text", "static", "label", "list", "list item",
    "page tab", "scroll pane", "window", "unknown",
})
ACTIONS = frozenset({"press", "click", "showmenu", "focus", "toggle", "select"})
STAGES = ("chat", "menu", "settings", "format", "folder", "progress", "done", "cancel")


class ProbeError(Exception):
    """An error code from our own fixed vocabulary, never a provider message."""


def safe(call, default=None):
    try:
        return call()
    except Exception:
        return default


def kind_of_id(value):
    if not value:
        return "empty"
    return "numeric" if re.fullmatch(r"[0-9]+", value) else "opaque"


def toolkit_version(value):
    value = str(value or "")
    return value if re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}", value) else "unknown"


def capture_tree(root, state_types):
    """Capture structural facts only; fake nodes can exercise this offline."""
    nodes = []
    pending = deque([([], root)])
    truncated = False

    def read(call, default=None):
        nonlocal truncated
        try:
            return call()
        except Exception:
            truncated = True
            return default

    while pending:
        path, node = pending.popleft()
        if len(nodes) >= MAX_NODES:
            truncated = True
            break
        role = str(read(node.get_role_name, "unknown") or "unknown").lower()
        role = role if role in ROLES else "other"
        action = read(node.get_action)
        action_count = read(action.get_n_actions, 0) if action is not None else 0
        action_count = max(0, int(action_count or 0))
        actions = []
        for index in range(min(action_count, MAX_ACTIONS)):
            name = str(read(lambda: action.get_action_name(index), "") or "").lower()
            actions.append(name if name in ACTIONS else "other")
        states = read(node.get_state_set)
        if states is None:
            truncated = True
        flags = {
            name: bool(read(lambda: states.contains(value), False)) if states is not None else False
            for name, value in state_types.items()
        }
        children = max(0, int(read(node.get_child_count, 0) or 0))
        nodes.append({
            "path": path,
            "role": role,
            "actions": actions,
            "id_kind": kind_of_id(read(node.get_accessible_id, "")),
            "states": flags,
            "children": children,
        })
        if action_count > MAX_ACTIONS or children > MAX_CHILDREN or (children and len(path) >= MAX_DEPTH):
            truncated = True
        if len(path) >= MAX_DEPTH:
            continue
        slots = max(0, MAX_NODES - len(nodes) - len(pending))
        if children > slots:
            truncated = True
        for index in range(min(children, MAX_CHILDREN, slots) - 1, -1, -1):
            child = read(lambda: node.get_child_at_index(index))
            if child is not None:
                pending.appendleft((path + [index], child))
            else:
                truncated = True
    return {"nodes": nodes, "truncated": truncated}


def selected_process(pid, executable):
    if pid <= 0 or not executable.is_absolute() or not executable.is_file():
        raise ProbeError("invalid_target")
    if os.stat(f"/proc/{pid}").st_uid != os.getuid():
        raise ProbeError("different_user")
    if not os.path.samefile(f"/proc/{pid}/exe", executable):
        raise ProbeError("executable_mismatch")
    return os.pidfd_open(pid)


def observe(pid, executable, stage):
    pidfd = selected_process(pid, executable)
    try:
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi

        Atspi.set_timeout(800, 1500)
        desktop = Atspi.get_desktop(0)
        count = max(0, desktop.get_child_count())
        if count > MAX_APPLICATIONS:
            raise ProbeError("application_search_too_large")
        matches = []
        for index in range(count):
            app = desktop.get_child_at_index(index)
            if app is None:
                raise ProbeError("application_search_incomplete")
            app_pid = app.get_process_id()
            if app_pid == pid:
                matches.append(app)
        if len(matches) > 1:
            # Telegram can register both a GTK shell and its actual Qt UI.
            # Select Qt only when every sibling is known GTK; never pick the
            # first root or resolve multiple Qt roots by titles/chat content.
            toolkits = [str(app.get_toolkit_name() or "").lower() for app in matches]
            qt = [app for app, toolkit in zip(matches, toolkits) if toolkit == "qt"]
            if len(qt) == 1 and all(toolkit in {"qt", "gtk"} for toolkit in toolkits):
                matches = qt
        if len(matches) != 1:
            raise ProbeError("application_not_unique_or_not_accessible")
        app = matches[0]
        tree = capture_tree(app, {
            "active": Atspi.StateType.ACTIVE,
            "focused": Atspi.StateType.FOCUSED,
            "selected": Atspi.StateType.SELECTED,
            "showing": Atspi.StateType.SHOWING,
            "enabled": Atspi.StateType.ENABLED,
        })
        poller = select.poll()
        poller.register(pidfd, select.POLLIN)
        if poller.poll(0) or not os.path.samefile(f"/proc/{pid}/exe", executable):
            raise ProbeError("process_changed_during_probe")
        return {
            "schema_version": 1,
            "stage": stage,
            "pid": pid,
            "toolkit_name": "Qt" if str(safe(app.get_toolkit_name, "") or "").lower() == "qt" else "other",
            "toolkit_version": toolkit_version(safe(app.get_toolkit_version)),
            "application_version": "unknown",
            **tree,
        }
    finally:
        os.close(pidfd)


def worker(queue, pid, executable, stage):
    try:
        queue.put({"ok": observe(pid, executable, stage)})
    except Exception as error:
        code = str(error) if isinstance(error, ProbeError) else "probe_failed"
        queue.put({"error": code})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--stage", choices=STAGES, required=True)
    parser.add_argument("--observe", action="store_true", help="explicitly observe the selected live process")
    args = parser.parse_args()
    if not args.observe:
        parser.error("--observe is required; no process was inspected")
    ctx = multiprocessing.get_context("spawn")
    queue = ctx.Queue(maxsize=1)
    process = ctx.Process(target=worker, args=(queue, args.pid, args.executable, args.stage))
    process.start()
    process.join(WORKER_SECONDS)
    if process.is_alive():
        process.terminate()
        process.join(2)
        if process.is_alive():
            process.kill()
            process.join()
        result = {"error": "probe_timeout"}
    elif process.exitcode:
        result = {"error": "probe_failed"}
    else:
        result = queue.get(timeout=1)
    print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
    return 0 if "ok" in result else 1


if __name__ == "__main__":
    sys.exit(main())
