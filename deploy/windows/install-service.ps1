<#
.SYNOPSIS
    Install bot-cooker as a Windows service.

.DESCRIPTION
    Copies bot-cooker.exe to the install directory, creates the data directory
    with a config (a fresh master key is generated on first install), registers
    a service running as LOCAL SERVICE with automatic restart, and starts it.
    Run from an elevated PowerShell in the unpacked release folder. Re-running
    upgrades the binary and keeps the existing config.

.EXAMPLE
    .\install-service.ps1 -Port 80 -OpenFirewall
#>
[CmdletBinding()]
param(
    [string]$ServiceName = "bot-cooker",
    [string]$InstallDir = (Join-Path $env:ProgramFiles "bot-cooker"),
    [string]$DataDir = (Join-Path $env:ProgramData "bot-cooker"),
    [int]$Port = 0,
    [switch]$OpenFirewall
)

$ErrorActionPreference = "Stop"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this script from an elevated (Administrator) PowerShell."
}

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = @((Join-Path $here "bot-cooker.exe"), (Join-Path $here "..\..\bot-cooker.exe")) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) { throw "bot-cooker.exe not found next to this script." }
$sample = @((Join-Path $here "bot-cooker.yaml"), (Join-Path $here "..\..\bot-cooker.yaml")) | Where-Object { Test-Path $_ } | Select-Object -First 1

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing -and $existing.Status -ne "Stopped") {
    Write-Host "Stopping $ServiceName for the upgrade..."
    Stop-Service -Name $ServiceName -Force
    $existing.WaitForStatus("Stopped", [TimeSpan]::FromSeconds(30))
}

New-Item -ItemType Directory -Force -Path $InstallDir, $DataDir | Out-Null
Copy-Item -Force $exe (Join-Path $InstallDir "bot-cooker.exe")

$config = Join-Path $DataDir "bot-cooker.yaml"
$log = Join-Path $DataDir "bot-cooker.log"
if (-not (Test-Path $config)) {
    if (-not $sample) { throw "bot-cooker.yaml not found next to this script." }
    $bytes = New-Object byte[] 48
    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $key = [Convert]::ToBase64String($bytes).TrimEnd("=").Replace("+", "-").Replace("/", "_")
    $text = (Get-Content -Raw $sample) -replace 'master_key: ""', "master_key: `"$key`""
    if ($Port -gt 0) { $text = $text -replace 'bind: "0\.0\.0\.0:8080"', "bind: `"0.0.0.0:$Port`"" }
    [IO.File]::WriteAllText($config, $text, (New-Object Text.UTF8Encoding($false)))
    Write-Host "Wrote $config with a new master key."
} elseif ($Port -gt 0) {
    Write-Warning "$config exists; -Port is ignored. Edit http.bind there instead."
}

# LOCAL SERVICE reads the config and writes the log; the key stays private.
& icacls $DataDir /inheritance:r /grant:r "*S-1-5-32-544:(OI)(CI)F" "*S-1-5-18:(OI)(CI)F" "*S-1-5-19:(OI)(CI)M" | Out-Null

$bin = "`"$InstallDir\bot-cooker.exe`" --service --service-name $ServiceName --config `"$config`" --log-file `"$log`""
# The command line contains quotes, so it never goes through sc.exe: Windows
# PowerShell 5.1 mangles embedded quotes in native-command arguments.
if ($existing) {
    $result = Get-CimInstance Win32_Service -Filter "Name='$ServiceName'" | Invoke-CimMethod -MethodName Change -Arguments @{ PathName = $bin }
    if ($result.ReturnValue -ne 0) { throw "updating the service failed ($($result.ReturnValue))" }
} else {
    New-Service -Name $ServiceName -BinaryPathName $bin -DisplayName "bot-cooker" -StartupType Automatic | Out-Null
}
& sc.exe config $ServiceName start= delayed-auto obj= "NT AUTHORITY\LocalService" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "sc.exe config failed ($LASTEXITCODE)" }
& sc.exe description $ServiceName "HTTP tarpit for unsolicited scanners" | Out-Null
& sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/30000/restart/60000 | Out-Null

if ($OpenFirewall) {
    $listen = (Select-String -Path $config -Pattern '^\s*bind:\s*"[^"]*:(\d+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    Get-NetFirewallRule -DisplayName "bot-cooker" -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -DisplayName "bot-cooker" -Direction Inbound -Action Allow -Protocol TCP -LocalPort $listen -Program "$InstallDir\bot-cooker.exe" | Out-Null
    Write-Host "Firewall: inbound TCP $listen allowed for bot-cooker.exe."
}

Start-Service -Name $ServiceName
(Get-Service -Name $ServiceName).WaitForStatus("Running", [TimeSpan]::FromSeconds(30))
Write-Host "$ServiceName is running. Config: $config  Log: $log"
