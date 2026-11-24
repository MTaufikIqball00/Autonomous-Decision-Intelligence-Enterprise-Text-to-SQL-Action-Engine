# ==============================================================================
# DATA-TO-ACTION: Service Status & Health Check (status.ps1)
# ==============================================================================

$ErrorActionPreference = "Continue"
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$LocalDir = Join-Path $RootDir ".local"
$PidDir = Join-Path $LocalDir "pids"
$EnvFile = Join-Path $RootDir ".env"

$PgHost = "127.0.0.1"; $PgPort = 5439
$BackendHost = "127.0.0.1"; $BackendPort = 8420
$FrontendHost = "127.0.0.1"; $FrontendPort = 3420

if (Test-Path $EnvFile) {
    foreach ($line in Get-Content $EnvFile) {
        $t = $line.Trim()
        if ([string]::IsNullOrWhiteSpace($t) -or $t.StartsWith("#")) { continue }
        $idx = $t.IndexOf("=")
        if ($idx -gt 0) {
            $k = $t.Substring(0, $idx).Trim()
            $v = $t.Substring($idx + 1).Trim()
            if ($k -eq "DTA_PG_HOST" -and $v) { $PgHost = $v }
            if ($k -eq "DTA_PG_PORT" -and $v) { $PgPort = [int]$v }
            if ($k -eq "DTA_BACKEND_HOST" -and $v) { $BackendHost = $v }
            if ($k -eq "DTA_BACKEND_PORT" -and $v) { $BackendPort = [int]$v }
            if ($k -eq "DTA_FRONTEND_HOST" -and $v) { $FrontendHost = $v }
            if ($k -eq "DTA_FRONTEND_PORT" -and $v) { $FrontendPort = [int]$v }
        }
    }
}

$PgIsReadyExe = Join-Path $LocalDir "postgres\bin\pg_isready.exe"
$pgStatus = "STOPPED"
if (Test-Path $PgIsReadyExe) {
    & $PgIsReadyExe -h $PgHost -p $PgPort -U postgres > $null 2>&1
    if ($LASTEXITCODE -eq 0) { $pgStatus = "HEALTHY" }
}

$beStatus = "STOPPED"
try {
    $r = Invoke-RestMethod -Uri "http://${BackendHost}:${BackendPort}/api/v1/health" -TimeoutSec 2
    if ($r.status -eq "ok") { $beStatus = "HEALTHY" }
} catch {}

$feStatus = "STOPPED"
try {
    $r = Invoke-WebRequest -Uri "http://${FrontendHost}:${FrontendPort}/" -UseBasicParsing -TimeoutSec 2
    if ($r.StatusCode -eq 200) { $feStatus = "HEALTHY" }
} catch {}

$rows = @(
    [PSCustomObject]@{ Service = "PostgreSQL 17.12.0"; Address = "${PgHost}:${PgPort}"; Status = $pgStatus; Exposure = "Loopback Internal"; Url = "postgres://${PgHost}:${PgPort}" },
    [PSCustomObject]@{ Service = "Rust Axum Backend"; Address = "${BackendHost}:${BackendPort}"; Status = $beStatus; Exposure = "Loopback Local"; Url = "http://${BackendHost}:${BackendPort}/api/v1/health" },
    [PSCustomObject]@{ Service = "Next.js Tailwind UI"; Address = "${FrontendHost}:${FrontendPort}"; Status = $feStatus; Exposure = "Loopback Local"; Url = "http://${FrontendHost}:${FrontendPort}" }
)

$rows | Format-Table -AutoSize
Write-Output "Logs directory: $(Join-Path $LocalDir 'logs')"
