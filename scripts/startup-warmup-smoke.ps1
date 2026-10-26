param(
    [string]$ReadmePath = "./deepwiki-rs/README.md",
    [string]$QmdBackend = "auto",
    [string]$QmdIndex = "warmup",
    [switch]$SkipGeneratorHelp,
    [switch]$SkipQmdStatus,
    [switch]$SkipBuildRelease,
    [switch]$SkipArtifactVerification
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
. (Join-Path $repoRoot "scripts\litho-build-helpers.ps1")

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )
    Write-Host "[warmup] $Name"
    & $Action
}

Invoke-Step -Name "Validate README path exists" -Action {
    if (!(Test-Path $ReadmePath)) {
        throw "README not found at: $ReadmePath"
    }
}

Invoke-Step -Name "Read README content" -Action {
    $raw = Get-Content -Path $ReadmePath -Raw
    if ([string]::IsNullOrWhiteSpace($raw)) {
        throw "README file is empty: $ReadmePath"
    }
    $lineCount = ($raw -split "`r?`n").Count
    $wordCount = ($raw -split "\s+").Where({ $_ -ne "" }).Count
    Write-Host "[warmup] README lines=$lineCount words=$wordCount"
}

$cratesToExercise = @()
if (-not $SkipGeneratorHelp) {
    $cratesToExercise += "litho-generator"
}
if (-not $SkipQmdStatus) {
    $cratesToExercise += "litho-qmd-cli"
}
$cratesToExercise = @($cratesToExercise | Select-Object -Unique)

if (-not $SkipBuildRelease -and $cratesToExercise.Count -gt 0) {
    Invoke-Step -Name "Build and publish fresh release artifacts" -Action {
        $result = Build-LithoReleaseArtifacts `
            -Crates $cratesToExercise `
            -RepoRoot $repoRoot `
            -CleanPublished `
            -CleanCargoRelease `
            -RefreshBuildStamp `
            -UseIsolatedTargetDir
        Write-Host "[warmup] published to $($result.PublishedDir)"
    }
}

if (-not $SkipArtifactVerification -and $cratesToExercise.Count -gt 0) {
    Invoke-Step -Name "Verify published build metadata" -Action {
        & (Join-Path $repoRoot "scripts\verify-build-tokens.ps1") -Crates $cratesToExercise
        if ($LASTEXITCODE -ne 0) {
            throw "build metadata verification failed with code $LASTEXITCODE"
        }
    }
}

if (-not $SkipGeneratorHelp) {
    Invoke-Step -Name "Verify litho-generator CLI startup" -Action {
        $generatorBinary = Get-LithoPublishedBinaryPath -Crate "litho-generator" -RepoRoot $repoRoot
        & $generatorBinary --help | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "litho-generator help command failed with code $LASTEXITCODE"
        }
    }
}

if (-not $SkipQmdStatus) {
    Invoke-Step -Name "Verify qmd CLI startup/status" -Action {
        $qmdBinary = Get-LithoPublishedBinaryPath -Crate "litho-qmd-cli" -RepoRoot $repoRoot
        & $qmdBinary --backend $QmdBackend --index $QmdIndex status --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "qmd status command failed with code $LASTEXITCODE"
        }
    }
}

Write-Host "[warmup] PASS"
