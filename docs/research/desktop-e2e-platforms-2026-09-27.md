# Actual desktop E2E on Linux, Windows and macOS

Reviewed: **2026-09-27** for `tgsum-t8t.1`. This is tool selection and
implementation guidance; it does not qualify TGSUM on any additional OS.
No messenger, agent account, user profile or authenticated client was opened.

## Decision

Use an opt-in debug build of the **real Tauri application** with the embedded
`tauri-plugin-wdio-webdriver` server and a W3C WebDriver client. A bounded Python
standard-library HTTP client can use its documented standalone interface; Node
and `@wdio/tauri-service` are optional. Prefer the
maintained embedded implementation over a new application-specific eval runner.
Keep the existing synthetic agent backend; drive the application's rendered
controls and real IPC. Test-only Node tooling does not require changing the
static frontend build.

The Tauri WebDriver guide now recommends WebdriverIO and its embedded provider
on all three desktop systems. Its direct `tauri-driver` path still supports only
Windows/Linux: there is no upstream WKWebView driver for macOS. The distinction
matters: “macOS cannot run Tauri E2E” is now stale advice.
[Tauri guide, updated 2026-06-29](https://v2.tauri.app/develop/tests/webdriver/).

| Route | Linux | Windows | macOS | Decision for this task |
| --- | --- | --- | --- | --- |
| Direct `tauri-driver` + native driver | WebKitWebDriver | Matching Edge driver | Unsupported | Adds separate platform setup |
| Embedded `tauri-plugin-wdio-webdriver` | Real WebKitGTK | Real WebView2 | Real WKWebView | Preferred shared path |
| Tauri `eval` + private result IPC | API exists | API exists | API exists | Feasible fallback; needs its own lifecycle qualification |
| Browser-only frontend / mock runtime | Different runtime | Different runtime | Different runtime | Separate test layer, not desktop evidence |

## Versions and upstream evidence

TGSUM's inspected lockfile pins Tauri **2.11.6**, `tauri-runtime-wry` **2.11.4**
and Wry **0.55.1**. Its frontend is served from `src-tauri/ui`, its main window
is created by `create_main_window`, and existing Python smoke scripts drive
the actual Linux WebView through the WebKit inspector.
[Local Cargo lock](../../Cargo.lock),
[host](../../src-tauri/src/lib.rs),
[existing smoke](../../scripts/ui-onboarding-smoke.py).

The reviewed WebdriverIO source is
**`f4eb421a7831d3b3b84489b372ed24a7a517e0c2`**. Both the service and Rust plugin
declare **1.4.0**; the npm `latest` endpoint and crates.io stable endpoint returned
1.4.0 on the review date. The plugin declares Tauri `2.10.0` as its compatible
dependency requirement and Rust 1.77; this does not prove TGSUM's complete
dependency graph still meets its own MSRV after adding it.
[Service manifest](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/packages/tauri-service/package.json),
[Rust manifest](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/packages/tauri-plugin-webdriver/Cargo.toml),
[npm metadata](https://registry.npmjs.org/@wdio%2ftauri-service/latest),
[crate metadata](https://crates.io/api/v1/crates/tauri-plugin-wdio-webdriver).

**Pin the released Rust crate to `=1.4.0`.** Its release tag resolves to
**`aef40049a9c566e72de4ffd08e08197ff32386ed`**. Comparing that tag with the
reviewed main commit found identical plugin source, including the macOS
run-loop/async fixes discussed below (only the plugin's own Cargo.lock differed).
The tag's [CI run 34045843783](https://github.com/webdriverio/desktop-mobile/actions/runs/34045843783)
from 2026-09-06 also passed its actual standard Embedded E2E steps: Linux job
**101810344146**, Windows **101810345783**, macOS ARM **101810344624**.
These job steps were checked through both pages of the jobs API.
[Released macOS implementation](https://github.com/webdriverio/desktop-mobile/blob/aef40049a9c566e72de4ffd08e08197ff32386ed/packages/tauri-plugin-webdriver/src/platform/macos.rs),
[release jobs page 1](https://api.github.com/repos/webdriverio/desktop-mobile/actions/runs/34045843783/jobs?per_page=100),
[release jobs page 2](https://api.github.com/repos/webdriverio/desktop-mobile/actions/runs/34045843783/jobs?per_page=100&page=2).

Evidence goes beyond a configured matrix. Upstream
[CI run 35934926616](https://github.com/webdriverio/desktop-mobile/actions/runs/35934926616)
(2026-09-23, the same commit) completed successfully. The jobs API reported
the **Embedded E2E step successful** in all five suites (standard, window,
multiremote, standalone, deeplink) on Linux, Windows and macOS ARM. Standard
job IDs: Linux **107431344691**, Windows **107432757650**, macOS ARM
**107431975523**. This proves those upstream fixture runs, not TGSUM's flow or
macOS Intel E2E.
[Jobs API](https://api.github.com/repos/webdriverio/desktop-mobile/actions/runs/35934926616/jobs?per_page=100),
[pinned workflow](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/.github/workflows/_ci-e2e-tauri-all-providers.reusable.yml).

## Why the embedded driver is preferable to a new eval runner

The plugin executes against each platform's native WebView. Its desktop HTTP
listener binds loopback. It supports a standalone W3C client, so the additional
`tauri-plugin-wdio` command-mocking plugin is unnecessary for these tests.
Use explicit `driverProvider: 'embedded'` if choosing `@wdio/tauri-service`;
do not depend on provider auto-detection or mutable defaults. Pin packages and
commit the test-tool lockfile.
[Plugin implementation](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/packages/tauri-plugin-webdriver/src/lib.rs),
[server](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/packages/tauri-plugin-webdriver/src/server/mod.rs),
[standalone usage](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/packages/tauri-plugin-webdriver/README.md).

Tauri 2.11.6 supports initialization scripts, `on_page_load` with Started/Finished,
and `eval`/`eval_with_callback`. Thus a compiled-in test script can manipulate
the real DOM and report through a private Rust command without a remote inspector.
However, `eval` returning successfully is not proof that asynchronous steps
passed; `eval_with_callback` also ignores exceptions due to a Windows limitation.
Such a harness needs explicit JS catch/rejection reporting, readiness predicates,
one-run correlation, a native watchdog and a nonzero failure exit.
[Builder API](https://docs.rs/tauri/2.11.6/tauri/webview/struct.WebviewWindowBuilder.html),
[eval API](https://docs.rs/tauri/2.11.6/tauri/webview/struct.WebviewWindow.html#method.eval_with_callback).

The reviewed embedded plugin already handles an additional macOS CI problem:
its early main-run-loop timer keeps WebKit resource/IPC work moving when no
display activity wakes the application. It also has a native async result
channel and handling for reclaimed WebKit completions. These are concrete
maintenance costs that a bare `on_page_load` + `eval` design would inherit.
[Pinned macOS implementation](https://github.com/webdriverio/desktop-mobile/blob/f4eb421a7831d3b3b84489b372ed24a7a517e0c2/packages/tauri-plugin-webdriver/src/platform/macos.rs).

## Required isolation in TGSUM

These are recommended implementation boundaries, not existing guarantees:

1. Compile registration and test commands only under an explicit E2E feature
   **and** debug assertions; require explicit test invocation at runtime.
   A normal debug launch must not start an automation listener.
2. Create a fresh owned temporary root; inject `ProjectStore` under it before
   normal setup. Store fixture exports, generated output and reports there.
   Refuse a nonempty store or a non-synthetic analysis catalog before mutation.
3. Give Linux/Windows WebView a fresh data directory. Use `incognito(true)` for
   the smoke WebView, especially macOS, and verify the onboarding/localStorage
   starts empty. Project persistence remains real in the temporary Project root.
4. Keep production IPC and UI event handlers. Substitute only the native
   picker response with predeclared synthetic paths, and the already supported
   fake agent backend. Do not intercept every `invoke` and manufacture results.
5. Disable test-run desktop-entry/opener side effects. Never discover installed
   messenger or agent clients, read their profiles, or select real credentials.
   A timeout kills only child processes owned by this test invocation.

**macOS storage is a separate boundary:** `data_directory` does not select a
WKWebView storage directory. Tauri exposes `data_store_identifier` for macOS 14+
and iOS 17+, but that is a persistent store. In locked Wry 0.55.1,
`incognito=true` explicitly chooses `WKWebsiteDataStore::nonPersistentDataStore`;
otherwise an absent/unsupported custom identifier falls back to the default
store. Setting Linux XDG variables alone cannot isolate a macOS WebView.
[Tauri store identifier](https://docs.rs/tauri/2.11.6/tauri/webview/struct.WebviewWindowBuilder.html#method.data_store_identifier),
[Wry 0.55.1 store selection](https://docs.rs/crate/wry/0.55.1/source/src/wkwebview/mod.rs).

## Concrete smoke and CI contract

One suite should create Projects through visible controls, import synthetic
full and per-chat Telegram JSON through the source flow, choose chat/topic/date
scope, save privacy choices, inspect the generated Review, explicitly run each
fake agent, read the rendered result and check exported files. Assertions should
also cover a rejected malformed/wrong-scope archive and stale Review invalidation.
Use real production IPC for setup only when the operation is outside the flow
being claimed. Keep source ingestion itself in the tested UI path.

Native file dialogs are outside the HTML document. Returning an owned fixture
path at their debug seam can qualify the following UI/IPC flow, but must be
reported as **picker simulated**. Existing Linux GTK/AT-SPI picker checks remain
distinct. DOM interactions and screenshots do not prove OS keyboard navigation,
accessibility permissions, installer behavior or Telegram automation.

| Runner | Execution | Evidence to retain |
| --- | --- | --- |
| Linux | Real app under Xvfb and required GTK/WebKit libraries | OS/image, architecture, WebKit version, assertions, screenshot |
| Windows | Real app directly on hosted runner with WebView2 | OS/image, architecture, WebView2 version, assertions, screenshot |
| macOS | Real app directly on hosted runner; embedded WKWebView driver | OS/image, architecture, WebKit version, assertions, screenshot |

The Tauri CI guide specifies Xvfb for Linux and direct execution on Windows;
the pinned upstream embedded workflow also runs directly on macOS. GitHub's
runner labels/images change, so report the actual image/version and arch from
each run; a three-OS matrix does not establish every OS release or architecture.
[Tauri CI guide](https://v2.tauri.app/develop/tests/webdriver/ci/),
[GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
[runner images](https://github.com/actions/runner-images).

Use bounded process/readiness/script timeouts, a nonzero exit for failed or
missing assertions, and `always()` report upload. Save the commit, Cargo and
test-tool versions, `tauri::webview_version()` when available, renderer UA,
assertion results and screenshots from the same actual app run. Upstream success
is sufficient reason to implement this path; each TGSUM OS row remains
**unverified until its own renderer run passes**. An unavailable native session
must fail or remain explicitly unverified, never silently become a mock test.

## Local implementation finding: wait for page readiness

The first TGSUM Linux attempt with the pinned plugin timed out evaluating the
session's user-agent expression and the first `execute/sync` request, while its
native screenshot endpoint successfully captured the rendered window. The host
had WebKitGTK/JavaScriptCore **2.52.6** and GLib **2.88.3**. Starting the WebDriver
session only after the real main window's **`PageLoadEvent::Finished`**, signalled
through a run-correlated native readiness file, let the same plugin complete
onboarding, Project creation, full JSON ingestion, chat/topic/date scope, Privacy
Review and Export only without an inspector connection. The subsequent fake-agent
flow was still running when this finding was recorded; this is not a completed
three-platform qualification.

Therefore wait for both server availability **and actual page completion**
before creating the session. An open HTTP port or registered window label is
insufficient readiness evidence for this harness. Keep a bounded timeout and
validate the readiness file's run nonce. The internal WebKit cause remains
unproven; do not describe this as a diagnosed GLib-version incompatibility.

The reviewed plugin waits for a window label and immediately evaluates a simple
user-agent IIFE. Its Linux eval and screenshot paths use the same
`MainContext::default().spawn_local` scheduling. Wry's own Linux `eval` additionally
buffers scripts during initial loading; direct plugin evaluation bypasses that
guard. These source facts are consistent with the observed startup race, without
proving its exact mechanism.
[Session creation](https://github.com/webdriverio/desktop-mobile/blob/aef40049a9c566e72de4ffd08e08197ff32386ed/packages/tauri-plugin-webdriver/src/server/handlers/session.rs),
[Linux eval and snapshot paths](https://github.com/webdriverio/desktop-mobile/blob/aef40049a9c566e72de4ffd08e08197ff32386ed/packages/tauri-plugin-webdriver/src/platform/linux.rs),
[Wry 0.55.1 Linux implementation](https://docs.rs/crate/wry/0.55.1/source/src/webkitgtk/mod.rs).
