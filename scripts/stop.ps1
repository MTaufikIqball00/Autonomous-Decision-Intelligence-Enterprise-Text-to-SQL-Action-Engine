# ==============================================================================
# DATA-TO-ACTION: Clean Shutdown Launcher (stop.ps1)
# ==============================================================================

$ErrorActionPreference = "Continue"

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$LocalDir = Join-Path $RootDir ".local"
$PidDir = Join-Path $LocalDir "pids"
$PgCtlExe = Join-Path $LocalDir "postgres\bin\pg_ctl.exe"
$PgDataDir = Join-Path $LocalDir "pgdata"

function Stop-ByPidFile([string]$Name, [string]$PidFile) {
    if (Test-Path $PidFile) {
        $raw = (Get-Content $PidFile -Raw).Trim()
        $procId = 0
        if ([int]::TryParse($raw, [ref]$procId)) {
            $p = Get-Process -Id $procId -ErrorAction SilentlyContinue
            if ($p -and -not $p.HasExited) {
                Write-Host "[stop] Stopping $Name (PID $procId)..."
                Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue
            }
        }
        Remove-Item -Force $PidFile -ErrorAction SilentlyContinue
    }
}

Stop-ByPidFile "frontend" (Join-Path $PidDir "frontend.pid")
Stop-ByPidFile "backend" (Join-Path $PidDir "backend.pid")

if ((Test-Path $PgCtlExe) -and (Test-Path (Join-Path $PgDataDir "postmaster.pid"))) {
    Write-Host "[stop] Stopping PostgreSQL cleanly..."
    & $PgCtlExe -D "`"$PgDataDir`"" -m fast -w stop | Out-Null
}
Stop-ByPidFile "postgres" (Join-Path $PidDir "postgres.pid")

Write-Host "[stop] All DATA-TO-ACTION services stopped."
