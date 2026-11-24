# ==============================================================================
# DATA-TO-ACTION: Portable PostgreSQL 17.12.0 Setup & Database Seeding
# - Downloads official theseus-rs/postgresql-binaries Windows x64 archive
# - Verifies SHA-256 checksum
# - Generates .env from .env.example with local random secrets (never printed)
# - Initializes cluster on isolated loopback port (default 127.0.0.1:5439)
# - Creates separated databases, least-privilege dta_readonly role, and seed data
# ==============================================================================

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$LocalDir = Join-Path $RootDir ".local"
$DownloadDir = Join-Path $LocalDir "downloads"
$PgDir = Join-Path $LocalDir "postgres"
$PgDataDir = Join-Path $LocalDir "pgdata"
$LogDir = Join-Path $LocalDir "logs"
$EnvFile = Join-Path $RootDir ".env"
$EnvExample = Join-Path $RootDir ".env.example"

$PgVersion = "17.12.0"
$PgZipName = "postgresql-$PgVersion-x86_64-pc-windows-msvc.zip"
$PgZipUrl = "https://github.com/theseus-rs/postgresql-binaries/releases/download/$PgVersion/$PgZipName"
$PgShaUrl = "$PgZipUrl.sha256"

New-Item -ItemType Directory -Force -Path $LocalDir, $DownloadDir, $LogDir | Out-Null

function New-RandomHex([int]$ByteCount) {
    $bytes = New-Object byte[] $ByteCount
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $rng.GetBytes($bytes)
    $rng.Dispose()
    return ([System.BitConverter]::ToString($bytes) -replace "-", "").ToLowerInvariant()
}

function Read-DotEnv([string]$Path) {
    $map = @{}
    foreach ($line in Get-Content $Path) {
        $trimmed = $line.Trim()
        if ([string]::IsNullOrWhiteSpace($trimmed) -or $trimmed.StartsWith("#")) { continue }
        $idx = $trimmed.IndexOf("=")
        if ($idx -gt 0) {
            $k = $trimmed.Substring(0, $idx).Trim()
            $v = $trimmed.Substring($idx + 1).Trim()
            $map[$k] = $v
        }
    }
    return $map
}

# 1. Generate .env from .env.example if missing
if (-not (Test-Path $EnvFile)) {
    Write-Host "[setup-postgres] Creating .env with locally generated random secrets..."
    $content = Get-Content $EnvExample -Raw
    $content = $content.Replace("CHANGE_ME_APP_DB_PASSWORD", (New-RandomHex 16))
    $content = $content.Replace("CHANGE_ME_READONLY_DB_PASSWORD", (New-RandomHex 16))
    $content = $content.Replace("CHANGE_ME_JWT_SECRET_AT_LEAST_32_CHARS", (New-RandomHex 32))
    $content = $content.Replace("0000000000000000000000000000000000000000000000000000000000000000", (New-RandomHex 32))
    Set-Content -Path $EnvFile -Value $content -Encoding UTF8
}

$envMap = Read-DotEnv $EnvFile
$PgHost = if ($envMap["DTA_PG_HOST"]) { $envMap["DTA_PG_HOST"] } else { "127.0.0.1" }
$PgPort = if ($envMap["DTA_PG_PORT"]) { $envMap["DTA_PG_PORT"] } else { "5439" }
$AppDbName = if ($envMap["DTA_APP_DATABASE"]) { $envMap["DTA_APP_DATABASE"] } else { "data_to_action_app" }
$AppDbUser = if ($envMap["DTA_APP_DB_USER"]) { $envMap["DTA_APP_DB_USER"] } else { "dta_app_user" }
$AppDbPass = $envMap["DTA_APP_DB_PASSWORD"]
$AnalyticsDbName = if ($envMap["DTA_ANALYTICS_DATABASE"]) { $envMap["DTA_ANALYTICS_DATABASE"] } else { "data_to_action_analytics" }
$RoDbUser = if ($envMap["DTA_READONLY_DB_USER"]) { $envMap["DTA_READONLY_DB_USER"] } else { "dta_readonly" }
$RoDbPass = $envMap["DTA_READONLY_DB_PASSWORD"]

if ([string]::IsNullOrWhiteSpace($AppDbPass) -or [string]::IsNullOrWhiteSpace($RoDbPass)) {
    throw "Database passwords in .env cannot be empty."
}

# 2. Download & verify portable PostgreSQL binaries if not yet installed
$InitDbExe = Join-Path $PgDir "bin\initdb.exe"
$PgCtlExe = Join-Path $PgDir "bin\pg_ctl.exe"
$PsqlExe = Join-Path $PgDir "bin\psql.exe"
$PgIsReadyExe = Join-Path $PgDir "bin\pg_isready.exe"

