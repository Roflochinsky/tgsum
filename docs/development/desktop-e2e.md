# Actual desktop E2E

`scripts/desktop-e2e.py` drives the **real application WebView** with an embedded
W3C WebDriver. Source selection, parsing, Project storage, sanitization, Review,
bundle export and result rendering use the application UI and production IPC.
The substituted boundaries are native file-picker responses, the synthetic
analysis backend and an explicitly configured Telegram refresh driver/clock.
No messenger or agent accounts are used.

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

On Linux the continuous-source flow writes synthetic MTP diagnostic frames
inside this run's owned temporary root. Actual Start/Stop/status IPC and the
application worker apply them to the Project. It checks draft preservation,
local-only package controls, source-access metadata and the separate collector
restart gap. It never launches or attaches to Telegram and does not qualify a
real new-message stream or background installation.

## Isolation and artifacts

- The optional `desktop-e2e` feature registers the pinned driver **1.4.0** only
  in a debug build with explicit `TGSUM_DESKTOP_E2E_ROOT` configuration. Ordinary
  debug launches have no listener. Release code contains no registration or
  native-picker substitution; ordinary builds have no driver dependency enabled.
- The fresh temporary root holds synthetic exports, output, ProjectStore and
  a predeclared picker queue. Reusing a Project directory is refused. Picker
  paths are canonicalized and must remain within that root.
- The refresh fixture requires `refresh_fixture: true` in the harness config.
  It writes synthetic single-chat JSON only below the owned temporary root;
  `refresh-control.json` supplies session state, time and simulated outcome.
  It never launches a messenger or inspects sessions. Ordinary app launches
  retain the unavailable production driver.
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

1. Direct file-selection startup without a mandatory dialog, help opened on
   request, optional agent settings in its disclosure, and Project creation
   through rendered controls. A failed project import resets its connection
   state; the next one-off export does not add sources to that project.
2. Full JSON import, selection of a direct chat and one forum topic, saved
   date range, privacy preset and sanitized Review. Source access details
   distinguish whole-file reads, raw snapshot storage and narrower context;
   a markup-like account label remains text. Saving one chat preserves the
   other chat's unsaved dates and blocks preparation until both are saved.
3. Local export without AI: actual prepared Markdown is written and checked
   for secret leaks. Switching projects clears the previous output path and
   folder button.
4. Each synthetic Codex/Claude adapter: Review, explicit Run, validated result
   and rendered evidence. No installed agent executable or auth is used.
5. Relink to a single-chat JSON, refresh the snapshot, preserve selected scope,
   invalidate old Review and reopen the saved Project. A watched export candidate
   requires explicit completion confirmation before repeat import and does not
   run analysis. With two connected Telegram chats, a file for the other native
   chat is rejected without changing either snapshot; a corrected file updates
   only its selected source and reports created/edited/missing counts; missing
   is not presented as a confirmed deletion. An absent or truncated selected
   JSON also leaves both snapshots and the analysis run unchanged and requires
   a fresh human confirmation before retry.
   A separate Project with an unimplemented OAuth source shows unverified
   access and disables unavailable Telegram/archive operations.
6. Refresh scheduling through rendered controls: opt-in on-start cadence,
   no dispatch while locked, one dispatch after unlock, a dynamically named
   completed archive and preservation of the other source and analysis baseline.
   Manual cancellation preserves the snapshot. Session loss leaves an unresolved
   attempt; recovery requires the explicit client-stopped checkbox. The final
   cadence returns to manual. All acquisition events come from the fake driver.
7. Single-chat one-off topic export and actual Markdown content verification.
8. Malformed JSON error and absence of uncaught renderer exceptions.
9. On Linux, local package controls cover output files, privacy handling,
   invalid-input recovery, automatic refresh of provided local files, cleanup
   of the previous generated package and a durable pause. A shared export
   exercises multiple selected chats/topics, one package control, rejection of
   an export missing a selected chat, and automatic combined updates.

The report's `checks` array records the stages that actually ran; Linux package
stages do not run on every platform. Historical runs below describe the suite
at their recorded revisions, before the later extensions. The refresh suite
passed locally through `--cargo-run` on 2026-09-28; its report was written to
`/tmp/tgsum-refresh-coordinator-e2e/report.json` and records a working tree based
on `9e6763b`, not a clean committed revision.

The CI `desktop-e2e` matrix executes this same script on Ubuntu, Windows and
macOS, uploading artifacts even after failure. The `arch` job also builds the
pacman package and the synthetic desktop harness inside `archlinux:base-devel`,
then runs this script as an unprivileged user with Arch's WebKitGTK, Xvfb and
session D-Bus. Its report artifact is `desktop-e2e-arch`. This checks the Arch
runtime under X11; it does not exercise a real Wayland desktop or install the
package. The Arch WebView step and revised startup/regressions were added on
2026-10-07; this section describes their configuration, not a verified cloud
result. A configured job is not a passing run; its actual result, revision and
image/version determine the verified scope.

This suite does not qualify native pickers, OS keyboard/accessibility behavior,
installers, real agent process isolation or official messenger automation.
The separate Linux assisted-export smoke exercises synthetic native pickers.
Real account and user-client tests remain under the user's control.

## Recorded TGSUM run

[CI 36306684685](https://github.com/Roflochinsky/tgsum/actions/runs/36306684685),
source **f3d3a131c975e70fbe292456ee7e658c821590ce**, 2026-09-27: all eight smoke
stages passed on each row. Downloaded `desktop-e2e-*` artifacts confirmed the
same source revision, a clean checkout and these actual runtime versions:

| Environment | Architecture | WebView version | Runner image | E2E job |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04, Linux 6.17.0-1022-azure | x86_64 | WebKitGTK 2.52.6 | 20260920.314.1 | 108584603977 |
| Windows Server 2025, build 26100 | AMD64 | WebView2 153.0.4234.48 | 20260922.246.2 | 108584603925 |
| macOS 26.6.2 | arm64 | WKWebView 21624.5.1.11.3 | 20260907.0351.1 | 108584603932 |

Local `cargo run -p tgsum --features desktop-e2e` also passed on Omarchy
x86_64 / WebKitGTK 2.52.6. The complete Rust gate and Rust 1.88 feature check
passed before publication. These observations qualify this synthetic desktop
flow on the listed environments; they do not imply every Windows/macOS version
or architecture, native picker automation, installer acceptance or real accounts.

Tool selection, upstream version/CI evidence and platform limitations are in
[the dated research](../research/desktop-e2e-platforms-2026-09-27.md).

## Source access extension

[CI 36308526932](https://github.com/Roflochinsky/tgsum/actions/runs/36308526932),
source **1de3f5d8371c1a2c0376bff8d0095efa1ba5bdde**, 2026-09-27: all ten stages
passed in the three desktop E2E jobs. Downloaded reports confirm clean checkouts
and the same WebView versions listed above. Added checks cover source access
details, literal rendering of account labels, unavailable OAuth methods and
the recipient shown before Run. Native pickers and agents remain synthetic.
This records the desktop jobs; the overall CI outcome is tracked in Beads.
