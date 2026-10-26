Set-StrictMode -Version Latest

function Get-LithoRepoReleaseDir {
  param(
    [string]$RepoRoot = (Get-Location).Path
  )

  $dir = Join-Path $RepoRoot "release"
  if (-not (Test-Path $dir)) {
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
  }
  return (Resolve-Path $dir).Path
}

function Get-LithoReleaseHarnessTargetDir {
  param(
    [string]$RepoRoot = (Get-Location).Path
  )

  $cacheDir = Join-Path $RepoRoot ".cache\cargo-target-release-harness"
  if (-not (Test-Path $cacheDir)) {
    New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
  }

  return (Resolve-Path $cacheDir).Path
}

function Get-LithoLocalSccacheDir {
  param(
    [string]$RepoRoot = (Get-Location).Path
  )

  $cacheDir = Join-Path $RepoRoot ".cache\sccache"
  if (-not (Test-Path $cacheDir)) {
    New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
  }

  return (Resolve-Path $cacheDir).Path
}

function Set-LithoSccacheEnvironment {
  param(
    [string]$RepoRoot = (Get-Location).Path
  )

  $env:SCCACHE_DIR = Get-LithoLocalSccacheDir -RepoRoot $RepoRoot
  $env:SCCACHE_SERVER_PORT = "5100"
  $env:SCCACHE_CACHE_SIZE = "5G"
  $env:SCCACHE_IDLE_TIMEOUT = "300"
  $env:SCCACHE_STARTUP_TIMEOUT = "30"
  $env:SCCACHE_REQUEST_TIMEOUT = "120"
  $env:SCCACHE_CACHE_COMPRESSION = "zstd"
  $env:SCCACHE_DIRECT = "true"
  $env:SCCACHE_LOG = "warn"
  $env:SCCACHE_NO_DAEMON = "0"
  $env:SCCACHE_MAX_CONNECTIONS = "4"
}

function Resolve-LithoCargoTargetDir {
  param(
    [string]$RepoRoot = (Get-Location).Path
  )

  if ($env:CARGO_TARGET_DIR -and -not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
      return $env:CARGO_TARGET_DIR
    }
    return (Join-Path $RepoRoot $env:CARGO_TARGET_DIR)
  }

  $globalTarget = "T:\RustCache\cargo-target"
  if (Test-Path $globalTarget) {
    return $globalTarget
  }

  return (Join-Path $RepoRoot "target")
}

function Get-LithoBinaryArtifactName {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Crate
  )

  switch ($Crate) {
    "litho-cli" { return "litho.exe" }
    default { return "$Crate.exe" }
  }
}

function Get-LithoCrateFromArtifactName {
  param(
    [Parameter(Mandatory = $true)]
    [string]$ArtifactName
  )

  switch ($ArtifactName.ToLowerInvariant()) {
    "litho.exe" { return "litho-cli" }
    default { return [System.IO.Path]::GetFileNameWithoutExtension($ArtifactName) }
  }
}

function Get-LithoPublishedBinaryPath {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Crate,
    [string]$RepoRoot = (Get-Location).Path
  )

  $releaseDir = Get-LithoRepoReleaseDir -RepoRoot $RepoRoot
  return (Join-Path $releaseDir (Get-LithoBinaryArtifactName -Crate $Crate))
}

function Get-LithoManifestPath {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Crate,
    [string]$RepoRoot = (Get-Location).Path
  )

  $manifestPath = Join-Path $RepoRoot ("crates/{0}/Cargo.toml" -f $Crate)
  if (-not (Test-Path $manifestPath)) {
    throw "Cargo manifest not found for crate '$Crate': $manifestPath"
  }

  return $manifestPath
}