if (-not (Test-Path $PsqlExe)) {
    $zipPath = Join-Path $DownloadDir $PgZipName
    $shaPath = Join-Path $DownloadDir "$PgZipName.sha256"

    if (-not (Test-Path $zipPath) -or ((Get-Item $zipPath).Length -lt 1000000)) {
        Remove-Item -Force $zipPath -ErrorAction SilentlyContinue
        Write-Host "[setup-postgres] Downloading PostgreSQL $PgVersion binaries..."
        if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
            & curl.exe -fsSL --http1.1 --retry 3 --connect-timeout 15 -o $zipPath $PgZipUrl
            if ($LASTEXITCODE -ne 0) { throw "curl.exe failed downloading $PgZipUrl (exit=$LASTEXITCODE)" }
        } else {
            Invoke-WebRequest -Uri $PgZipUrl -OutFile $zipPath -UseBasicParsing
        }
    }
    if (-not (Test-Path $shaPath) -or ((Get-Item $shaPath).Length -lt 32)) {
        Remove-Item -Force $shaPath -ErrorAction SilentlyContinue
        if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
            & curl.exe -fsSL --http1.1 --retry 3 --connect-timeout 15 -o $shaPath $PgShaUrl
            if ($LASTEXITCODE -ne 0) { throw "curl.exe failed downloading $PgShaUrl (exit=$LASTEXITCODE)" }
        } else {
            Invoke-WebRequest -Uri $PgShaUrl -OutFile $shaPath -UseBasicParsing
        }
    }

    $expectedSha = ((Get-Content $shaPath -Raw).Trim() -split "\s+")[0].ToLowerInvariant()
    $actualSha = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expectedSha -ne $actualSha) {
        Remove-Item -Force $zipPath -ErrorAction SilentlyContinue
        throw "SHA-256 verification failed for $PgZipName (expected $expectedSha, got $actualSha)"
    }
    Write-Host "[setup-postgres] SHA-256 checksum verified: $actualSha"

    if (Test-Path $PgDir) { Remove-Item -Recurse -Force $PgDir }
    New-Item -ItemType Directory -Force -Path $PgDir | Out-Null
    Write-Host "[setup-postgres] Extracting PostgreSQL $PgVersion..."
    if (Get-Command tar.exe -ErrorAction SilentlyContinue) {
        & tar.exe -xf $zipPath -C $PgDir
        if ($LASTEXITCODE -ne 0) { throw "tar.exe failed extracting $zipPath" }
    } else {
        Expand-Archive -Path $zipPath -DestinationPath $PgDir -Force
    }

    # Handle nested directory if archive wraps root folder
    if (-not (Test-Path $PsqlExe)) {
        $sub = Get-ChildItem $PgDir -Directory | Select-Object -First 1
        if ($sub -and (Test-Path (Join-Path $sub.FullName "bin\psql.exe"))) {
            Get-ChildItem $sub.FullName | Move-Item -Destination $PgDir -Force
            Remove-Item -Recurse -Force $sub.FullName -ErrorAction SilentlyContinue
        }
    }
    if (-not (Test-Path $PsqlExe)) {
        throw "Extracted archive did not contain bin\psql.exe under $PgDir"
    }
}

# 3. Bootstrap superuser password file (ephemeral, removed immediately)
$BootstrapAdminPassFile = Join-Path $LocalDir "pg_bootstrap.pw"
$AdminPassFilePersist = Join-Path $LocalDir "pg_admin.secret"
if (-not (Test-Path $AdminPassFilePersist)) {
    $bootstrapPass = New-RandomHex 16
    [System.IO.File]::WriteAllText($AdminPassFilePersist, $bootstrapPass, (New-Object System.Text.UTF8Encoding($false)))
} else {
    $bootstrapPass = (Get-Content $AdminPassFilePersist -Raw).Trim()
}

# 4. Initialize pgdata cluster if not already initialized
$PgVersionFile = Join-Path $PgDataDir "PG_VERSION"
if (-not (Test-Path $PgVersionFile)) {
    Write-Host "[setup-postgres] Initializing PostgreSQL data cluster at .local\pgdata..."
    New-Item -ItemType Directory -Force -Path $PgDataDir | Out-Null
    [System.IO.File]::WriteAllText($BootstrapAdminPassFile, $bootstrapPass, (New-Object System.Text.UTF8Encoding($false)))
    try {
        $initProc = Start-Process -FilePath $InitDbExe `
            -ArgumentList @("-D", "`"$PgDataDir`"", "-U", "postgres", "--auth=scram-sha-256", "--pwfile=`"$BootstrapAdminPassFile`"", "-E", "UTF8", "--locale=C") `
            -WindowStyle Hidden -Wait -PassThru
        if ($initProc.ExitCode -ne 0) {
            throw "initdb.exe exited with code $($initProc.ExitCode)"
        }
    } finally {
        Remove-Item -Force $BootstrapAdminPassFile -ErrorAction SilentlyContinue
    }

    # Harden postgresql.conf to listen strictly on loopback and allocated port
    $pgConf = Join-Path $PgDataDir "postgresql.conf"
    Add-Content -Path $pgConf -Value "`n# Data-to-Action Isolated Loopback Configuration`nlisten_addresses = '$PgHost'`nport = $PgPort`nmax_connections = 50`nlog_destination = 'stderr'`nlogging_collector = off`n"
}

