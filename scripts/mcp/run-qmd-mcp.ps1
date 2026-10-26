param(
  [switch]$Check,
  [string]$RuntimeRoot,
  [string]$EntryPath,
  [switch]$PreferRust,
  [string]$DatabaseUrl,
  [switch]$SkipHealthcheck,
  [string]$RepoRoot = "C:\codedev\litho-workspace"
)

$ErrorActionPreference = "Stop"

function Resolve-Bun {
  $candidates = @(
    (Join-Path $env:USERPROFILE ".bun\bin\bun.exe"),
    "C:\Program Files\Bun\bun.exe"
  )

  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return $candidate
    }
  }

  $bunCmd = Get-Command bun.exe -ErrorAction SilentlyContinue
  if ($bunCmd -and $bunCmd.Source) {
    return $bunCmd.Source
  }

  return $null
}

function Resolve-RustQmdMcp {
  if ($env:QMD_RUST_MCP_BIN -and (Test-Path $env:QMD_RUST_MCP_BIN)) {
    return $env:QMD_RUST_MCP_BIN
  }

  $candidates = @(
    "C:\Users\david\bin\litho-qmd-mcp-20260720-meta.exe",
    "T:\RustCache\cargo-target\release\litho-qmd-mcp.exe",
    "C:\codedev\litho-workspace\target\release\litho-qmd-mcp.exe",
    "C:\codedev\litho-workspace\target\debug\litho-qmd-mcp.exe",
    "T:\RustCache\cargo-target\debug\litho-qmd-mcp.exe"
  )
  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return $candidate
    }
  }

  $cmd = Get-Command litho-qmd-mcp.exe -ErrorAction SilentlyContinue
  if ($cmd -and $cmd.Source) {
    return $cmd.Source
  }
  return $null
}

function Read-DotEnv([string]$path) {
  $result = @{}
  if (-not (Test-Path $path)) {
    return $result
  }

  foreach ($line in Get-Content $path -ErrorAction SilentlyContinue) {
    $trimmed = $line.Trim()
    if (-not $trimmed -or $trimmed.StartsWith("#")) {
      continue
    }
    $parts = $trimmed -split "=", 2
    if ($parts.Count -ne 2) {
      continue
    }
    $key = $parts[0].Trim()
    $value = $parts[1].Trim()
    if ($value.StartsWith('"') -and $value.EndsWith('"') -and $value.Length -ge 2) {
      $value = $value.Substring(1, $value.Length - 2)
    }
    $result[$key] = $value
  }

  return $result
}

function Read-RepoQmdConfig([string]$repoRootPath) {
  $paths = @(
    (Join-Path $repoRootPath "qmd.config.json"),
    (Join-Path $repoRootPath "config\qmd.config.json")
  )

  foreach ($path in $paths) {
    if (Test-Path $path) {
      try {
        return (Get-Content $path -Raw | ConvertFrom-Json)
      } catch {
        throw "Failed to parse QMD config file: $path ($($_.Exception.Message))"
      }
    }
  }

  return $null
}

function Get-ConfigValue($config, [string[]]$path) {
  if (-not $config) {
    return $null
  }
  $cursor = $config
  foreach ($segment in $path) {
    if ($null -eq $cursor) {
      return $null
    }
    $cursor = $cursor.$segment
  }
  return $cursor
}

function Coalesce([object[]]$values) {
  foreach ($v in $values) {
    if ($null -ne $v -and "$v".Trim() -ne "") {
      return "$v"
    }
  }
  return $null
}

function Resolve-DefaultDatabaseUrl($repoCfg, $dotenvValues) {
  if ($DatabaseUrl) {
    return $DatabaseUrl
  }

  $explicit = Coalesce @(
    (Get-ConfigValue $repoCfg @("database", "url")),
    $dotenvValues["QMD_DATABASE_URL"],
    $env:QMD_DATABASE_URL
  )
  if ($explicit) {
    return $explicit
  }

  $pgHost = Coalesce @((Get-ConfigValue $repoCfg @("database", "host")), $dotenvValues["QMD_DB_HOST"], $dotenvValues["PGHOST"], $env:PGHOST, "localhost")
  $pgPort = Coalesce @((Get-ConfigValue $repoCfg @("database", "port")), $dotenvValues["QMD_DB_PORT"], $dotenvValues["PGPORT"], $env:PGPORT, "5300")
  $pgUser = Coalesce @((Get-ConfigValue $repoCfg @("database", "user")), $dotenvValues["QMD_DB_USER"], $dotenvValues["PGUSER"], $env:PGUSER, "postgres")
  $pgPass = Coalesce @((Get-ConfigValue $repoCfg @("database", "password")), $dotenvValues["QMD_DB_PASSWORD"], $dotenvValues["PGPASSWORD"], $env:PGPASSWORD)
  if (-not $pgPass) {
    # No built-in default: a literal fallback password silently masks missing configuration.
    throw "No QMD database password configured. Set database.password in $RepoRoot\qmd.config.json, PGPASSWORD/QMD_DB_PASSWORD in $RepoRoot\.env, QMD_DATABASE_URL, or pass -DatabaseUrl."
  }
  $pgName = Coalesce @((Get-ConfigValue $repoCfg @("database", "name")), $dotenvValues["QMD_DB_NAME"], $dotenvValues["PGDATABASE"], $env:PGDATABASE, "qmd_index")

  return "postgresql://$pgUser`:$pgPass@${pgHost}:$pgPort/$pgName"
}

