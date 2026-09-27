param([Parameter(Mandatory)][string]$ReadyPath)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms

$form = New-Object System.Windows.Forms.Form
$form.Text = 'Private Customer Alice message 123'
$form.Name = 'session-private'
$form.Width = 320
$form.Height = 120
$button = New-Object System.Windows.Forms.Button
$button.Text = 'Another private label'
$button.Name = '123456789'
$button.Width = 220
$button.Height = 40
$button.Left = 20
$button.Top = 20
$form.Controls.Add($button)

$form.Show()
[System.Windows.Forms.Application]::DoEvents()
@{
    pid = $PID
    hwnd = $form.Handle.ToInt64()
    executable = (Get-Process -Id $PID).Path
} | ConvertTo-Json -Compress | Set-Content -LiteralPath $ReadyPath -Encoding UTF8
[System.Windows.Forms.Application]::Run($form)