# 5. Start PostgreSQL if not currently running
$pgLogFile = Join-Path $LogDir "postgres.log"
& $PgIsReadyExe -h $PgHost -p $PgPort -U postgres > $null 2>&1
if ($LASTEXITCODE -ne 0) {
    # Check port availability before starting
    $portInUse = Get-NetTCPConnection -LocalPort ([int]$PgPort) -State Listen -ErrorAction SilentlyContinue
    if ($portInUse) {
        throw "Port $PgPort is already occupied by another process (PID $($portInUse[0].OwningProcess))."
    }
    Write-Host "[setup-postgres] Starting PostgreSQL on ${PgHost}:${PgPort}..."
    $PostgresExe = Join-Path $PgDir "bin\postgres.exe"
    $pgErrFile = Join-Path $LogDir "postgres.err.log"
    $pgProc = Start-Process -FilePath $PostgresExe `
        -ArgumentList @("-D", "`"$PgDataDir`"", "-h", $PgHost, "-p", $PgPort) `
        -WindowStyle Hidden -RedirectStandardOutput $pgLogFile -RedirectStandardError $pgErrFile -PassThru
    $ready = $false
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 500
        & $PgIsReadyExe -h $PgHost -p $PgPort -U postgres > $null 2>&1
        if ($LASTEXITCODE -eq 0) {
            $ready = $true
            break
        }
        if ($pgProc.HasExited) {
            throw "postgres.exe exited unexpectedly with code $($pgProc.ExitCode). Check $pgErrFile"
        }
    }
    if (-not $ready) {
        throw "PostgreSQL did not become ready on ${PgHost}:${PgPort} within 15s. Check $pgErrFile"
    }
}

# 6. Apply SQL schemas and seed data using PGPASSWORD in process scope only
$prevPgPassword = $env:PGPASSWORD
try {
    $env:PGPASSWORD = $bootstrapPass

    Write-Host "[setup-postgres] Applying 00_setup_databases_and_roles.sql..."
    & $PsqlExe -h $PgHost -p $PgPort -U postgres -d postgres `
        -v app_db_name="$AppDbName" `
        -v app_db_user="$AppDbUser" `
        -v app_db_password="$AppDbPass" `
        -v analytics_db_name="$AnalyticsDbName" `
        -v readonly_db_user="$RoDbUser" `
        -v readonly_db_password="$RoDbPass" `
        -f (Join-Path $RootDir "sql\00_setup_databases_and_roles.sql")
    if ($LASTEXITCODE -ne 0) { throw "Failed applying 00_setup_databases_and_roles.sql" }

    Write-Host "[setup-postgres] Seeding $AnalyticsDbName with 01_seed_analytics_db.sql..."
    & $PsqlExe -h $PgHost -p $PgPort -U postgres -d $AnalyticsDbName `
        -f (Join-Path $RootDir "sql\01_seed_analytics_db.sql")
    if ($LASTEXITCODE -ne 0) { throw "Failed applying 01_seed_analytics_db.sql" }

    Write-Host "[setup-postgres] Initializing $AppDbName with 02_init_app_db.sql..."
    $env:PGPASSWORD = $AppDbPass
    & $PsqlExe -h $PgHost -p $PgPort -U $AppDbUser -d $AppDbName `
        -f (Join-Path $RootDir "sql\02_init_app_db.sql")
    if ($LASTEXITCODE -ne 0) { throw "Failed applying 02_init_app_db.sql" }

    # 7. Verify dta_readonly can query analytics and CANNOT write or read restricted_hr
    $env:PGPASSWORD = $RoDbPass
    $orderCount = & $PsqlExe -h $PgHost -p $PgPort -U $RoDbUser -d $AnalyticsDbName -t -A -c "SELECT COUNT(*) FROM analytics.orders;"
    if ($LASTEXITCODE -ne 0) { throw "Read-only verification query failed!" }
    Write-Host "[setup-postgres] Verified read-only query (analytics.orders count = $orderCount)."

    $ErrorActionPreference = "Continue"
    $null = & $PsqlExe -h $PgHost -p $PgPort -U $RoDbUser -d $AnalyticsDbName -c "INSERT INTO analytics.regions (region_code, region_name, country) VALUES ('XX', 'Blocked', 'Nowhere');" 2>&1
    $insertExit = $LASTEXITCODE
    $null = & $PsqlExe -h $PgHost -p $PgPort -U $RoDbUser -d $AnalyticsDbName -c "SELECT * FROM restricted_hr.employee_salaries;" 2>&1
    $hrExit = $LASTEXITCODE
    $ErrorActionPreference = "Stop"

    if ($insertExit -eq 0 -or $hrExit -eq 0) {
        throw "Security verification FAILED: dta_readonly was able to write or access restricted_hr!"
    }
    Write-Host "[setup-postgres] Verified read-only boundary: INSERT blocked (exit=$insertExit) and restricted_hr blocked (exit=$hrExit)."
} finally {
    $env:PGPASSWORD = $prevPgPassword
}

Write-Host "[setup-postgres] PostgreSQL 17.12.0 setup complete on ${PgHost}:${PgPort}."
