# Inputs are generated as single-quoted literals by login.rs, never evaluated.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$sid = $identity.User.Value
if ($ExpectedSid -and $sid -ne $ExpectedSid) {
    throw 'Start at login must be configured using the same Windows account. A different administrator account is not supported.'
}
$name = "Mihomo Client-$sid"
$arguments = '--autostart --scheduled-launch'

if ($Mode -eq 'cleanup' -or $Mode -eq 'has-tasks') {
    # Only remove this product's tasks whose executable is the one being removed.
    $tasks = @(Get-ScheduledTask | Where-Object {
        $_.TaskName -like 'Mihomo Client-S-1-*' -and
        $_.Actions.Count -eq 1 -and $_.Actions[0].Execute -eq $Executable
    })
    if ($Mode -eq 'has-tasks') { ($tasks.Count -gt 0).ToString().ToLowerInvariant() }
    else { $tasks | Unregister-ScheduledTask -Confirm:$false }
    exit 0
}
if ($Mode -eq 'status') {
    # Query the root task directly without loading the ScheduledTasks module.
    $scheduler = New-Object -ComObject 'Schedule.Service'
    $scheduler.Connect()
    $folder = $scheduler.GetFolder('\')
    $task = $null
    try {
        $task = $folder.GetTask($name)
    } catch {
        # HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)
        if ($_.Exception.GetBaseException().HResult -ne 0x80070002) { throw }
    }

    $enabled = $false
    $repair = $false
    if ($null -ne $task) {
        $enabled = $task.Enabled
        $definition = $task.Definition
        $principal = $definition.Principal
        $taskSid = $principal.UserId
        if ($taskSid -notlike 'S-1-*') {
            try {
                $taskSid = (New-Object Security.Principal.NTAccount($taskSid)).Translate([Security.Principal.SecurityIdentifier]).Value
            } catch { $taskSid = '' }
        }

        $actions = $definition.Actions
        # RunLevel 1 is highest, LogonType 3 is interactive token, and action type 0 is executable.
        $repair = $taskSid -ne $sid -or $principal.RunLevel -ne 1 -or
            $principal.LogonType -ne 3 -or $actions.Count -ne 1
        if (-not $repair) {
            $action = $actions.Item(1)
            $repair = $action.Type -ne 0 -or $action.Path -ne $Executable -or $action.Arguments -ne $arguments
        }
    }
    @{ enabled = $enabled; needsRepair = $repair; supported = $true;
       detail = $(if ($repair) { 'The login task needs updating for this installation. Click Repair.' }
                  else { 'Starts in the background and restores the previous core state. Windows asks for administrator approval when changing this option.' })
    } | ConvertTo-Json -Compress
} elseif ($Mode -eq 'enable') {
    $principal = New-ScheduledTaskPrincipal -UserId $sid -LogonType Interactive -RunLevel Highest
    $action = New-ScheduledTaskAction -Execute $Executable -Argument $arguments -WorkingDirectory (Split-Path -LiteralPath $Executable)
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $sid
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable
    Register-ScheduledTask -TaskName $name -Action $action -Trigger $trigger -Principal $principal `
        -Settings $settings -Description 'Mihomo Client: restore the previous core state at user login.' -Force | Out-Null
    # Let the unelevated launcher query/run the task, without granting write access.
    $scheduler = New-Object -ComObject 'Schedule.Service'
    $scheduler.Connect()
    $scheduler.GetFolder('\').GetTask($name).SetSecurityDescriptor("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGX;;;$sid)", 0)
} elseif ($Mode -eq 'disable') {
    $task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    if ($task) { Unregister-ScheduledTask -TaskName $name -Confirm:$false }
} elseif ($Mode -eq 'run') {
    $task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    if (-not $task -or $task.State -eq 'Disabled') { throw 'The login task is missing or disabled.' }
    if ($task.Actions[0].Execute -ne $Executable) { throw 'Repair the login task in Settings before using it.' }
    Start-ScheduledTask -TaskName $name
} elseif ($Mode -eq 'sid') {
    $sid
} else {
    throw 'Unknown login task operation.'
}
