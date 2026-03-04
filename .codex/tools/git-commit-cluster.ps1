param(
    [string]$RepoPath = (Get-Location).Path,
    [ValidateSet("auto", "directory", "semantic", "single")]
    [string]$Strategy = "auto",
    [int]$MaxGroup = 30,
    [switch]$WithOllama,
    [string]$TuningConfig = "",
    [string[]]$Tune = @(),
    [string]$OutFile = "",
    [switch]$PrettySummary
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-AnalyzerPath {
    $candidates = @(
        "C:\codedev\git-cluster\bin\git-cluster-analyzer.exe",
        "C:\codedev\.claude\tools\bin\git-cluster-analyzer.exe",
        "C:\Users\david\.claude\tools\git-cluster-analyzer\bin\git-cluster-analyzer.exe",
        "C:\Users\david\.codex\tools\git-cluster-analyzer\bin\git-cluster-analyzer.exe"
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) {
            return $path
        }
    }
    throw "git-cluster-analyzer.exe not found in known locations."
}

function Format-Summary {
    param([object]$Payload)
    $clusters = if ($Payload.PSObject.Properties.Name -contains "clusters") {
        $Payload.clusters
    } else {
        @()
    }
    if (-not $clusters -or $clusters.Count -eq 0) {
        Write-Host "No clusters proposed."
        return
    }
    $rows = foreach ($cluster in $clusters) {
        [pscustomobject]@{
            Id         = $cluster.id
            Type       = $cluster.type
            Scope      = $cluster.scope
            Files      = @($cluster.files).Count
            Confidence = [math]::Round([double]$cluster.confidence, 2)
            Message    = $cluster.message
        }
    }
    $rows | Sort-Object Id | Format-Table -AutoSize
}

$exe = Resolve-AnalyzerPath
$args = @("propose", "--repo", $RepoPath, "--strategy", $Strategy, "--max-group", $MaxGroup)
if ($WithOllama) { $args += "--with-ollama" }
if ($TuningConfig) { $args += @("--tuning-config", $TuningConfig) }
foreach ($entry in $Tune) {
    $args += @("--tune", $entry)
}

$raw = & $exe @args
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if ($OutFile) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutFile) | Out-Null
    $raw | Set-Content -Path $OutFile
}

if ($PrettySummary) {
    try {
        $json = $raw | ConvertFrom-Json
        if ($json.PSObject.Properties.Name -contains "ollamaWarning" -and $json.ollamaWarning) {
            Write-Warning "Ollama warning: $($json.ollamaWarning)"
        }
        Format-Summary -Payload $json
    } catch {
        Write-Warning "Unable to parse JSON output for summary mode."
        Write-Output $raw
    }
} else {
    Write-Output $raw
}
