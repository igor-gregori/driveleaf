# M0 engine comparison on a synthetic volume. Needs no elevation, because every
# engine reads an ordinary file. The file stays in the OS cache after it is
# written, so this measures parsing, not the disk.
param(
    [int]$Records = 3000000,
    [int]$Runs = 3,
    [string]$Folder = $PSScriptRoot
)

$ErrorActionPreference = 'Stop'
$cli = Join-Path $PSScriptRoot 'driveleaf-cli.exe'
$compare = Join-Path $PSScriptRoot 'engine-compare.exe'
$image = Join-Path $Folder 'synth.img'
$dump = Join-Path $Folder 'synth.mft'
$results = Join-Path $Folder 'm0-synth.json'

function Invoke-Json([string]$Exe, [string[]]$Arguments) {
    (& $Exe @Arguments | Out-String) | ConvertFrom-Json
}

$generated = Measure-Command { & $cli synth $image $Records 1 | Out-Null }
Write-Host "image written in $([math]::Round($generated.TotalSeconds, 1)) s"

# The mft crate reads a bare $MFT, which starts at cluster 64 of this image.
$source = [IO.File]::OpenRead($image)
$target = [IO.File]::Create($dump)
[void]$source.Seek(64 * 4096, 'Begin')
$buffer = New-Object byte[] (4MB)
$left = [int64]$Records * 1024
while ($left -gt 0) {
    $read = $source.Read($buffer, 0, [int][math]::Min([int64]$buffer.Length, $left))
    $target.Write($buffer, 0, $read)
    $left -= $read
}
$source.Close(); $target.Close()

$all = New-Object System.Collections.ArrayList
for ($run = 1; $run -le $Runs; $run++) {
    [void]$all.Add((Invoke-Json $cli @('mft', $image)))
    [void]$all.Add((Invoke-Json $compare @('ntfs-reader', $image)))
    [void]$all.Add((Invoke-Json $compare @('ntfs-reader-scan', $image)))
    Write-Host "run $run done"
}
[void]$all.Add((Invoke-Json $compare @('mft', $dump)))

$all | ConvertTo-Json -Depth 4 | Set-Content $results -Encoding utf8
Remove-Item $image, $dump
$all | Select-Object engine, @{ n = 'seconds'; e = { [math]::Round($_.elapsed_ms / 1000, 3) } },
    files, directories, names, @{ n = 'peak_mb'; e = { [math]::Round($_.peak_memory_bytes / 1MB) } },
    @{ n = 'engine_mb'; e = { [math]::Round($_.engine_memory_bytes / 1MB) } } | Format-Table -AutoSize
