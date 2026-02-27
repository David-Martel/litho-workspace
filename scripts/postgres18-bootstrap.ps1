param(
  [string]$PgBin = "C:\Program Files\PostgreSQL\18\bin",
  [string]$PgHost = "127.0.0.1",
  [int]$Port = 5432,
  [string]$User = "postgres",
  [string]$Password = "password",
  [string]$Database = "qmd_index",
  [switch]$RestartService
)

$ErrorActionPreference = "Stop"

$pgIsReady = Join-Path $PgBin "pg_isready.exe"
$psql = Join-Path $PgBin "psql.exe"
$createdb = Join-Path $PgBin "createdb.exe"

if (-not (Test-Path $pgIsReady) -or -not (Test-Path $psql) -or -not (Test-Path $createdb)) {
  throw "PostgreSQL tools not found in $PgBin"
}

$svc = Get-Service postgresql-x64-18 -ErrorAction SilentlyContinue
if ($svc -and ($RestartService -or $svc.Status -ne 'Running')) {
  if ($RestartService -and $svc.Status -eq 'Running') {
    Restart-Service postgresql-x64-18 -ErrorAction Stop
  } else {
    Start-Service postgresql-x64-18 -ErrorAction Stop
  }
}

& $pgIsReady -h $PgHost -p $Port
if ($LASTEXITCODE -ne 0) {
  throw "PostgreSQL is not accepting connections on ${PgHost}:$Port"
}

$env:PGPASSWORD = $Password

# Verify auth and apply password to role as idempotent maintenance.
$alterRoleSql = "ALTER ROLE ""$User"" WITH PASSWORD '$Password';"
& $psql -h $PgHost -p $Port -U $User -d postgres -v ON_ERROR_STOP=1 -c $alterRoleSql
if ($LASTEXITCODE -ne 0) {
  throw "Unable to authenticate as $User against postgres DB"
}

$exists = & $psql -h $PgHost -p $Port -U $User -d postgres -t -A -c "SELECT 1 FROM pg_database WHERE datname='$Database';"
if ($LASTEXITCODE -ne 0) {
  throw "Failed checking database existence for $Database"
}

if ("$exists".Trim() -ne "1") {
  & $createdb -h $PgHost -p $Port -U $User $Database
  if ($LASTEXITCODE -ne 0) {
    throw "Failed creating database: $Database"
  }
}

Write-Output "postgres_ok host=$PgHost port=$Port user=$User db=$Database"
