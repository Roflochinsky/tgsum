# Windows Telegram Desktop: controlled UIA observation

`scripts/telegram-uia-probe.ps1` is a **read-only diagnostic** for
`tgsum-i1w.3.1` and the user-controlled qualification `tgsum-t8t.24`. It
does not launch Telegram, select a chat, invoke a control, export data, or
enable the Windows Bridge driver. Do not run it against a real Telegram process
without the user's control. Automatic export stays disabled.

The operator supplies an explicit process ID, exact executable path, top-level
window handle and fixed stage. The probe checks the executable, process owner,
Windows session and window PID before reading the UIA subtree. It reports only
allowlisted roles and pattern names, identifier **shape**, enabled/offscreen
flags and tree paths. It does not request control names, text, values, raw IDs,
screenshots, credentials or process memory. A different-process child is
excluded and marks the report incomplete. UIA calls run in a child process
with a 30-second outer timeout; any provider error is reported generically.
The tree is limited to 256 nodes, depth 6 and 64 children per node.

On a user-controlled Windows session with a disposable test chat, navigate
Telegram manually to one stage, determine the chosen process/window handle
locally, then run one observation:

```powershell
powershell.exe -NoProfile -File .\scripts\telegram-uia-probe.ps1 `
  -ProcessId <TELEGRAM_PID> -Executable 'C:\path\Telegram.exe' `
  -WindowHandle <DECIMAL_HWND> -Stage settings -Observe `
  > "$env:TEMP\tgsum-telegram-uia-settings.json"
```

The accepted stages are `chat`, `menu`, `settings`, `format`, `folder`,
`progress`, `done` and `cancel`. Keep local reports private and review them
before sharing. A report cannot prove that an `Invoke` pattern works, that the
selected account/chat has a native ID available, or that Done belongs to a new
export. In particular, Qt can expose a false-positive `Invoke` for FlatLabel;
see the [Windows source review](../research/telegram-windows-bridge-2026-09-27.md).
An unknown native identity or missing format/path/completion signal remains a
no-go. The probe records `application_version: unknown`; the UIA tree cannot
qualify the installed client version.

The test uses only fake nodes and an invalid CLI target, never Telegram:

```powershell
powershell.exe -NoProfile -File .\scripts\test-telegram-uia-probe.ps1
```

The Windows `test` CI job runs these fake-tree checks and a second integration
check that creates its own WinForms window, calls the real UIA transport against
that window, verifies the report omits its private-looking labels and raw ID,
and rejects the wrong executable, PID and HWND. It closes only its synthetic
process. Neither check launches Telegram or proves Telegram-specific selectors.
The UIA binding uses
[Microsoft's AutomationElement](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement?view=windowsdesktop-10.0)
and [RawViewWalker](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.treewalker.rawviewwalker?view=windowsdesktop-10.0)
to observe an existing window. Its presence does not change the
`telegram_desktop_ui` registry support decision.