function Get-LithoManifestVersion {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Crate,
    [string]$RepoRoot = (Get-Location).Path
  )

  $manifestPath = Get-LithoManifestPath -Crate $Crate -RepoRoot $RepoRoot
  $manifest = Get-Content -LiteralPath $manifestPath -Raw
  $match = [regex]::Match($manifest, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
  if ($match.Success) {
    return $match.Groups[1].Value
  }

  if ($manifest -match '(?m)^\s*version\.workspace\s*=\s*true\s*$') {
    $workspaceManifestPath = Join-Path $RepoRoot "Cargo.toml"
    $workspaceManifest = Get-Content -LiteralPath $workspaceManifestPath -Raw
    $workspaceMatch = [regex]::Match($workspaceManifest, '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
    if ($workspaceMatch.Success) {
      return $workspaceMatch.Groups[1].Value
    }
  }

  throw "Unable to determine package version from manifest: $manifestPath"
}

function Clear-LithoReleaseArtifacts {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Crates,
    [string]$RepoRoot = (Get-Location).Path
  )

  $releaseDir = Get-LithoRepoReleaseDir -RepoRoot $RepoRoot
  foreach ($crate in ($Crates | Select-Object -Unique)) {
    $artifact = Get-LithoBinaryArtifactName -Crate $crate
    foreach ($name in @($artifact, [System.IO.Path]::ChangeExtension($artifact, ".pdb"))) {
      $artifactPath = Join-Path $releaseDir $name
      if (Test-Path $artifactPath) {
        Remove-Item -LiteralPath $artifactPath -Force
        Write-Host "[clean] removed published artifact $artifactPath" -ForegroundColor DarkYellow
      }
    }
  }
}

function Publish-LithoReleaseArtifacts {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Crates,
    [string]$RepoRoot = (Get-Location).Path,
    [string]$Profile = "release"
  )

  $targetDir = Resolve-LithoCargoTargetDir -RepoRoot $RepoRoot
  $sourceDir = Join-Path $targetDir $Profile
  $destDir = Get-LithoRepoReleaseDir -RepoRoot $RepoRoot

  if (-not (Test-Path $sourceDir)) {
    throw "Cargo target output directory not found: $sourceDir"
  }

  foreach ($crate in $Crates) {
    $artifact = Get-LithoBinaryArtifactName -Crate $crate
    $sourcePath = Join-Path $sourceDir $artifact
    if (-not (Test-Path $sourcePath)) {
      throw "Built artifact not found for crate '$crate': $sourcePath"
    }

    $destPath = Join-Path $destDir $artifact
    Copy-Item -LiteralPath $sourcePath -Destination $destPath -Force
    Write-Host "[publish] $artifact -> $destPath" -ForegroundColor DarkGreen

    $pdbName = [System.IO.Path]::ChangeExtension($artifact, ".pdb")
    $pdbSource = Join-Path $sourceDir $pdbName
    if (Test-Path $pdbSource) {
      Copy-Item -LiteralPath $pdbSource -Destination (Join-Path $destDir $pdbName) -Force
    }
  }

  return $destDir
}

function Clear-LithoCargoReleaseArtifacts {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Crates,
    [string]$RepoRoot = (Get-Location).Path
  )

  $uniqueCrates = @($Crates | Select-Object -Unique)
  if ($uniqueCrates.Count -eq 0) {
    return
  }

  $arguments = @("clean", "--release")
  foreach ($crate in $uniqueCrates) {
    $arguments += @("-p", $crate)
  }

  Invoke-LithoCargo -Arguments $arguments -WorkingDirectory $RepoRoot
}

