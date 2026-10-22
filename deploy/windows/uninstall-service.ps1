<#
.SYNOPSIS
    Remove the bot-cooker Windows service.

.DESCRIPTION
    Stops and deletes the service and its firewall rule and removes the
    installed binary. The data directory (config with the master key, log) is
    kept unless -Purge is given.
#>
[CmdletBinding()]
param(
    [string]$ServiceName = "bot-cooker",
    [string]$InstallDir = (Join-Path $env:ProgramFiles "bot-cooker"),
    [string]$DataDir = (Join-Path $env:ProgramData "bot-cooker"),
    [switch]$Purge
)

$ErrorActionPreference = "Stop"

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($service) {
    if ($service.Status -ne "Stopped") {
        Stop-Service -Name $ServiceName -Force
        $service.WaitForStatus("Stopped", [TimeSpan]::FromSeconds(30))
    }
    & sc.exe delete $ServiceName | Out-Null
    Write-Host "Service $ServiceName removed."
}
Get-NetFirewallRule -DisplayName "bot-cooker" -ErrorAction SilentlyContinue | Remove-NetFirewallRule
if (Test-Path $InstallDir) { Remove-Item -Recurse -Force $InstallDir }
if ($Purge -and (Test-Path $DataDir)) {
    Remove-Item -Recurse -Force $DataDir
    Write-Host "Removed $DataDir."
} else {
    Write-Host "Kept $DataDir (config and log). Use -Purge to delete it."
}
