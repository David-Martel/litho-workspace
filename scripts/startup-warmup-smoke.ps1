param(
    [string]$ReadmePath = "./deepwiki-rs/README.md",
    [string]$QmdBackend = "auto",
    [string]$QmdIndex = "warmup",
    [switch]$SkipGeneratorHelp,
    [switch]$SkipQmdStatus
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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

if (-not $SkipGeneratorHelp) {
    Invoke-Step -Name "Verify litho-generator CLI startup" -Action {
        cargo run -p litho-generator -- --help | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "litho-generator help command failed with code $LASTEXITCODE"
        }
    }
}

if (-not $SkipQmdStatus) {
    Invoke-Step -Name "Verify qmd CLI startup/status" -Action {
        cargo run -p litho-qmd-cli -- --backend $QmdBackend --index $QmdIndex status --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "qmd status command failed with code $LASTEXITCODE"
        }
    }
}

Write-Host "[warmup] PASS"
