# Linux Telegram Desktop: controlled AT-SPI observation

`scripts/telegram-atspi-probe.py` is a **read-only diagnostic** for
`tgsum-i1w.2.1` and the user-controlled backlog `tgsum-t8t.23`. It does not
launch a client, select a chat, press buttons, export data or enable the
Official Client Bridge. Do not run it against a real Telegram process without
the user's control. TGSUM's automatic export remains disabled.

The operator supplies one PID, the exact executable path and a fixed stage.
The probe verifies that the process belongs to the current user and still
runs that executable, holds a Linux pidfd, and observes only its AT-SPI tree.
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
