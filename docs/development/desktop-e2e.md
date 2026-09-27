# Actual desktop E2E

`scripts/desktop-e2e.py` drives the **real application WebView** with an embedded
W3C WebDriver. Source selection, parsing, Project storage, sanitization, Review,
bundle export and result rendering use the application UI and production IPC.
The only substituted boundaries are native file-picker responses and the
existing synthetic analysis backend. No messenger or agent accounts are used.

## Run

Install the normal desktop build dependencies and Python 3.12 or newer. No npm
frontend build, Selenium package, external browser or standalone driver is needed.

```sh
cargo build --locked -p tgsum --features desktop-e2e
python scripts/desktop-e2e.py
```

On a headless Linux runner, install Xvfb and a session D-Bus:

```sh
GDK_BACKEND=x11 dbus-run-session -- xvfb-run -a python scripts/desktop-e2e.py
```

For a local development run through Cargo:

```sh
python scripts/desktop-e2e.py --cargo-run --report-dir /tmp/tgsum-desktop-report
```

The script launches its own application and checks a per-run nonce, an empty
Project store, empty UI preferences and a synthetic agent catalog before driving
controls. Do not point it at an existing application. It waits for the native
`PageLoadEvent::Finished` before creating a WebDriver session: a listening server
and an existing window alone were insufficient in the first local run.

## Isolation and artifacts

- The optional `desktop-e2e` feature registers the pinned driver **1.4.0** only
  in a debug build with explicit `TGSUM_DESKTOP_E2E_ROOT` configuration. Ordinary
  debug launches have no listener. Release code contains no registration or
  native-picker substitution; ordinary builds have no driver dependency enabled.
- The fresh temporary root holds synthetic exports, output, ProjectStore and
  a predeclared picker queue. Reusing a Project directory is refused. Picker
  paths are canonicalized and must remain within that root.
- The test window uses an incognito WebView and a temporary data directory.
  Incognito is essential on macOS, where `data_directory` does not select a
  separate WKWebView store. Desktop-entry updates and external openers are
  disabled for this invocation.
- The application listener binds loopback. The runner uses no proxy for it,
  has bounded startup/script timeouts, returns failure for missing assertions,
  and terminates its own application on exit.
- Reports default to ignored `desktop-e2e-report/`: `report.json`, application
  log and actual WebView screenshots. The JSON records checked stages, source
  revision/dirty flag, OS, architecture, runner image, Python/Rust, WebView
  version, renderer UA and the simulated boundaries. Temporary synthetic data
  is retained at the reported path for diagnosis.

## Covered flow

1. First-run onboarding and Project creation through rendered controls.
2. Full JSON import, selection of a direct chat and one forum topic, saved
   date range, privacy preset and sanitized Review.
3. Export only: actual prepared Markdown is written and checked for secret leaks.
4. Each synthetic Codex/Claude adapter: Review, explicit Run, validated result
   and rendered evidence. No installed agent executable or auth is used.
5. Relink to a single-chat JSON, refresh the snapshot, preserve selected scope,
   invalidate old Review and reopen the saved Project.
6. Single-chat one-off topic export and actual Markdown content verification.
7. Malformed JSON error and absence of uncaught renderer exceptions.

The CI `desktop-e2e` matrix executes this same script on Ubuntu, Windows and
macOS, uploading artifacts even after failure. A configured job is not a passing
run; its actual result and image/version determine the verified scope.

This suite does not qualify native pickers, OS keyboard/accessibility behavior,
installers, real agent process isolation or official messenger automation.
The separate Linux assisted-export smoke exercises synthetic native pickers.
Real account and user-client tests remain under the user's control.

Tool selection, upstream version/CI evidence and platform limitations are in
[the dated research](../research/desktop-e2e-platforms-2026-09-27.md).