function Mask-DbUrl([string]$url) {
  return ($url -replace "(postgres(?:ql)?://[^:]+):[^@]+@", '$1:***@')
}

function Resolve-PgTools {
  $pgRoot = "C:\Program Files\PostgreSQL"
  if (Test-Path $pgRoot) {
    $versionDirs = Get-ChildItem -Path $pgRoot -Directory -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -match '^\d+(\.\d+)?$' } |
      Sort-Object {
        if ($_.Name -match '^\d+$') {
          [version]("$($_.Name).0")
        } else {
          [version]($_.Name)
        }
      } -Descending

    foreach ($versionDir in $versionDirs) {
      $dir = Join-Path $versionDir.FullName "bin"
      $pgIsReady = Join-Path $dir "pg_isready.exe"
      $psql = Join-Path $dir "psql.exe"
      $createdb = Join-Path $dir "createdb.exe"
      if ((Test-Path $pgIsReady) -and (Test-Path $psql) -and (Test-Path $createdb)) {
        return @{
          Dir = $dir
          Version = $versionDir.Name
          PgIsReady = $pgIsReady
          Psql = $psql
          CreateDb = $createdb
        }
      }
    }
  }

  $pgIsReadyCmd = Get-Command pg_isready.exe -ErrorAction SilentlyContinue
  $psqlCmd = Get-Command psql.exe -ErrorAction SilentlyContinue
  $createdbCmd = Get-Command createdb.exe -ErrorAction SilentlyContinue
  if ($pgIsReadyCmd -and $psqlCmd -and $createdbCmd) {
    return @{
      Dir = "<path>"
      Version = "unknown"
      PgIsReady = $pgIsReadyCmd.Source
      Psql = $psqlCmd.Source
      CreateDb = $createdbCmd.Source
    }
  }

  return $null
}

function Resolve-PostgresServiceName($tools) {
  $known = @()
  if ($tools -and $tools.Version -and $tools.Version -ne "unknown") {
    $known += "postgresql-x64-$($tools.Version)"
  }

  foreach ($name in $known) {
    $svc = Get-Service -Name $name -ErrorAction SilentlyContinue
    if ($svc) {
      return $svc.Name
    }
  }

  $svcCandidates = Get-Service -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like "postgresql-x64-*" } |
    Sort-Object {
      $raw = $_.Name -replace '^postgresql-x64-', ''
      if ($raw -match '^\d+(\.\d+)?$') { [version]$raw } else { [version]'0.0' }
    } -Descending

  if ($svcCandidates -and $svcCandidates.Count -gt 0) {
    return $svcCandidates[0].Name
  }

  return $null
}

function Wait-PostgresReady($tools, [string]$pgHost, [int]$pgPort, [int]$maxAttempts = 10, [int]$delaySeconds = 1) {
  for ($i = 0; $i -lt $maxAttempts; $i++) {
    & $tools.PgIsReady -h $pgHost -p $pgPort | Out-Null
    if ($LASTEXITCODE -eq 0) {
      return $true
    }
    Start-Sleep -Seconds $delaySeconds
  }
  return $false
}

function Get-DbPartsFromUrl([string]$url) {
  try {
    $uri = [System.Uri]$url
    $userinfo = $uri.UserInfo
    $user = "postgres"
    $pass = $null
    if ($userinfo) {
      $uiparts = $userinfo -split ":", 2
      $user = $uiparts[0]
      if ($uiparts.Count -eq 2) {
        $pass = $uiparts[1]
      }
    }
    return @{
      Host = $uri.Host
      Port = if ($uri.Port -gt 0) { $uri.Port } else { 5432 }
      Database = $uri.AbsolutePath.TrimStart('/')
      User = $user
      Password = $pass
    }
  } catch {
    return $null
  }
}

