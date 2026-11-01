# Runs the M0 interface spike and samples the memory of the whole process tree
# (the app plus its WebView2 processes). No elevation needed.
param(
    [string]$Exe = (Join-Path $PSScriptRoot 'driveleaf.exe'),
    [string]$Report = (Join-Path $PSScriptRoot 'spike-ui.json'),
    [string]$Screenshot = (Join-Path $PSScriptRoot 'spike-ui.png'),
    [int]$TimeoutSeconds = 120
)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SpikeWin32 {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
'@

function Get-TreeMemory([int]$RootId) {
    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, WorkingSetSize, PrivatePageCount, Name
    $byParent = $all | Group-Object ParentProcessId -AsHashTable -AsString
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($RootId)
    $tree = @()
    while ($queue.Count -gt 0) {
        $id = $queue.Dequeue()
        $tree += $all | Where-Object ProcessId -EQ $id
        if ($byParent -and $byParent.ContainsKey("$id")) {
            foreach ($child in $byParent["$id"]) { $queue.Enqueue([int]$child.ProcessId) }
        }
    }
    [pscustomobject]@{
        Processes = $tree.Count
        WorkingSetMb = [math]::Round((($tree | Measure-Object WorkingSetSize -Sum).Sum) / 1MB, 1)
        PrivateMb = [math]::Round((($tree | Measure-Object PrivatePageCount -Sum).Sum) / 1MB, 1)
        App = ($tree | Where-Object ProcessId -EQ $RootId | ForEach-Object { [math]::Round($_.PrivatePageCount / 1MB, 1) })
    }
}

Remove-Item $Report, $Screenshot -ErrorAction SilentlyContinue
$env:DRIVELEAF_SPIKE_OUT = $Report
$process = Start-Process $Exe -PassThru
$started = Get-Date
$idle = $null
$peak = [pscustomobject]@{ WorkingSetMb = 0; PrivateMb = 0 }
$shot = $false

while (-not $process.HasExited) {
    $elapsed = ((Get-Date) - $started).TotalSeconds
    if ($elapsed -gt $TimeoutSeconds) { Stop-Process $process -Force; throw 'spike timed out' }
    try {
        $memory = Get-TreeMemory $process.Id
        if ($null -eq $idle -and $elapsed -gt 1.5) { $idle = $memory }
        if ($memory.WorkingSetMb -gt $peak.WorkingSetMb) { $peak = $memory }
    } catch { }
    if (-not $shot -and $elapsed -gt 7) {
        $process.Refresh()
        $rect = New-Object SpikeWin32+RECT
        if ($process.MainWindowHandle -ne 0 -and [SpikeWin32]::GetWindowRect($process.MainWindowHandle, [ref]$rect)) {
            try {
                $size = New-Object System.Drawing.Size(($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
                $bitmap = New-Object System.Drawing.Bitmap($size.Width, $size.Height)
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $size)
                $bitmap.Save($Screenshot, [System.Drawing.Imaging.ImageFormat]::Png)
                $graphics.Dispose(); $bitmap.Dispose()
                $shot = $true
            } catch { $shot = $true; Write-Warning "screenshot failed: $_" }
        }
    }
    Start-Sleep -Milliseconds 300
}

[pscustomobject]@{
    ExitCode = $process.ExitCode
    Seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
    IdleWorkingSetMb = $idle.WorkingSetMb
    IdlePrivateMb = $idle.PrivateMb
    PeakWorkingSetMb = $peak.WorkingSetMb
    PeakPrivateMb = $peak.PrivateMb
    PeakProcesses = $peak.Processes
    PeakAppPrivateMb = $peak.App
} | Format-List