function Build-LithoReleaseArtifacts {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Crates,
    [string]$RepoRoot = (Get-Location).Path,
    [switch]$CleanPublished,
    [switch]$CleanCargoRelease,
    [switch]$RefreshBuildStamp,
    [switch]$UseIsolatedTargetDir
  )

  $uniqueCrates = @($Crates | Select-Object -Unique)
  if ($uniqueCrates.Count -eq 0) {
    throw "At least one crate must be provided."
  }

  $previousTargetDir = $env:CARGO_TARGET_DIR
  try {
    if ($UseIsolatedTargetDir.IsPresent) {
      $env:CARGO_TARGET_DIR = Get-LithoReleaseHarnessTargetDir -RepoRoot $RepoRoot
      Write-Host "[build] using isolated target dir $($env:CARGO_TARGET_DIR)" -ForegroundColor DarkCyan
    }

    if ($RefreshBuildStamp.IsPresent -or
      -not $env:LITHO_BUILD_TOKEN -or
      [string]::IsNullOrWhiteSpace($env:LITHO_BUILD_TOKEN)) {
      $stamp = Set-LithoBuildStamp
      Write-Host "[build] build token=$($stamp.BuildToken)" -ForegroundColor DarkCyan
    }

    Enable-LithoSccache -AllowFallback -RepoRoot $RepoRoot

    if ($CleanPublished.IsPresent) {
      Clear-LithoReleaseArtifacts -Crates $uniqueCrates -RepoRoot $RepoRoot
    }

    if ($CleanCargoRelease.IsPresent) {
      Clear-LithoCargoReleaseArtifacts -Crates $uniqueCrates -RepoRoot $RepoRoot
    }

    $arguments = @("build", "--release")
    foreach ($crate in $uniqueCrates) {
      $arguments += @("-p", $crate)
    }

    Invoke-LithoCargo -Arguments $arguments -WorkingDirectory $RepoRoot
    $publishedDir = Publish-LithoReleaseArtifacts -RepoRoot $RepoRoot -Crates $uniqueCrates

    return [PSCustomObject]@{
      BuildToken = $env:LITHO_BUILD_TOKEN
      PublishedDir = $publishedDir
      BinaryPaths = @($uniqueCrates | ForEach-Object {
          Get-LithoPublishedBinaryPath -Crate $_ -RepoRoot $RepoRoot
        })
    }
  }
  finally {
    if ([string]::IsNullOrWhiteSpace($previousTargetDir)) {
      Remove-Item env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    } else {
      $env:CARGO_TARGET_DIR = $previousTargetDir
    }
  }
}

function Resolve-RealCargoExe {
  $cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
  if ($cargo) { return $cargo }

  $rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
  if (Test-Path $rustup) {
    $resolved = & $rustup which cargo 2>$null
    if ($LASTEXITCODE -eq 0 -and $resolved) {
      return ($resolved | Select-Object -First 1).Trim()
    }
  }

  return "cargo"
}

function Set-LithoBuildStamp {
  param(
    [switch]$ExportToGitHubEnv
  )

  $gitTag = (& git describe --tags --always --dirty 2>$null)
  if ($LASTEXITCODE -ne 0 -or -not $gitTag) { $gitTag = "no-tag" }
  $gitTag = $gitTag.Trim()

  $gitSha = (& git rev-parse --short HEAD 2>$null)
  if ($LASTEXITCODE -ne 0 -or -not $gitSha) { $gitSha = "no-sha" }
  $gitSha = $gitSha.Trim()

  $timestamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  $tokenTime = (Get-Date).ToUniversalTime().ToString("yyyyMMdd-HHmmss")
  $safeTag = ($gitTag -replace "[^\w\.\-]+", "_")
  $token = "$safeTag-$gitSha-$tokenTime"

  $env:LITHO_BUILD_TIMESTAMP = $timestamp
  $env:LITHO_BUILD_GIT_TAG = $gitTag
  $env:LITHO_BUILD_GIT_SHA = $gitSha
  $env:LITHO_BUILD_TOKEN = $token
  $env:LITHO_EXPECT_BUILD_TOKEN = $token

  if ($ExportToGitHubEnv -and $env:GITHUB_ENV) {
    @(
      "LITHO_BUILD_TIMESTAMP=$timestamp"
      "LITHO_BUILD_GIT_TAG=$gitTag"
      "LITHO_BUILD_GIT_SHA=$gitSha"
      "LITHO_BUILD_TOKEN=$token"
      "LITHO_EXPECT_BUILD_TOKEN=$token"
    ) | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
  }

  return [PSCustomObject]@{
    Timestamp = $timestamp
    GitTag = $gitTag
    GitSha = $gitSha
    BuildToken = $token
  }
}

