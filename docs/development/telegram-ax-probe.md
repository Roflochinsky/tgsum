# macOS Telegram Desktop: controlled AX observation

`scripts/telegram-ax-probe.swift` is a **read-only diagnostic** for
`tgsum-i1w.4.1` and user-controlled qualification `tgsum-t8t.25`. It does
not launch Telegram, perform an Accessibility action, export data or enable the
macOS Bridge driver. Do not run it against a real client without the user's
control. Automatic export remains disabled.

The operator supplies one PID, exact `.app` bundle path and fixed stage. The
probe checks the running app's bundle URL and launch instance, then checks AX
trust **without prompting**. It reads only allowlisted AX roles/actions,
identifier shape, enabled/selected flags and tree paths. It never requests
AXTitle, AXValue, AXDescription, message text, raw identifiers, credentials,
session files or process memory. Different-process child elements are excluded.
The tree is bounded to 256 nodes, depth 6 and 64 children per node; AX
messaging has a 0.8-second timeout and the worker has a 30-second outer limit.
Unknown errors are redacted. These are TGSUM diagnostic limits, not claims
about Telegram's service limits.

On a user-controlled macOS session with a disposable test chat, the operator
can compile and run one observation after manually navigating to a stage:

```sh
swiftc scripts/telegram-ax-probe.swift -o /tmp/tgsum-telegram-ax-probe
/tmp/tgsum-telegram-ax-probe --pid <TELEGRAM_PID> \
  --bundle /Applications/Telegram.app --stage settings --observe \
  > /tmp/tgsum-telegram-ax-settings.json
```

Accepted stages are `chat`, `menu`, `settings`, `format`, `folder`, `progress`,
`done` and `cancel`. Keep local reports private and review them before sharing.
The diagnostic does not authenticate the app's code signature, establish the
native account/chat ID, test whether an advertised action works, or tie Done
to a new export. It reports `application_version: unknown`; bundle path and
version data do not qualify the installed binary. [The source review](../research/telegram-macos-bridge-2026-09-27.md)
explains the current no-go, including likely missing AXPress in the reviewed
Qt/lib_ui chain and differences between website Desktop and Telegram Lite.

The CI `test (macos-latest)` job compiles the tool and runs `--self-test`
against fake trees only. It does not request Accessibility permission or access
Telegram. The implementation uses Apple's documented
[AXUIElement](https://developer.apple.com/documentation/applicationservices/axuielement),
[bounded child reads](https://developer.apple.com/documentation/applicationservices/1462060-axuielementcopyattributevalues)
and [messaging timeout](https://developer.apple.com/documentation/applicationservices/1459345-axuielementsetmessagingtimeout).
