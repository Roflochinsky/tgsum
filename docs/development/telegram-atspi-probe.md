# Linux Telegram Desktop: controlled AT-SPI observation

`scripts/telegram-atspi-probe.py` is a **read-only diagnostic** for
`tgsum-i1w.2.1` and the user-controlled backlog `tgsum-t8t.23`. It does not
launch a client, select a chat, press buttons, export data or enable the
Official Client Bridge. Do not run it against a real Telegram process without
the user's control. TGSUM's automatic export remains disabled.

The operator supplies one PID, the exact executable path and a fixed stage.
The probe verifies that the process belongs to the current user and still
runs that executable, holds a Linux pidfd, and observes only its AT-SPI tree.
When the same process registers a GTK shell alongside exactly one Qt root,
the probe selects that Qt root. Multiple Qt roots, or an unknown toolkit sibling,
remain ambiguous and are rejected. A sole GTK root is retained for diagnostics
and the synthetic transport test; it does not establish that Telegram's actual
Qt controls are accessible.
It reads roles, machine action names, state flags, toolkit version and the
**shape** of accessibility IDs. It never requests accessible names, message
text, descriptions, raw IDs, credentials, session data, screenshots or process
memory. Unknown action names and roles are reported as `other`; provider error
details are omitted. The report has at most 256 nodes, depth 6 and 64 children
per node, and a 30-second worker limit. `truncated: true` means the observation
is incomplete. [GNOME documents `Atspi.set_timeout` in milliseconds](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/func.set_timeout.html);
the probe uses 800 ms for calls and 1500 ms for startup.

On a user-controlled session with a known test chat, the operator can navigate
Telegram manually to a stage, then run one explicit observation:

```sh
python -B scripts/telegram-atspi-probe.py \
  --pid <TELEGRAM_PID> --executable /usr/bin/Telegram \
  --stage settings --observe > /tmp/tgsum-telegram-settings-probe.json
```

Repeat separately for `chat`, `menu`, `format`, `folder`, `progress`, `done` and
`cancel` only as needed. The probe does not invoke any action. Keep local reports
private; review them before sharing. A report cannot prove that `press` has the
correct business effect, that an account/chat native ID was selected, or that
Done belongs to a new export. Those points need the separate controlled PoC
specified in [Linux research](../research/telegram-linux-bridge-2026-09-27.md).
In particular, AT-SPI toolkit version is **not** Telegram's application version.
An absent action or unknown identity is a no-go, not a reason to try coordinates
or a global keyboard shortcut.

## Accessibility activation on the current Linux host

In the controlled 2026-10-02 Omarchy/Hyprland/Wayland observation, both
`org.a11y.Status.IsEnabled` and `ScreenReaderEnabled` initially were false.
The selected Telegram process exposed only two GTK nodes (application/window,
toolkit 3.24.52), even with its export settings open. Temporarily setting
`IsEnabled` true exposed separate GTK and Qt 6.11.2 roots for that same PID;
this revealed the mixed-root selection bug now covered by the fake suite.
No titles or message text were read, and no actions were invoked. Package metadata was
`telegram-desktop 7.2.5-1`; the running application version remains unconfirmed.

[Qt documents the activation flags and `QT_LINUX_ACCESSIBILITY_ALWAYS_ON`](https://doc.qt.io/qt-6/qaccessible.html)
(read 2026-10-02). Its generic description covers Unix/X11; the observation
above is evidence for this particular Wayland session, not every Qt build.
For a user-controlled restart, the per-process environment variable is an
alternative to changing session-wide accessibility flags. Reusing an already
running single-instance client does not establish that it received the variable.
Record original session values and restore them after any temporary experiment.

Do not run the CI `dbus-run-session` recipe on this live Omarchy session:
its AT-SPI launcher reused the active session's socket and broke connectivity.
Restarting the user `at-spi-dbus-bus.service` restored the bus, and the native
fake-window test then passed on the existing session, but Telegram's Qt root
did not reappear with either activation flag. A user-controlled Telegram restart
with `QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1` restored the Qt root while both global
flags remained false. CI uses its own isolated machine/display.
The native test on the existing session opens only its own fake GTK window and
binds the probe to that process. These observations do not enable automation;
native account/chat identity and completion remain unqualified.

After the user reopened export settings, the fixed CLI selected Qt 6.11.2 and
returned 119 nodes, but `truncated: true` because the application-wide depth
limit cut off parts of the dialog. A separate bounded read-only observation of
the unique active/showing direct child of that Qt root used depth 12, the same
256-node/64-child bounds and a 30-second outer timeout. It normalized roles from
the AT-SPI enum (provider role labels otherwise often map to `other`) and
`setFocus` to `focus`, without reading names/text or invoking actions. This
supplement is not an added CLI mode. The dialog contained 30 nodes, including
seven checkboxes, with `truncated: false`; its 12 advertised actions were all
focus. There were no advertised press/click/toggle actions in that subtree.
This blocks the current AT-SPI export-action path for the observed build;
manual export/import checks can continue. It does not prove the behavior of
other builds, settings screens, or native account/chat identity.

Offline fake-tree validation requires no desktop or account:

```sh
python -B scripts/test-telegram-atspi-probe.py
```

The CI `connectors` job runs these fake-tree tests and a native transport check
under Xvfb/D-Bus. The latter creates its own GTK window, calls the real AT-SPI
probe against that process, checks that private-looking labels and raw IDs are
absent, and rejects a wrong executable or PID. It terminates only the fake GTK
process. Neither check installs or opens Telegram.

This host's read-only package metadata showed Arch `telegram-desktop 7.2.5-1`
at `/usr/bin/Telegram` on 2026-09-27. The source review pinned upstream
`v7.2.9`; it cannot qualify the installed package or its patches. No installed
Telegram process or account was opened during this tool's development.
