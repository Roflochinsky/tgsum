param(
    [Parameter(Mandatory)][int]$ProcessId,
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][long]$WindowHandle,
    [Parameter(Mandatory)][ValidateSet('chat', 'menu', 'settings', 'format',
        'folder', 'progress', 'done', 'cancel')][string]$Stage,
    [switch]$Observe,
    [switch]$Worker
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'TelegramUiaProbe.psm1') -Force

function Invoke-Observation {
    if ($ProcessId -le 0 -or $WindowHandle -le 0 -or
        -not [IO.Path]::IsPathRooted($Executable) -or
        -not [IO.File]::Exists($Executable)) { throw 'invalid_target' }
    $selectedPath = [IO.Path]::GetFullPath($Executable)
    $target = Get-Process -Id $ProcessId -ErrorAction Stop
    $start = $target.StartTime.ToUniversalTime().Ticks
    if (-not [string]::Equals($target.Path, $selectedPath,
            [StringComparison]::OrdinalIgnoreCase)) { throw 'executable_mismatch' }
    if ($target.SessionId -ne [Diagnostics.Process]::GetCurrentProcess().SessionId) {
        throw 'different_session'
    }
    $cim = Get-CimInstance Win32_Process -Filter "ProcessId = $ProcessId"
    if ($null -eq $cim) { throw 'process_changed' }
    $owner = Invoke-CimMethod -InputObject $cim -MethodName GetOwner
    $actualUser = "$($owner.Domain)\$($owner.User)"
    if ($owner.ReturnValue -ne 0 -or -not [string]::Equals($actualUser,
            [Security.Principal.WindowsIdentity]::GetCurrent().Name,
            [StringComparison]::OrdinalIgnoreCase)) { throw 'different_user' }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TgsumProbeWindow {
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
}
'@
    $hwnd = [IntPtr]$WindowHandle
    [uint32]$windowPid = 0
    if (-not [TgsumProbeWindow]::IsWindow($hwnd) -or
        [TgsumProbeWindow]::GetAncestor($hwnd, 2) -ne $hwnd -or
        [TgsumProbeWindow]::GetWindowThreadProcessId($hwnd, [ref]$windowPid) -eq 0 -or
        $windowPid -ne $ProcessId) { throw 'window_mismatch' }
    $reader = Get-TgsumUiaReader -ProcessId $ProcessId
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    if ($null -eq $root -or $root.Current.ProcessId -ne $ProcessId) {
        throw 'uia_root_mismatch'
    }
    $tree = Get-TgsumUiaProbeTree -Root $root -Reader $reader -ProcessId $ProcessId
    $again = Get-Process -Id $ProcessId -ErrorAction Stop
    if ($again.StartTime.ToUniversalTime().Ticks -ne $start -or
        -not [string]::Equals($again.Path, $selectedPath,
            [StringComparison]::OrdinalIgnoreCase) -or
        -not [TgsumProbeWindow]::IsWindow($hwnd)) { throw 'process_changed' }
    return @{ schema_version = 1; stage = $Stage; pid = $ProcessId;
        application_version = 'unknown'; nodes = $tree.nodes;
        truncated = $tree.truncated }
}

if (-not $Observe) {
    '{"error":"observe_required"}'
    exit 1
}

if ($Worker) {
    try {
        $result = Invoke-Observation
        @{ ok = $result } | ConvertTo-Json -Depth 12 -Compress
        exit 0
    } catch {
        # Provider errors can contain user data. Never print exception text.
        '{"error":"probe_failed"}'
        exit 1
    }
}

$outPath = [IO.Path]::GetTempFileName()
$errPath = [IO.Path]::GetTempFileName()
try {
    $shell = Join-Path $PSHOME 'powershell.exe'
    $arguments = '-NoProfile -NonInteractive -MTA -File "{0}" -ProcessId {1} -Executable "{2}" -WindowHandle {3} -Stage {4} -Observe -Worker' -f
        $PSCommandPath, $ProcessId, $Executable, $WindowHandle, $Stage
    $child = Start-Process -FilePath $shell -ArgumentList $arguments -PassThru `
        -RedirectStandardOutput $outPath -RedirectStandardError $errPath
    if (-not $child.WaitForExit(30000)) {
        Stop-Process -InputObject $child -Force -ErrorAction SilentlyContinue
        '{"error":"probe_timeout"}'
        exit 1
    }
    if ($child.ExitCode -ne 0) {
        '{"error":"probe_failed"}'
        exit 1
    }
    $body = [IO.File]::ReadAllText($outPath)
    $report = $body | ConvertFrom-Json
    if ($null -eq $report.ok -or $report.ok.schema_version -ne 1) {
        '{"error":"probe_failed"}'
        exit 1
    }
    $body.Trim()
} catch {
    '{"error":"probe_failed"}'
    exit 1
} finally {
    Remove-Item $outPath, $errPath -ErrorAction SilentlyContinue
}
