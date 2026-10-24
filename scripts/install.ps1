[CmdletBinding()]
param(
    [switch]$Uninstall,
    [switch]$Purge,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Wirehaus')
)

$ErrorActionPreference = 'Stop'
$source = Split-Path -Parent $MyInvocation.MyCommand.Path
$agent = Join-Path $InstallDir 'wirehaus-agent.exe'
$dataDir = Join-Path $env:LOCALAPPDATA 'Wirehaus'
$stateFile = Join-Path $dataDir 'state\state'
$files = @('wirehaus-agent.exe', 'wirehaus-agentd.exe')

function Invoke-Agent {
    param([string[]]$Arguments)
    & $agent @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "wirehaus-agent $($Arguments -join ' ') failed with code $LASTEXITCODE"
    }
}

function Test-Package {
    $sums = Join-Path $source 'SHA256SUMS'
    if (-not (Test-Path $sums)) {
        throw 'SHA256SUMS is missing from the package.'
    }
    foreach ($line in Get-Content $sums) {
        $hash, $name = $line -split '\s+', 2
        $path = Join-Path $source ($name.Trim() -replace '/', '\')
        if (-not (Test-Path $path)) {
            throw "$name is listed in SHA256SUMS but missing."
        }
        if ((Get-FileHash -Algorithm SHA256 $path).Hash.ToLower() -ne $hash.ToLower()) {
            throw "$name does not match SHA256SUMS."
        }
    }
}

if ($Uninstall) {
    if (Test-Path $agent) {
        Invoke-Agent @('service', 'uninstall')
    }
    Remove-Item -Recurse -Force $InstallDir -ErrorAction SilentlyContinue
    if ($Purge) {
        Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
    }
    Write-Host 'Wirehaus has been removed.'
    exit 0
}

Get-ChildItem -Recurse -File $source | Unblock-File
Test-Package

if (Test-Path $agent) {
    Invoke-Agent @('service', 'uninstall')
}
New-Item -ItemType Directory -Force $InstallDir | Out-Null
foreach ($file in $files) {
    Copy-Item -Force (Join-Path $source $file) $InstallDir
}
$viewer = Join-Path $InstallDir 'viewer'
Remove-Item -Recurse -Force $viewer -ErrorAction SilentlyContinue
Copy-Item -Recurse -Force (Join-Path $source 'viewer') $viewer

$firstRun = -not (Test-Path $stateFile)
if ($firstRun) {
    Invoke-Agent @('init')
    Write-Host ''
    Write-Host 'Save the secret above in your authenticator app now: it is only shown once.'
    Write-Host ''
}

Invoke-Agent @('service', 'install', '--viewer-dir', $viewer)
Invoke-Agent @('pairing')
Write-Host ''
Write-Host 'Open http://127.0.0.1:7878 on this computer, or reach it through your tunnel, and enter the pairing code.'
