# M0 engine spike on a real volume. Only reads. Asks for elevation once, because
# Windows only lets administrators open a raw volume.
#
# For a true first-scan number, run it right after a reboot. The Driveleaf reads
# bypass the file cache, so its runs stay comparable anyway.
param(
    [string]$Drive = 'C',
    [int]$Runs = 3,
    [string]$WizTree = ''
)

$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"",
        '-Drive', $Drive, '-Runs', $Runs)
    if ($WizTree) { $arguments += @('-WizTree', "`"$WizTree`"") }
    Start-Process powershell -Verb RunAs -ArgumentList $arguments -Wait
    return
}

$cli = Join-Path $PSScriptRoot 'driveleaf-cli.exe'
$compare = Join-Path $PSScriptRoot 'engine-compare.exe'
$results = Join-Path $PSScriptRoot 'm0-bench.json'
$device = "\\.\${Drive}:"

function Invoke-Json([string]$Exe, [string[]]$Arguments) {
    (& $Exe @Arguments | Out-String) | ConvertFrom-Json
}

$all = New-Object System.Collections.ArrayList
for ($run = 1; $run -le $Runs; $run++) {
    Write-Host "run $run of $Runs"
    # ntfs-reader goes first so a one-off cost of the first run shows on it.
    [void]$all.Add((Invoke-Json $compare @('ntfs-reader', $device)))
    [void]$all.Add((Invoke-Json $cli @('mft', "${Drive}:")))
    [void]$all.Add((Invoke-Json $compare @('ntfs-reader-scan', $device)))
}

# A capture of what the scan reads lets the tree be tested without elevation.
# It holds every file name on the volume and stays on this machine.
$capture = Join-Path $PSScriptRoot "${Drive}-drive.dlmft"
& $cli capture "${Drive}:" $capture | Out-Null

# Administrator rights are here anyway, so build the volume only Windows can write.
try {
    & (Join-Path $PSScriptRoot 'build.ps1') -OutDir $PSScriptRoot
} catch {
    Write-Warning "The Windows fixture was not built: $_"
}

$volume = Get-Volume -DriveLetter $Drive
$summary = [ordered]@{
    measured = (Get-Date).ToString('s')
    volume_size_bytes = $volume.Size
    volume_used_bytes = $volume.Size - $volume.SizeRemaining
    runs = $all
}
if ($WizTree) {
    # Optional: WizTree portable, used only as a timing and totals reference.
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $csv = Join-Path $env:TEMP 'driveleaf-wiztree.csv'
    Start-Process $WizTree -ArgumentList "`"${Drive}:`" /export=`"$csv`" /admin=1 /exportfolders=0" -Wait
    $summary.wiztree_seconds = $watch.Elapsed.TotalSeconds
    Remove-Item $csv -ErrorAction SilentlyContinue
}
$summary | ConvertTo-Json -Depth 5 | Set-Content $results -Encoding utf8

$all | Select-Object engine, @{ n = 'seconds'; e = { [math]::Round($_.elapsed_ms / 1000, 3) } },
    files, directories, @{ n = 'allocated_gb'; e = { [math]::Round($_.allocated_bytes / 1GB, 2) } },
    allocated_vs_used_percent, corrupt_records, malformed_records,
    @{ n = 'peak_mb'; e = { [math]::Round($_.peak_memory_bytes / 1MB) } } | Format-Table -AutoSize
Write-Host "Saved to $results"
Read-Host 'Press Enter to close'