function Get-LithoFreeTcpPort {
  param(
    [int]$Start = 34000,
    [int]$End = 49000
  )

  for ($port = $Start; $port -le $End; $port++) {
    try {
      $listener = [System.Net.Sockets.TcpListener]::new(
        [System.Net.IPAddress]::Loopback,
        $port
      )
      $listener.Start()
      $listener.Stop()
      return $port
    } catch {
      continue
    }
  }

  throw "Unable to find a free TCP port in range [$Start, $End]."
}

function Enable-LithoSccache {
  param(
    [switch]$AllowFallback,
    [string]$RepoRoot = (Get-Location).Path
  )

  $sccacheExe = (Get-Command sccache.exe -ErrorAction SilentlyContinue).Source
  if (-not $sccacheExe) {
    Write-Warning "sccache.exe not found. Falling back to direct rustc."
    $env:RUSTC_WRAPPER = ""
    return
  }

  Set-LithoSccacheEnvironment -RepoRoot $RepoRoot

  if (-not $env:SCCACHE_SERVER_PORT -or [string]::IsNullOrWhiteSpace($env:SCCACHE_SERVER_PORT)) {
    $env:SCCACHE_SERVER_PORT = (Get-LithoFreeTcpPort).ToString()
  }

  $env:RUSTC_WRAPPER = "sccache"
  $output = & $sccacheExe --start-server 2>&1
  if ($LASTEXITCODE -ne 0) {
    $text = ($output | Out-String)
    if ($text -match "Address in use") {
      $probe = & $sccacheExe --show-stats 2>&1
      if ($LASTEXITCODE -eq 0) {
        Write-Host "[sccache] reusing existing server on port $($env:SCCACHE_SERVER_PORT)." -ForegroundColor DarkCyan
        return
      }
    }

    if ($AllowFallback.IsPresent) {
      Write-Warning "sccache failed to start: $text"
      Write-Warning "Falling back to direct rustc wrapper."
      $env:RUSTC_WRAPPER = ""
      return
    }
    throw "sccache failed to start: $text"
  }
}

function Test-SccacheStartupFailure {
  param(
    [Parameter(Mandatory = $true)]
    [string]$OutputText
  )

  return $OutputText -match "sccache: error: Server startup failed" -or
    $OutputText -match "Windows port exclusion" -or
    $OutputText -match "Try setting SCCACHE_SERVER_PORT"
}

function Invoke-LithoCargo {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Arguments,
    [string]$WorkingDirectory = (Get-Location).Path,
    [switch]$NoSccacheFallback
  )

  $cargoExe = Resolve-RealCargoExe
  Push-Location $WorkingDirectory
  try {
    $output = & $cargoExe @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    $output | ForEach-Object { Write-Host $_ }

    if ($exitCode -eq 0) { return }

    $joined = ($output | Out-String)
    if (-not $NoSccacheFallback.IsPresent -and
      $env:RUSTC_WRAPPER -eq "sccache" -and
      (Test-SccacheStartupFailure -OutputText $joined)) {
      Write-Warning "Detected sccache startup failure. Re-running cargo without sccache."
      $env:RUSTC_WRAPPER = ""
      $retry = & $cargoExe @Arguments 2>&1
      $retryExit = $LASTEXITCODE
      $retry | ForEach-Object { Write-Host $_ }
      if ($retryExit -ne 0) {
        throw "Cargo failed after sccache fallback (exit $retryExit): $($Arguments -join ' ')"
      }
      return
    }

    throw "Cargo command failed (exit $exitCode): $($Arguments -join ' ')"
  }
  finally {
    Pop-Location
  }
}
