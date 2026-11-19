# Builds dist\Lull-windows-x64-setup.exe from target\release\lull.exe.
# Usage: pwsh packaging/windows/build-installer.ps1 -Version 0.3.3
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
if (-not (Test-Path $iscc)) {
    choco install innosetup -y --no-progress | Out-Null
}
$env:LULL_VERSION = $Version
& $iscc /Q (Join-Path $PSScriptRoot 'lull.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)" }
Get-Item (Join-Path $PSScriptRoot '..\..\dist\Lull-windows-x64-setup.exe')
