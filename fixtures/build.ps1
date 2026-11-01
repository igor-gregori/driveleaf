# Builds a real NTFS volume written by Windows itself, with the exactness cases
# that only Windows can produce, and saves it as a compressed image plus a
# manifest of what it must contain. Needs administrator rights, because it
# formats a virtual disk; using the result does not.
#
# It touches nothing but its own temporary virtual disk.
param(
    [string]$OutDir = $PSScriptRoot,
    [string]$Letter = '',
    [int]$SizeMb = 64
)

$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $identity.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'build.ps1 needs administrator rights'
}
if (-not $Letter) {
    $used = (Get-Volume | Where-Object DriveLetter).DriveLetter
    $Letter = 'XWVUTSRQ'.ToCharArray() | Where-Object { $used -notcontains $_ } | Select-Object -First 1
}

$vhd = Join-Path $env:TEMP "driveleaf-fixture-$PID.vhd"

function Invoke-Diskpart([string[]]$Lines) {
    $script = Join-Path $env:TEMP "driveleaf-diskpart-$PID.txt"
    Set-Content $script $Lines -Encoding ascii
    $output = & diskpart /s $script
    Remove-Item $script
    if ($LASTEXITCODE -ne 0) { throw "diskpart failed:`n$output" }
}

Invoke-Diskpart @(
    "create vdisk file=`"$vhd`" maximum=$SizeMb type=fixed",
    "select vdisk file=`"$vhd`"",
    'attach vdisk',
    'create partition primary',
    'format fs=ntfs quick label=driveleaf',
    "assign letter=$Letter"
)

try {
    $root = "${Letter}:\"
    $bytes = { param($count) New-Object byte[] $count }

    # Plain files of every size class, nested in folders.
    New-Item -ItemType Directory "$root\docs\deep\deeper" | Out-Null
    [IO.File]::WriteAllBytes("$root\docs\tiny.txt", (& $bytes 100))
    [IO.File]::WriteAllBytes("$root\docs\small.bin", (& $bytes 5000))
    [IO.File]::WriteAllBytes("$root\docs\deep\medium.bin", (& $bytes 300000))
    # Built from code points, so the name survives however this file is read.
    $unicode = "relat$([char]0xF3)rio $([char]::ConvertFromUtf32(0x1F33F)).txt"
    [IO.File]::WriteAllBytes("$root\docs\deep\deeper\$unicode", (& $bytes 1234))
    for ($index = 0; $index -lt 500; $index++) {
        [IO.File]::WriteAllBytes("$root\docs\deep\many-$index.dat", (& $bytes 2000))
    }

    # Hard links: three names, one file.
    New-Item -ItemType Directory "$root\links" | Out-Null
    [IO.File]::WriteAllBytes("$root\links\original.bin", (& $bytes 123456))
    New-Item -ItemType HardLink -Path "$root\links\second.bin" -Value "$root\links\original.bin" | Out-Null
    New-Item -ItemType HardLink -Path "$root\docs\third.bin" -Value "$root\links\original.bin" | Out-Null

    # Sparse: a big logical size over almost no clusters.
    New-Item -ItemType Directory "$root\sparse" | Out-Null
    $stream = [IO.File]::Create("$root\sparse\hole.bin")
    $stream.Close()
    & fsutil sparse setflag "$root\sparse\hole.bin" | Out-Null
    $stream = [IO.File]::OpenWrite("$root\sparse\hole.bin")
    $stream.SetLength(20MB)
    $stream.Seek(19MB, 'Begin') | Out-Null
    $stream.Write((& $bytes 4096), 0, 4096)
    $stream.Close()

    # Compression, the classic NTFS kind and the WOF kind CompactOS uses.
    New-Item -ItemType Directory "$root\compressed" | Out-Null
    $text = [Text.Encoding]::ASCII.GetBytes(('compress me ' * 80000))
    [IO.File]::WriteAllBytes("$root\compressed\ntfs.txt", $text)
    [IO.File]::WriteAllBytes("$root\compressed\wof.txt", $text)
    & compact /c "$root\compressed\ntfs.txt" | Out-Null
    & compact /c /exe:lzx "$root\compressed\wof.txt" | Out-Null

    # Alternate data streams.
    New-Item -ItemType Directory "$root\streams" | Out-Null
    Set-Content "$root\streams\carrier.txt" 'main stream'
    Set-Content "$root\streams\carrier.txt" -Stream 'Zone.Identifier' '[ZoneTransfer]'
    Set-Content "$root\streams\carrier.txt" -Stream 'big' ('y' * 200000)

    # Reparse points, which must never be followed.
    New-Item -ItemType Directory "$root\target" | Out-Null
    [IO.File]::WriteAllBytes("$root\target\inside.bin", (& $bytes 70000))
    New-Item -ItemType Junction -Path "$root\junction" -Value "$root\target" | Out-Null
    New-Item -ItemType SymbolicLink -Path "$root\symlink" -Value "$root\target" | Out-Null

    # Freed records, which the bitmap must skip.
    New-Item -ItemType Directory "$root\deleted" | Out-Null
    for ($index = 0; $index -lt 100; $index++) {
        [IO.File]::WriteAllBytes("$root\deleted\gone-$index.bin", (& $bytes 3000))
    }
    for ($index = 0; $index -lt 100; $index += 2) {
        Remove-Item "$root\deleted\gone-$index.bin"
    }

    # The manifest is what the slow, documented way sees.
    $entries = Get-ChildItem $root -Recurse -Force -Attributes !ReparsePoint -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch 'System Volume Information' } |
        ForEach-Object {
            [ordered]@{
                path = $_.FullName.Substring(3)
                directory = $_.PSIsContainer
                size = if ($_.PSIsContainer) { 0 } else { $_.Length }
            }
        }
    $volume = Get-Volume -DriveLetter $Letter
    $manifest = [ordered]@{
        used_bytes = $volume.Size - $volume.SizeRemaining
        entries = @($entries)
    }
    [IO.File]::WriteAllText(
        (Join-Path $OutDir 'windows-ntfs.manifest.json'),
        ($manifest | ConvertTo-Json -Depth 4 -Compress),
        (New-Object Text.UTF8Encoding $false)
    )

    # Flushes everything, so the raw copy below is a consistent volume.
    Write-VolumeCache -DriveLetter $Letter

    # The volume itself, without the virtual disk around it.
    $source = [IO.File]::Open("\\.\${Letter}:", 'Open', 'Read', 'ReadWrite')
    $target = [IO.File]::Create((Join-Path $OutDir 'windows-ntfs.img.gz'))
    $gzip = New-Object IO.Compression.GZipStream($target, [IO.Compression.CompressionLevel]::Optimal)
    $buffer = New-Object byte[] (1MB)
    $left = $volume.Size
    while ($left -gt 0) {
        $wanted = [int][math]::Min([int64]$buffer.Length, $left)
        $read = $source.Read($buffer, 0, $wanted)
        if ($read -eq 0) { break }
        $gzip.Write($buffer, 0, $read)
        $left -= $read
    }
    $gzip.Close(); $target.Close(); $source.Close()
} finally {
    Invoke-Diskpart @("select vdisk file=`"$vhd`"", 'detach vdisk')
    Remove-Item $vhd -ErrorAction SilentlyContinue
}
Get-Item (Join-Path $OutDir 'windows-ntfs.*') | Select-Object Name, Length | Format-Table -AutoSize
