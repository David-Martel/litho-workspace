Set-StrictMode -Version Latest

function Resolve-RealCargoExe {
  $rustup = Join-Path $env:USERPROFILE ".cargo\bin\rustup.exe"
  if (Test-Path $rustup) {
    $resolved = & $rustup which cargo 2>$null
    if ($LASTEXITCODE -eq 0 -and $resolved) {
      return ($resolved | Select-Object -First 1).Trim()
    }
  }

  $cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
  if ($cargo) { return $cargo }
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
    [switch]$AllowFallback
  )

  $sccacheExe = (Get-Command sccache.exe -ErrorAction SilentlyContinue).Source
  if (-not $sccacheExe) {
    Write-Warning "sccache.exe not found. Falling back to direct rustc."
    $env:RUSTC_WRAPPER = ""
    return
  }

  if (-not $env:SCCACHE_SERVER_PORT -or [string]::IsNullOrWhiteSpace($env:SCCACHE_SERVER_PORT)) {
    $env:SCCACHE_SERVER_PORT = (Get-LithoFreeTcpPort).ToString()
  }

  $env:RUSTC_WRAPPER = "sccache"
  $output = & $sccacheExe --start-server 2>&1
  if ($LASTEXITCODE -ne 0) {
    $text = ($output | Out-String)
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
