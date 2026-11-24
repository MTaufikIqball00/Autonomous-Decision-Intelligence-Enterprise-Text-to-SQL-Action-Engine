# ==============================================================================
# DATA-TO-ACTION: Windows PowerShell Enterprise Launcher (start.ps1)
# - Validates toolchain dependencies & .env configuration
# - Verifies isolated port availability (5439, 8420, 3420)
# - Prevents duplicate process spawning via .local/pids/*.pid
# - Starts PostgreSQL 17.12.0, Rust Axum Backend, and Next.js Frontend
# - Captures stdout/stderr logs in .local/logs/
# ==============================================================================

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$LocalDir = Join-Path $RootDir ".local"
$PidDir = Join-Path $LocalDir "pids"
$LogDir = Join-Path $LocalDir "logs"
$EnvFile = Join-Path $RootDir ".env"

New-Item -ItemType Directory -Force -Path $PidDir, $LogDir | Out-Null

function Read-DotEnv([string]$Path) {
    $map = @{}
    foreach ($line in Get-Content $Path) {
        $t = $line.Trim()
        if ([string]::IsNullOrWhiteSpace($t) -or $t.StartsWith("#")) { continue }
        $idx = $t.IndexOf("=")
        if ($idx -gt 0) {
            $map[$t.Substring(0, $idx).Trim()] = $t.Substring($idx + 1).Trim()
        }
    }
    return $map
}

function Test-PidAlive([string]$PidFile) {
    if (-not (Test-Path $PidFile)) { return $null }
    $raw = (Get-Content $PidFile -Raw).Trim()
    $procId = 0
    if ([int]::TryParse($raw, [ref]$procId)) {
        $p = Get-Process -Id $procId -ErrorAction SilentlyContinue
        if ($p -and -not $p.HasExited) { return $p }
    }
    Remove-Item -Force $PidFile -ErrorAction SilentlyContinue
    return $null
}

function Test-PortInUse([int]$Port) {
    $conn = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
    return $conn
}

# 1. Dependency checks
foreach ($cmd in @("cargo", "node", "npm")) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        throw "Required dependency '$cmd' not found on PATH."
    }
}

if (-not (Test-Path $EnvFile) -or -not (Test-Path (Join-Path $LocalDir "pgdata\PG_VERSION"))) {
    Write-Host "[launcher] Bootstrapping portable PostgreSQL & .env via setup-postgres.ps1..."
    & (Join-Path $PSScriptRoot "setup-postgres.ps1")
}

$envMap = Read-DotEnv $EnvFile
foreach ($reqKey in @("DTA_APP_DB_PASSWORD", "DTA_READONLY_DB_PASSWORD", "DTA_JWT_SECRET", "DTA_MASTER_KEY")) {
    if ([string]::IsNullOrWhiteSpace($envMap[$reqKey])) {
        throw "Required configuration variable $reqKey is missing in .env"
    }
}

$PgHost = if ($envMap["DTA_PG_HOST"]) { $envMap["DTA_PG_HOST"] } else { "127.0.0.1" }
$PgPort = if ($envMap["DTA_PG_PORT"]) { [int]$envMap["DTA_PG_PORT"] } else { 5439 }
$BackendHost = if ($envMap["DTA_BACKEND_HOST"]) { $envMap["DTA_BACKEND_HOST"] } else { "127.0.0.1" }
$BackendPort = if ($envMap["DTA_BACKEND_PORT"]) { [int]$envMap["DTA_BACKEND_PORT"] } else { 8420 }
$FrontendHost = if ($envMap["DTA_FRONTEND_HOST"]) { $envMap["DTA_FRONTEND_HOST"] } else { "127.0.0.1" }
$FrontendPort = if ($envMap["DTA_FRONTEND_PORT"]) { [int]$envMap["DTA_FRONTEND_PORT"] } else { 3420 }

# 2. Ensure PostgreSQL is running on isolated port
$PgDir = Join-Path $LocalDir "postgres"
$PostgresExe = Join-Path $PgDir "bin\postgres.exe"
$PgIsReadyExe = Join-Path $PgDir "bin\pg_isready.exe"
$PgDataDir = Join-Path $LocalDir "pgdata"
$pgPidFile = Join-Path $PidDir "postgres.pid"

& $PgIsReadyExe -h $PgHost -p $PgPort -U postgres > $null 2>&1
if ($LASTEXITCODE -ne 0) {
    $busy = Test-PortInUse $PgPort
    if ($busy) {
        throw "Port $PgPort is already occupied by PID $($busy[0].OwningProcess)."
    }
    Write-Host "[launcher] Starting PostgreSQL on ${PgHost}:${PgPort}..."
    $pgOut = Join-Path $LogDir "postgres.log"
    $pgErr = Join-Path $LogDir "postgres.err.log"
    $pgProc = Start-Process -FilePath $PostgresExe `
        -ArgumentList @("-D", "`"$PgDataDir`"", "-h", $PgHost, "-p", "$PgPort") `
        -WindowStyle Hidden -RedirectStandardOutput $pgOut -RedirectStandardError $pgErr -PassThru
    Set-Content -Path $pgPidFile -Value $pgProc.Id
    $pgReady = $false
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 500
        & $PgIsReadyExe -h $PgHost -p $PgPort -U postgres > $null 2>&1
        if ($LASTEXITCODE -eq 0) { $pgReady = $true; break }
        if ($pgProc.HasExited) { throw "PostgreSQL failed to start. See $pgErr" }
    }
    if (-not $pgReady) { throw "PostgreSQL did not respond within 15s." }
} else {
    Write-Host "[launcher] PostgreSQL already healthy on ${PgHost}:${PgPort}."
}

