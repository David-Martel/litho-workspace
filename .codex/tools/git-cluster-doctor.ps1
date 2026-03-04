param(
    [string]$RepoPath = "",
    [string]$TuningConfig = "",
    [string[]]$Tune = @(),
    [switch]$CheckOllama
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

$exe = Resolve-AnalyzerPath
$args = @("doctor")
if ($RepoPath) { $args += @("--repo", $RepoPath) }
if ($TuningConfig) { $args += @("--tuning-config", $TuningConfig) }
if ($CheckOllama) { $args += "--check-ollama" }
foreach ($entry in $Tune) {
    $args += @("--tune", $entry)
}

& $exe @args
exit $LASTEXITCODE
