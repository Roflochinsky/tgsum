$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'TelegramUiaProbe.psm1') -Force

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

$private = 'Private Customer Alice message 123'
$root = @{ ProcessId = 42; Role = 'Window'; AutomationId = 'session-private';
    Patterns = @(); Enabled = $true; Offscreen = $false; Children = @(
        @{ ProcessId = 42; Role = 'Button'; AutomationId = '123456789';
            Patterns = @('Invoke', $private); Enabled = $true; Offscreen = $false;
            Children = @() },
        @{ ProcessId = 42; Role = $private; AutomationId = 'chat-private';
            Patterns = @(); Enabled = $true; Offscreen = $false;
            Children = @() }
    ) }
$reader = @{
    Record = { param($item) $item }
    Children = { param($item) $item.Children }
}
$report = Get-TgsumUiaProbeTree -Root $root -Reader $reader -ProcessId 42
$json = $report | ConvertTo-Json -Depth 12 -Compress
Assert-True ($report.nodes.Count -eq 3) 'expected three bounded nodes'
Assert-True ($report.nodes[1].patterns -contains 'Invoke') 'Invoke shape absent'
Assert-True ($report.nodes[1].id_kind -eq 'numeric') 'numeric id shape absent'
Assert-True ($report.nodes[2].role -eq 'other') 'unknown role must be masked'
foreach ($value in @($private, 'session-private', '123456789', 'chat-private')) {
    Assert-True (-not $json.Contains($value)) 'private data leaked to report'
}

$root.Children += @{ ProcessId = 43; Role = 'Text'; AutomationId = 'other-process';
    Patterns = @('Text'); Enabled = $true; Offscreen = $false; Children = @() }
$report = Get-TgsumUiaProbeTree -Root $root -Reader $reader -ProcessId 42
Assert-True ($report.nodes.Count -eq 3 -and $report.truncated) 'cross-process node must be refused'
Assert-True (-not (($report | ConvertTo-Json -Depth 12).Contains('other-process'))) 'cross-process id leaked'

$root.Children = @()
for ($i = 0; $i -lt 100; $i++) {
    $root.Children += @{ ProcessId = 42; Role = 'Text'; AutomationId = '';
        Patterns = @(); Enabled = $true; Offscreen = $false; Children = @() }
}
$report = Get-TgsumUiaProbeTree -Root $root -Reader $reader -ProcessId 42
Assert-True ($report.nodes.Count -eq 65 -and $report.truncated) 'wide tree bound failed'

$chain = @{ ProcessId = 42; Role = 'Text'; AutomationId = '';
    Patterns = @(); Enabled = $true; Offscreen = $false; Children = @() }
for ($i = 0; $i -lt 8; $i++) {
    $chain = @{ ProcessId = 42; Role = 'Pane'; AutomationId = '';
        Patterns = @(); Enabled = $true; Offscreen = $false; Children = @($chain) }
}
$report = Get-TgsumUiaProbeTree -Root $chain -Reader $reader -ProcessId 42
Assert-True ($report.nodes.Count -eq 7 -and $report.truncated) 'deep tree bound failed'

$shell = Join-Path $PSHOME 'powershell.exe'
$probe = Join-Path $PSScriptRoot 'telegram-uia-probe.ps1'
$refused = & $shell -NoProfile -NonInteractive -File $probe `
    -ProcessId 999999 -Executable 'C:\missing.exe' -WindowHandle 1 -Stage chat
Assert-True ($LASTEXITCODE -ne 0 -and $refused -eq '{"error":"observe_required"}') `
    'CLI must require explicit observation'

'Windows UIA probe synthetic checks: PASS'
