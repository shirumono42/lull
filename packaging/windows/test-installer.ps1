# Installs the built installer silently, checks what launchers need, uninstalls.
$ErrorActionPreference = 'Stop'
$setup = Join-Path $PSScriptRoot '..\..\dist\Lull-windows-x64-setup.exe'
$app = "$env:LOCALAPPDATA\Programs\Lull\Lull.exe"
$menu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Lull.lnk"
$startup = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\Lull.lnk"
$uninstKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{2D7B8AFF-E0BB-4F20-8F61-0294E8028B51}_is1'
function Check($ok, $what) { if (-not $ok) { throw "FAILED: $what" }; "ok: $what" }

$p = Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TASKS=startup' -PassThru -Wait
Check ($p.ExitCode -eq 0) "installer exit code 0 (got $($p.ExitCode))"
Check (Test-Path $app) "Lull.exe installed at $app"
Check (Test-Path $menu) 'Start menu shortcut'
$target = (New-Object -ComObject WScript.Shell).CreateShortcut($menu).TargetPath
Check ($target -eq $app) "Start menu shortcut points to Lull.exe ($target)"
Check (Test-Path $startup) 'sign-in shortcut (task selected)'
$reg = Get-ItemProperty $uninstKey
Check ($reg.DisplayName -eq 'Lull') "Settings > Apps entry ($($reg.DisplayName) $($reg.DisplayVersion))"

$uninst = $reg.UninstallString.Trim('"')
$u = Start-Process $uninst -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -PassThru -Wait
Start-Sleep 2
Check ($u.ExitCode -eq 0) 'uninstaller exit code 0'
Check (-not (Test-Path $app)) 'Lull.exe removed'
Check (-not (Test-Path $menu)) 'Start menu shortcut removed'
Check (-not (Test-Path $startup)) 'sign-in shortcut removed'
Check (-not (Test-Path $uninstKey)) 'Settings > Apps entry removed'
