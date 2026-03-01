<#
.SYNOPSIS
    Check and optionally configure Windows pagefile for Rust compilation.

.DESCRIPTION
    Rust compilation on large workspaces (codex-tui, litho-generator) can
    demand 3-4 GB VA per link.exe process. System-managed pagefiles grow
    too slowly, causing OOM. A fixed-size pagefile eliminates expansion
    latency during parallel linking.

    Run without -Fix to report current state. Run with -Fix to apply
    recommended settings (requires elevation).

.PARAMETER Fix
    Apply recommended pagefile settings (requires Administrator).

.PARAMETER Drive
    Drive letter for pagefile (default: C).

.PARAMETER InitialMB
    Initial pagefile size in MB (default: 8192 = 8 GB).

.PARAMETER MaximumMB
    Maximum pagefile size in MB (default: 24576 = 24 GB).
#>
param(
    [switch]$Fix,
    [string]$Drive = "C",
    [int]$InitialMB = 8192,
    [int]$MaximumMB = 24576
)

$ErrorActionPreference = "Stop"

# Report current state
Write-Host "=== System Memory ===" -ForegroundColor Cyan
$cs = Get-CimInstance Win32_ComputerSystem
$ramGB = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
Write-Host "  Physical RAM: $ramGB GB"
Write-Host "  Auto-managed pagefile: $($cs.AutomaticManagedPagefile)"

Write-Host "`n=== Current Pagefile Usage ===" -ForegroundColor Cyan
Get-CimInstance Win32_PageFileUsage | ForEach-Object {
    Write-Host "  $($_.Name)"
    Write-Host "    Allocated: $($_.AllocatedBaseSize) MB"
    Write-Host "    Current:   $($_.CurrentUsage) MB"
    Write-Host "    Peak:      $($_.PeakUsage) MB"
}

Write-Host "`n=== Pagefile Settings ===" -ForegroundColor Cyan
$settings = Get-CimInstance Win32_PageFileSetting
if ($settings) {
    $settings | ForEach-Object {
        $minLabel = if ($_.InitialSize -eq 0) { "system-managed" } else { "$($_.InitialSize) MB" }
        $maxLabel = if ($_.MaximumSize -eq 0) { "system-managed" } else { "$($_.MaximumSize) MB" }
        Write-Host "  $($_.Name): Min=$minLabel, Max=$maxLabel"
    }
} else {
    Write-Host "  (system-managed, no explicit settings)"
}

# Recommendations
Write-Host "`n=== Recommendation ===" -ForegroundColor Yellow
$minRecommended = [math]::Max(8192, [math]::Ceiling($ramGB * 0.5) * 1024)
$maxRecommended = [math]::Max(24576, [math]::Ceiling($ramGB * 1.5) * 1024)
Write-Host "  For $ramGB GB RAM with Rust compilation:"
Write-Host "  Suggested: Initial=$minRecommended MB, Maximum=$maxRecommended MB"
Write-Host "  Drive: $Drive (prefer NVMe with most free space)"

$currentPeak = ($settings | ForEach-Object { $_.MaximumSize }) | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum
if ($currentPeak -and $currentPeak -ge $maxRecommended) {
    Write-Host "`n  Pagefile looks adequate. No changes needed." -ForegroundColor Green
    return
}

if (-not $Fix) {
    Write-Host "`n  Run with -Fix to apply recommended settings (requires elevation)." -ForegroundColor DarkGray
    return
}

# Apply fix — requires elevation
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)
if (-not $isAdmin) {
    throw "Administrator privileges required. Run PowerShell as Administrator."
}

Write-Host "`n=== Applying Pagefile Settings ===" -ForegroundColor Green
Write-Host "  Disabling automatic management..."
$cs2 = Get-WmiObject Win32_ComputerSystem -EnableAllPrivileges
$cs2.AutomaticManagedPagefile = $false
$cs2.Put() | Out-Null

Write-Host "  Removing existing explicit settings..."
Get-WmiObject Win32_PageFileSetting | ForEach-Object { $_.Delete() }

Write-Host "  Creating ${Drive}:\pagefile.sys (Init=${InitialMB}MB, Max=${MaximumMB}MB)..."
$pf = ([WmiClass]"Win32_PageFileSetting").CreateInstance()
$pf.Name = "${Drive}:\pagefile.sys"
$pf.InitialSize = $InitialMB
$pf.MaximumSize = $MaximumMB
$pf.Put() | Out-Null

Write-Host "`n  Done. REBOOT REQUIRED for changes to take effect." -ForegroundColor Yellow
