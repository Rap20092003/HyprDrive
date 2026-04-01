#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Install or uninstall the HyprDrive Helper as a Windows service.

.DESCRIPTION
    The HyprDrive Helper runs with elevated privileges to access NTFS MFT
    and USN journal data. It communicates with the daemon via named pipe IPC.

.PARAMETER BinaryPath
    Path to the helper binary. Defaults to the release build output.

.PARAMETER Uninstall
    Remove the service instead of installing it.

.EXAMPLE
    # Install (from the helper directory)
    .\install-service.ps1

    # Install with custom binary path
    .\install-service.ps1 -BinaryPath "C:\Program Files\HyprDrive\hyprdrive-helper-windows.exe"

    # Uninstall
    .\install-service.ps1 -Uninstall
#>
param(
    [string]$BinaryPath = "$PSScriptRoot\..\..\target\release\hyprdrive-helper-windows.exe",
    [switch]$Uninstall
)

$ServiceName = "HyprDriveHelper"
$DisplayName = "HyprDrive Filesystem Helper"
$Description = "Privileged helper for NTFS MFT and USN journal access. Communicates with HyprDrive daemon via named pipe."

if ($Uninstall) {
    Write-Host "Stopping service '$ServiceName'..." -ForegroundColor Yellow
    sc.exe stop $ServiceName 2>$null
    Start-Sleep -Seconds 2
    Write-Host "Removing service '$ServiceName'..." -ForegroundColor Yellow
    sc.exe delete $ServiceName
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Service '$ServiceName' removed successfully." -ForegroundColor Green
    } else {
        Write-Host "Failed to remove service (may not exist)." -ForegroundColor Red
    }
    exit $LASTEXITCODE
}

# Resolve binary path
try {
    $ResolvedPath = (Resolve-Path $BinaryPath -ErrorAction Stop).Path
} catch {
    Write-Host "Binary not found at: $BinaryPath" -ForegroundColor Red
    Write-Host "Build it first: cargo build -p hyprdrive-helper-windows --release" -ForegroundColor Yellow
    exit 1
}

Write-Host "Installing service '$ServiceName'..." -ForegroundColor Cyan
Write-Host "  Binary: $ResolvedPath" -ForegroundColor Gray

# Create the service (manual start — daemon starts it on-demand)
sc.exe create $ServiceName binPath= "`"$ResolvedPath`"" start= demand DisplayName= "$DisplayName"

if ($LASTEXITCODE -ne 0) {
    Write-Host "Failed to create service." -ForegroundColor Red
    exit $LASTEXITCODE
}

# Set description
sc.exe description $ServiceName "$Description"

# Configure failure recovery: restart after 5s, 10s, 30s (reset counter daily)
sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/10000/restart/30000

Write-Host ""
Write-Host "Service '$ServiceName' installed successfully." -ForegroundColor Green
Write-Host ""
Write-Host "Commands:" -ForegroundColor Cyan
Write-Host "  Start:   sc.exe start $ServiceName"
Write-Host "  Stop:    sc.exe stop $ServiceName"
Write-Host "  Status:  sc.exe query $ServiceName"
Write-Host "  Remove:  .\install-service.ps1 -Uninstall"
