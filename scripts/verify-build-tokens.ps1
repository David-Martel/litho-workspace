param(
  [string[]]$Crates = @(
    "litho-cli",
    "litho-generator",
    "litho-book",
    "litho-qmd-cli",
    "litho-qmd-mcp"
  ),
  [string[]]$BinaryPaths = @(),
  [switch]$BuildRelease,
  [switch]$CleanBeforeBuild,
  [string]$ExpectedToken = $env:LITHO_EXPECT_BUILD_TOKEN
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
. (Join-Path $repoRoot "scripts\litho-build-helpers.ps1")

if ($BuildRelease.IsPresent) {
  Build-LithoReleaseArtifacts `
    -Crates $Crates `
    -RepoRoot $repoRoot `
    -CleanPublished:$CleanBeforeBuild `
    -CleanCargoRelease:$CleanBeforeBuild `
    -RefreshBuildStamp `
    -UseIsolatedTargetDir | Out-Null
}

if ($BinaryPaths.Count -eq 0) {
  $BinaryPaths = @($Crates | ForEach-Object {
      Get-LithoPublishedBinaryPath -Crate $_ -RepoRoot $repoRoot
    })
}

$seen = @{}
$tokenPattern = [regex]"build_token=([^\)\s]+)"

foreach ($binary in $BinaryPaths) {
  $fullPath = if ([System.IO.Path]::IsPathRooted($binary)) {
    $binary
  } else {
    Join-Path $repoRoot $binary
  }

  if (-not (Test-Path $fullPath)) {
    throw "Binary not found: $fullPath"
  }

  $artifactName = [System.IO.Path]::GetFileName($fullPath)
  $crate = Get-LithoCrateFromArtifactName -ArtifactName $artifactName
  $expectedVersion = Get-LithoManifestVersion -Crate $crate -RepoRoot $repoRoot

  $versionOutput = (& $fullPath --version 2>&1 | Out-String).Trim()
  if ([string]::IsNullOrWhiteSpace($versionOutput)) {
    throw "No --version output for $fullPath"
  }

  if ($versionOutput -notmatch [regex]::Escape($expectedVersion)) {
    throw "Version output for $fullPath does not contain manifest version '$expectedVersion'. Output: $versionOutput"
  }

  $match = $tokenPattern.Match($versionOutput)
  if (-not $match.Success) {
    throw "Missing build_token marker in --version output for $fullPath. Output: $versionOutput"
  }

  $token = $match.Groups[1].Value
  $seen[$fullPath] = [PSCustomObject]@{
    Crate = $crate
    Version = $expectedVersion
    Token = $token
  }

  if ($ExpectedToken -and $ExpectedToken.Trim().Length -gt 0 -and $token -ne $ExpectedToken) {
    throw "Build token mismatch for $fullPath. Expected '$ExpectedToken', got '$token'."
  }
}

$unique = @($seen.Values.Token | Sort-Object -Unique)
if ($unique.Count -ne 1) {
  throw "Build tokens are out of sync across binaries: $($unique -join ', ')"
}

Write-Host "Build token verification passed: $($unique[0])"
foreach ($entry in $seen.GetEnumerator() | Sort-Object Name) {
  Write-Host "  $($entry.Key) -> crate=$($entry.Value.Crate) version=$($entry.Value.Version) token=$($entry.Value.Token)"
}