function Ensure-PostgresServerAndDb([string]$url, [string]$adminDb = "postgres") {
  if ($env:QMD_MCP_SKIP_DB_BOOTSTRAP -and @("1", "true", "True") -contains $env:QMD_MCP_SKIP_DB_BOOTSTRAP) {
    return
  }

  $tools = Resolve-PgTools
  if (-not $tools) {
    Write-Verbose "Postgres CLI tools were not found; skipping local DB bootstrap."
    return
  }

  $parts = Get-DbPartsFromUrl $url
  if (-not $parts -or -not $parts.Database) {
    return
  }

  $pgPass = Coalesce @($parts.Password, $env:PGPASSWORD)
  if ($pgPass) {
    $env:PGPASSWORD = $pgPass
  }

  $ready = Wait-PostgresReady $tools $parts.Host $parts.Port 2 1
  if (-not $ready) {
    $serviceName = Resolve-PostgresServiceName $tools
    if ($serviceName) {
      $svc = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
      if ($svc -and $svc.Status -ne 'Running') {
        Start-Service -Name $serviceName -ErrorAction SilentlyContinue
      }
    }
    $ready = Wait-PostgresReady $tools $parts.Host $parts.Port 15 1
  }

  if (-not $ready) {
    throw "Postgres is not accepting connections on ${parts.Host}:$($parts.Port)."
  }

  $dbNameSql = $parts.Database.Replace("'", "''")
  $existsOutput = & $tools.Psql -h $parts.Host -p $parts.Port -U $parts.User -d $adminDb -t -A -c "SELECT 1 FROM pg_database WHERE datname='${dbNameSql}';" 2>$null
  $exists = ($LASTEXITCODE -eq 0 -and "$existsOutput".Trim() -eq "1")
  if (-not $exists) {
    & $tools.CreateDb -h $parts.Host -p $parts.Port -U $parts.User $parts.Database | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw "Unable to create database '$($parts.Database)' using user '$($parts.User)'."
    }
  }
}

$repoCfg = Read-RepoQmdConfig $RepoRoot
$dotenvValues = Read-DotEnv (Join-Path $RepoRoot ".env")
if (-not $dotenvValues.Count) {
  $dotenvValues = Read-DotEnv (Join-Path $RepoRoot "config\.env")
}

$rustMcp = Resolve-RustQmdMcp
$bun = Resolve-Bun
$runtimeRoot = if ($RuntimeRoot) { $RuntimeRoot } else { "C:\Users\david\.codex\tools\qmd-mcp\runtime" }
$mcpEntry = if ($EntryPath) {
  $EntryPath
} elseif ($env:QMD_MCP_ENTRY -and (Test-Path $env:QMD_MCP_ENTRY)) {
  $env:QMD_MCP_ENTRY
} elseif (Test-Path (Join-Path $runtimeRoot "dist\mcp.js")) {
  Join-Path $runtimeRoot "dist\mcp.js"
} else {
  Join-Path $runtimeRoot "src\mcp.ts"
}
$runtimeNodeModules = Join-Path $runtimeRoot "node_modules"

$env:QMD_DATABASE_URL = Resolve-DefaultDatabaseUrl $repoCfg $dotenvValues
$qmdAdminDb = Coalesce @(
  (Get-ConfigValue $repoCfg @("database", "admin_db")),
  $dotenvValues["QMD_PG_ADMIN_DB"],
  "postgres"
)
if (-not $env:PGPASSWORD) {
  $env:PGPASSWORD = Coalesce @(
    $dotenvValues["PGPASSWORD"],
    $dotenvValues["QMD_DB_PASSWORD"],
    (Get-ConfigValue $repoCfg @("database", "password"))
  )
}
if (-not $env:QMD_MCP_LINE_NUMBERS) {
  $env:QMD_MCP_LINE_NUMBERS = "0"
}
$env:NO_COLOR = "1"

$preferRustResolved = $true
if ($env:QMD_MCP_PREFER_RUST -and @("0", "false", "False") -contains $env:QMD_MCP_PREFER_RUST) {
  $preferRustResolved = $false
}
if ($PreferRust) {
  $preferRustResolved = $true
}

if ($Check) {
  $rustPath = if ($rustMcp) { $rustMcp } else { "<not-found>" }
  Write-Output "bun: $(if ($bun) { $bun } else { '<not-found>' })"
  Write-Output "rust_qmd_mcp: $rustPath"
  Write-Output "qmd_repo_root: $RepoRoot"
  Write-Output "qmd_runtime: $runtimeRoot"
  Write-Output "qmd_mcp: $mcpEntry"
  Write-Output "qmd_database_url: $(Mask-DbUrl $env:QMD_DATABASE_URL)"
  Write-Output "prefer_rust_default: $preferRustResolved"
  exit 0
}

if ($preferRustResolved -and $rustMcp) {
  Ensure-PostgresServerAndDb $env:QMD_DATABASE_URL $qmdAdminDb

  $skipCheck = $SkipHealthcheck -or $env:QMD_MCP_SKIP_HEALTHCHECK -eq "1"
  Push-Location $RepoRoot
  try {
    if (-not $skipCheck) {
      & $rustMcp --healthcheck | Out-Null
      if ($LASTEXITCODE -ne 0) {
        throw "litho-qmd-mcp healthcheck failed with Postgres URL $(Mask-DbUrl $env:QMD_DATABASE_URL)"
      }
    }

    & $rustMcp
    exit $LASTEXITCODE
  } finally {
    Pop-Location
  }
}

if (-not $bun) {
  throw "Bun runtime not found and Rust MCP is unavailable."
}
if (-not (Test-Path $mcpEntry)) {
  throw "QMD MCP runtime entrypoint missing: $mcpEntry"
}
if (-not (Test-Path $runtimeNodeModules)) {
  throw "QMD MCP dependencies missing at $runtimeNodeModules. Run: bun install --production (in $runtimeRoot)."
}

Push-Location $runtimeRoot
try {
  & $bun $mcpEntry
  exit $LASTEXITCODE
} finally {
  Pop-Location
}
