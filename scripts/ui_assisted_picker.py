"""Native GTK picker driver for an explicitly owned synthetic Tauri test PID.

Called by ui-assisted-smoke.py using the system Python with PyGObject. Refuses
to type unless Hyprland's active window belongs to that exact test process.
"""
import json
import subprocess
import sys
import time

import gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi


def nodes(element):
    yield element
    for child in element:
        yield from nodes(child)


def showing(element):
    return element.get_state_set().contains(Atspi.StateType.SHOWING)


def main():
    pid, path = int(sys.argv[1]), sys.argv[2]
    assert path.startswith('/tmp/tgsum-assisted-ui-'), 'Synthetic paths only'
    deadline = time.monotonic() + 10
    dialog = None
    while time.monotonic() < deadline:
        app = next((a for a in Atspi.get_desktop(0) if a.get_process_id() == pid), None)
        if app:
            dialog = next((c for c in app if c.get_role_name() == 'file chooser' and showing(c)), None)
        if dialog:
            break
        time.sleep(.05)
    assert dialog is not None, 'Owned test picker did not open'

    def key(*args):
        active = json.loads(subprocess.check_output(['hyprctl', 'activewindow', '-j']))
        assert active.get('pid') == pid, 'Test window must be active for keyboard input'
        subprocess.run(['wtype', *args], check=True)
        time.sleep(.15)

    # Recent's empty list may ignore Ctrl+L. Selecting Home then slash opens
    # GTK's location field without clicking coordinates or other applications.
    places = next(x for x in nodes(dialog) if x.get_role_name() == 'list box' and showing(x))
    for row in places:
        if any(x.get_role_name() == 'label' and x.get_name() == 'Home' for x in nodes(row)):
            places.get_selection_iface().select_child(row.get_index_in_parent())
            row.get_component_iface().grab_focus()
            key('-k', 'Return')
            break
    key('-k', 'slash')
    entry = next(x for x in nodes(dialog) if x.get_role_name() == 'text'
                 and showing(x) and x.get_state_set().contains(Atspi.StateType.FOCUSED))
    entry.get_editable_text_iface().set_text_contents(path)
    button = next(x for x in nodes(dialog) if x.get_role_name() == 'button'
                  and x.get_name() == 'Open' and showing(x))
    button.get_action_iface().do_action(0)
    time.sleep(.25)
    if showing(dialog):
        button.get_action_iface().do_action(0)


if __name__ == '__main__':
    main()
