param(
  [string[]]$BinaryPaths = @(
    "target/release/litho.exe",
    "target/release/litho-generator.exe",
    "target/release/litho-book.exe",
    "target/release/litho-qmd-cli.exe",
    "target/release/litho-qmd-mcp.exe"
  ),
  [string]$ExpectedToken = $env:LITHO_EXPECT_BUILD_TOKEN
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

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

  $versionOutput = (& $fullPath --version 2>&1 | Out-String).Trim()
  if ([string]::IsNullOrWhiteSpace($versionOutput)) {
    throw "No --version output for $fullPath"
  }

  $match = $tokenPattern.Match($versionOutput)
  if (-not $match.Success) {
    throw "Missing build_token marker in --version output for $fullPath. Output: $versionOutput"
  }

  $token = $match.Groups[1].Value
  $seen[$fullPath] = $token

  if ($ExpectedToken -and $ExpectedToken.Trim().Length -gt 0 -and $token -ne $ExpectedToken) {
    throw "Build token mismatch for $fullPath. Expected '$ExpectedToken', got '$token'."
  }
}

$unique = $seen.Values | Sort-Object -Unique
if ($unique.Count -ne 1) {
  throw "Build tokens are out of sync across binaries: $($unique -join ', ')"
}

Write-Host "Build token verification passed: $($unique[0])"
foreach ($entry in $seen.GetEnumerator() | Sort-Object Name) {
  Write-Host "  $($entry.Key) -> $($entry.Value)"
}
