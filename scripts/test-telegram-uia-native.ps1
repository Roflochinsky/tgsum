$ErrorActionPreference = 'Stop'

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

$shell = Join-Path $PSHOME 'powershell.exe'
$fixture = Join-Path $PSScriptRoot 'fixtures\uia-test-window.ps1'
$probe = Join-Path $PSScriptRoot 'telegram-uia-probe.ps1'
$ready = Join-Path $env:TEMP ("tgsum-uia-{0}.json" -f [guid]::NewGuid())
$fake = $null
try {
    $arguments = '-NoProfile -NonInteractive -File "{0}" -ReadyPath "{1}"' -f $fixture, $ready
    $fake = Start-Process -FilePath $shell -ArgumentList $arguments -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not [IO.File]::Exists($ready) -and [DateTime]::UtcNow -lt $deadline) {
        if ($fake.HasExited) { throw 'synthetic window stopped before readiness' }
        Start-Sleep -Milliseconds 100
    }
    Assert-True ([IO.File]::Exists($ready)) 'synthetic window did not become ready'
    $target = Get-Content -LiteralPath $ready -Raw | ConvertFrom-Json
    Assert-True ($target.pid -eq $fake.Id -and $target.hwnd -gt 0) 'synthetic target mismatch'

    $output = & $shell -NoProfile -NonInteractive -File $probe `
        -ProcessId $target.pid -Executable $target.executable `
        -WindowHandle $target.hwnd -Stage chat -Observe
    Assert-True ($LASTEXITCODE -eq 0) 'native UIA probe failed on synthetic window'
    $report = $output | ConvertFrom-Json
    Assert-True ($report.ok.schema_version -eq 1) 'probe schema mismatch'
    Assert-True ($report.ok.nodes.Count -ge 1) 'probe returned no UIA nodes'
    $body = $report | ConvertTo-Json -Depth 15 -Compress
    foreach ($private in @('Private Customer Alice', 'Another private label',
                           'session-private', '123456789')) {
        Assert-True (-not $body.Contains($private)) 'synthetic private text or raw ID leaked'
    }

    $wrongExecutable = & $shell -NoProfile -NonInteractive -File $probe `
        -ProcessId $target.pid -Executable $env:ComSpec `
        -WindowHandle $target.hwnd -Stage chat -Observe
    Assert-True ($LASTEXITCODE -ne 0 -and
        ($wrongExecutable | ConvertFrom-Json).error -eq 'probe_failed') `
        'wrong executable must fail closed'
    $wrongPid = & $shell -NoProfile -NonInteractive -File $probe `
        -ProcessId $PID -Executable $target.executable `
        -WindowHandle $target.hwnd -Stage chat -Observe
    Assert-True ($LASTEXITCODE -ne 0 -and
        ($wrongPid | ConvertFrom-Json).error -eq 'probe_failed') `
        'wrong PID must fail closed'
    $wrongWindow = & $shell -NoProfile -NonInteractive -File $probe `
        -ProcessId $target.pid -Executable $target.executable `
        -WindowHandle 1 -Stage chat -Observe
    Assert-True ($LASTEXITCODE -ne 0 -and
        ($wrongWindow | ConvertFrom-Json).error -eq 'probe_failed') `
        'wrong HWND must fail closed'

} finally {
    if ($null -ne $fake -and -not $fake.HasExited) {
        Stop-Process -InputObject $fake -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $ready -ErrorAction SilentlyContinue
}
'Windows UIA synthetic native-window transport: PASS'
exit 0
