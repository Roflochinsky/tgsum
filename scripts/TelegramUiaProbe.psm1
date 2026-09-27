# Read-only UIA structure capture. The reader never requests Name, Value,
# TextPattern content, raw identifiers, or invokes a control pattern.

function Get-TgsumIdKind {
    param([AllowNull()][string]$Value)
    if ([string]::IsNullOrEmpty($Value)) { return 'empty' }
    if ($Value -match '^[0-9]+$') { return 'numeric' }
    return 'opaque'
}

function Get-TgsumUiaProbeTree {
    param(
        [Parameter(Mandatory)]$Root,
        [Parameter(Mandatory)][hashtable]$Reader,
        [Parameter(Mandatory)][int]$ProcessId
    )
    $allowedRoles = @('Window', 'Pane', 'Group', 'Button', 'Text', 'Edit',
        'List', 'ListItem', 'Menu', 'MenuItem', 'CheckBox', 'RadioButton',
        'ComboBox', 'Hyperlink', 'Tab', 'TabItem', 'Document', 'ProgressBar')
    $allowedPatterns = @('Invoke', 'Value', 'SelectionItem', 'Toggle',
        'ExpandCollapse', 'Window', 'Text', 'Scroll')
    $queue = New-Object 'System.Collections.Generic.Queue[object]'
    $queue.Enqueue(@{ Item = $Root; Path = @() })
    $nodes = New-Object 'System.Collections.Generic.List[object]'
    $truncated = $false
    while ($queue.Count -gt 0 -and $nodes.Count -lt 256) {
        $entry = $queue.Dequeue()
        $raw = & $Reader['Record'] $entry.Item
        if ([int]$raw.ProcessId -ne $ProcessId) {
            $truncated = $true
            continue
        }
        $role = [string]$raw.Role
        if ($role -notin $allowedRoles) { $role = 'other' }
        $patterns = @($raw.Patterns | Where-Object { $_ -in $allowedPatterns } |
            Select-Object -Unique)
        $children = @(& $Reader['Children'] $entry.Item)
        $count = $children.Count
        if ($count -gt 64 -or ($count -gt 0 -and $entry.Path.Count -ge 6)) {
            $truncated = $true
        }
        $nodes.Add(@{
            path = @($entry.Path)
            role = $role
            patterns = $patterns
            id_kind = Get-TgsumIdKind $raw.AutomationId
            enabled = [bool]$raw.Enabled
            offscreen = [bool]$raw.Offscreen
            children_at_least = [Math]::Min($count, 65)
        })
        if ($entry.Path.Count -ge 6) { continue }
        $limit = [Math]::Min(64, [Math]::Min($count, 256 - $nodes.Count - $queue.Count))
        if ($count -gt $limit) { $truncated = $true }
        for ($index = 0; $index -lt $limit; $index++) {
            $queue.Enqueue(@{ Item = $children[$index]; Path = @($entry.Path) + @($index) })
        }
    }
    if ($queue.Count -gt 0) { $truncated = $true }
    return @{ nodes = @($nodes.ToArray()); truncated = $truncated }
}

function Get-TgsumUiaReader {
    param([Parameter(Mandatory)][int]$ProcessId)
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
    $patternIds = @{
        Invoke = [System.Windows.Automation.InvokePattern]::Pattern.Id
        Value = [System.Windows.Automation.ValuePattern]::Pattern.Id
        SelectionItem = [System.Windows.Automation.SelectionItemPattern]::Pattern.Id
        Toggle = [System.Windows.Automation.TogglePattern]::Pattern.Id
        ExpandCollapse = [System.Windows.Automation.ExpandCollapsePattern]::Pattern.Id
        Window = [System.Windows.Automation.WindowPattern]::Pattern.Id
        Text = [System.Windows.Automation.TextPattern]::Pattern.Id
        Scroll = [System.Windows.Automation.ScrollPattern]::Pattern.Id
    }
    $record = {
        param($element)
        $current = $element.Current
        $pid = [int]$current.ProcessId
        if ($pid -ne $ProcessId) { return @{ ProcessId = $pid } }
        $available = @($element.GetSupportedPatterns() | ForEach-Object { $_.Id })
        $patterns = @($patternIds.Keys | Where-Object { $patternIds[$_] -in $available })
        $role = [string]$current.ControlType.ProgrammaticName
        if ($role.StartsWith('ControlType.')) { $role = $role.Substring(12) }
        return @{
            ProcessId = $pid
            Role = $role
            Patterns = $patterns
            AutomationId = [string]$current.AutomationId
            Enabled = [bool]$current.IsEnabled
            Offscreen = [bool]$current.IsOffscreen
        }
    }.GetNewClosure()
    $children = {
        param($element)
        $result = New-Object 'System.Collections.Generic.List[object]'
        $child = $walker.GetFirstChild($element)
        while ($null -ne $child -and $result.Count -lt 65) {
            $result.Add($child)
            $child = $walker.GetNextSibling($child)
        }
        return $result.ToArray()
    }.GetNewClosure()
    return @{ Record = $record; Children = $children }
}

Export-ModuleMember -Function Get-TgsumIdKind, Get-TgsumUiaProbeTree, Get-TgsumUiaReader
