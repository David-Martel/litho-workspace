param(
    [string]$ProjectPath = ".",
    [string]$OutputPath = ".\litho.docs",
    [string]$ConfigPath = "",
    [int]$Iterations = 2,
    [switch]$IncrementalAfterFirst,
    [switch]$BuildRelease,
    [switch]$UseCodexRs,
    [string]$ModelEfficient = "",
    [string]$ModelPowerful = "",
    [string]$LlmApiBaseUrl = "",
    [string]$LlmApiKey = "",
    [string]$Format = "md"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($Iterations -lt 1) {
    throw "Iterations must be >= 1."
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$binPath = Join-Path $repoRoot "target\release\litho-generator.exe"

Push-Location $repoRoot
try {
    if ($BuildRelease -or -not (Test-Path $binPath)) {
        Write-Host "Building litho-generator release binary..."
        cargo build --release -p litho-generator
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to build litho-generator (exit $LASTEXITCODE)."
        }
    }

    if (-not (Test-Path $binPath)) {
        throw "Binary not found: $binPath"
    }

    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $runDir = Join-Path $repoRoot ".litho\runs\$timestamp"
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null

    $summary = @()

    for ($i = 1; $i -le $Iterations; $i++) {
        $args = @()

        if ($IncrementalAfterFirst.IsPresent -and $i -gt 1) {
            $args += "--incremental"
        }

        $args += @("--project-path", $ProjectPath)
        $args += @("--output-path", $OutputPath)
        $args += @("--format", $Format)

        if ($ConfigPath -and $ConfigPath.Trim().Length -gt 0) {
            $args += @("--config", $ConfigPath)
        }
        if ($UseCodexRs.IsPresent) {
            $args += @("--llm-provider", "codexrs")
        }
        if ($ModelEfficient -and $ModelEfficient.Trim().Length -gt 0) {
            $args += @("--model-efficient", $ModelEfficient)
        }
        if ($ModelPowerful -and $ModelPowerful.Trim().Length -gt 0) {
            $args += @("--model-powerful", $ModelPowerful)
        }
        if ($LlmApiBaseUrl -and $LlmApiBaseUrl.Trim().Length -gt 0) {
            $args += @("--llm-api-base-url", $LlmApiBaseUrl)
        }
        if ($LlmApiKey -and $LlmApiKey.Trim().Length -gt 0) {
            $args += @("--llm-api-key", $LlmApiKey)
        }

        $mode = if ($args -contains "--incremental") { "incremental" } else { "full" }
        Write-Host ""
        Write-Host "Run $i/$Iterations ($mode): $binPath $($args -join ' ')"

        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & $binPath @args
        $exitCode = $LASTEXITCODE
        $sw.Stop()

        if ($exitCode -ne 0) {
            throw "Run $i failed with exit code $exitCode."
        }

        $resolvedOutput = (Resolve-Path $OutputPath).Path
        $validationPath = Join-Path $resolvedOutput "validation-report.json"
        $briefSummaryPath = Join-Path $resolvedOutput "__Litho_Summary_Brief__.md"

        $qualityScore = $null
        if (Test-Path $validationPath) {
            try {
                $validation = Get-Content $validationPath -Raw | ConvertFrom-Json
                if ($null -ne $validation.quality_score) {
                    $qualityScore = [math]::Round(([double]$validation.quality_score) * 100.0, 2)
                }
            } catch {
                Write-Warning "Could not parse ${validationPath}: $($_.Exception.Message)"
            }

            Copy-Item $validationPath (Join-Path $runDir "validation-report.run$i.json") -Force
        }

        if (Test-Path $briefSummaryPath) {
            Copy-Item $briefSummaryPath (Join-Path $runDir "__Litho_Summary_Brief__.run$i.md") -Force
        }

        $entry = [PSCustomObject]@{
            run                = $i
            mode               = $mode
            duration_seconds   = [math]::Round($sw.Elapsed.TotalSeconds, 2)
            quality_score_pct  = $qualityScore
            output_path        = $resolvedOutput
            timestamp_utc      = (Get-Date).ToUniversalTime().ToString("o")
        }
        $summary += $entry

        if ($null -ne $qualityScore) {
            Write-Host "Run $i completed in $($entry.duration_seconds)s; quality=$qualityScore%"
        } else {
            Write-Host "Run $i completed in $($entry.duration_seconds)s; quality report not found."
        }
    }

    $jsonPath = Join-Path $runDir "run-summary.json"
    $summary | ConvertTo-Json -Depth 4 | Set-Content -Path $jsonPath -Encoding UTF8

    $mdPath = Join-Path $runDir "run-summary.md"
    $md = @()
    $md += "# Litho Bootstrap Run Summary"
    $md += ""
    $md += "Generated: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss zzz")"
    $md += ""
    $md += "| Run | Mode | Duration (s) | Quality (%) |"
    $md += "|---|---|---:|---:|"
    foreach ($row in $summary) {
        $q = if ($null -eq $row.quality_score_pct) { "n/a" } else { $row.quality_score_pct }
        $md += "| $($row.run) | $($row.mode) | $($row.duration_seconds) | $q |"
    }
    Set-Content -Path $mdPath -Value ($md -join [Environment]::NewLine) -Encoding UTF8

    Write-Host ""
    Write-Host "Bootstrap completed."
    Write-Host "Run artifacts: $runDir"
    Write-Host "Summary JSON: $jsonPath"
    Write-Host "Summary MD:   $mdPath"
}
finally {
    Pop-Location
}