# 3. Build & start Rust Backend
$backendPidFile = Join-Path $PidDir "backend.pid"
$existingBackend = Test-PidAlive $backendPidFile
$BackendDir = Join-Path $RootDir "backend"
$ReleaseExe = Join-Path $BackendDir "target\release\dta_backend.exe"
$DebugExe = Join-Path $BackendDir "target\debug\dta_backend.exe"

if (-not $existingBackend) {
    $busy = Test-PortInUse $BackendPort
    if ($busy) {
        throw "Backend port $BackendPort is already occupied by PID $($busy[0].OwningProcess)."
    }
    $backendExe = $null
    if (Test-Path $ReleaseExe) {
        $backendExe = $ReleaseExe
    } elseif (Test-Path $DebugExe) {
        $backendExe = $DebugExe
    } else {
        Write-Host "[launcher] Building Rust backend (release)..."
        Push-Location $BackendDir
        try {
            & cargo build --release
            if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed." }
        } finally {
            Pop-Location
        }
        $backendExe = $ReleaseExe
    }

    Write-Host "[launcher] Starting Rust backend on http://${BackendHost}:${BackendPort}..."
    $beOut = Join-Path $LogDir "backend.stdout.log"
    $beErr = Join-Path $LogDir "backend.stderr.log"
    $beProc = Start-Process -FilePath $backendExe `
        -WorkingDirectory $RootDir `
        -WindowStyle Hidden -RedirectStandardOutput $beOut -RedirectStandardError $beErr -PassThru
    Set-Content -Path $backendPidFile -Value $beProc.Id

    $healthUrl = "http://${BackendHost}:${BackendPort}/api/v1/health"
    $beReady = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        if ($beProc.HasExited) {
            throw "Rust backend exited early (code=$($beProc.ExitCode)). Check $beErr"
        }
        try {
            $resp = Invoke-RestMethod -Uri $healthUrl -TimeoutSec 2
            if ($resp.status -eq "ok") { $beReady = $true; break }
        } catch {}
    }
    if (-not $beReady) {
        throw "Rust backend failed health check at $healthUrl. Check $beOut and $beErr"
    }
} else {
    Write-Host "[launcher] Rust backend already running (PID $($existingBackend.Id))."
}

# 4. Build & start Next.js Frontend
$frontendPidFile = Join-Path $PidDir "frontend.pid"
$existingFrontend = Test-PidAlive $frontendPidFile
$FrontendDir = Join-Path $RootDir "frontend"
$NextBin = Join-Path $FrontendDir "node_modules\next\dist\bin\next"

if (-not $existingFrontend) {
    $busy = Test-PortInUse $FrontendPort
    if ($busy) {
        throw "Frontend port $FrontendPort is already occupied by PID $($busy[0].OwningProcess)."
    }
    if (-not (Test-Path $NextBin)) {
        Write-Host "[launcher] Installing frontend packages..."
        Push-Location $FrontendDir
        try {
            & npm install
            if ($LASTEXITCODE -ne 0) { throw "npm install failed." }
        } finally {
            Pop-Location
        }
    }
    if (-not (Test-Path (Join-Path $FrontendDir ".next\BUILD_ID"))) {
        Write-Host "[launcher] Building Next.js production bundle..."
        Push-Location $FrontendDir
        try {
            $env:NEXT_PUBLIC_API_URL = "http://${BackendHost}:${BackendPort}"
            & npm run build
            if ($LASTEXITCODE -ne 0) { throw "npm run build failed." }
        } finally {
            Pop-Location
        }
    }

    Write-Host "[launcher] Starting Next.js frontend on http://${FrontendHost}:${FrontendPort}..."
    $feOut = Join-Path $LogDir "frontend.stdout.log"
    $feErr = Join-Path $LogDir "frontend.stderr.log"
    $nodeExe = (Get-Command node).Source
    $feProc = Start-Process -FilePath $nodeExe `
        -ArgumentList @("`"$NextBin`"", "start", "-H", $FrontendHost, "-p", "$FrontendPort") `
        -WorkingDirectory $FrontendDir `
        -WindowStyle Hidden -RedirectStandardOutput $feOut -RedirectStandardError $feErr -PassThru
    Set-Content -Path $frontendPidFile -Value $feProc.Id

    $feUrl = "http://${FrontendHost}:${FrontendPort}/"
    $feReady = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        if ($feProc.HasExited) {
            throw "Frontend exited early (code=$($feProc.ExitCode)). Check $feErr"
        }
        try {
            $r = Invoke-WebRequest -Uri $feUrl -UseBasicParsing -TimeoutSec 2
            if ($r.StatusCode -eq 200) { $feReady = $true; break }
        } catch {}
    }
    if (-not $feReady) {
        throw "Next.js frontend did not become ready at $feUrl. Check $feErr"
    }
} else {
    Write-Host "[launcher] Next.js frontend already running (PID $($existingFrontend.Id))."
}

Write-Host "`n=== DATA-TO-ACTION SERVICES STARTED ==="
& (Join-Path $PSScriptRoot "status.ps1")
