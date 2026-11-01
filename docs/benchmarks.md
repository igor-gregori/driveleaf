# Benchmarks and accuracy

Measured in November 2026 on one machine: Ryzen 7 9800X3D (16 threads), 47 GB RAM,
Radeon RX 9070 XT, Lexar NM1090 NVMe, Windows 11 build 26300, 180 Hz display. That is
a fast desktop, so read these as best-case numbers.

## Targets for 0.1

| Target | Result | Status |
|---|---|---|
| Scan time within 20% of WizTree | Not compared yet | Open |
| Public reference: 0.6 M files in about 1.3 s on NVMe | 0.54 M files in 0.39 s | Beaten |
| Engine memory up to 150 bytes per entry | 71 to 115 bytes | Met |
| Allocated total within 1% of used space | −0.12% on a real 533 GB drive | Met |
| `.exe` up to 15 MB | 4.3 MB, and a 1.7 MB installer | Met |
| 60 FPS while zooming with 200,000 tiles | 180 FPS, the display's limit | Met |
| Navigation under 100 ms at 5 M files | 21 to 40 ms per query | Met |
| No freezing during a scan | The scan runs on its own threads | Met |

## Scanning a real drive

The C: drive, 533 GB used, 711,349 records and 543,673 files, read as administrator.
Each engine ran 3 times, and the workload was the same: decode every in-use record,
count, read the name and sum the sizes. Reproduce with `scripts/m0-bench.ps1`.

| Engine | Median | First run | Peak memory |
|---|---|---|---|
| **Driveleaf** | **0.394 s** | 1.125 s | 214 MB |
| `ntfs-reader` 0.6.0, `Mft` | 0.930 s | 0.930 s | 950 MB |
| `ntfs-reader` 0.6.0, `MftScan` | 1.371 s | 1.371 s | 118 MB |

- This run read the MFT on one thread. 352 ms of the 393 ms went to reading, so
  parsing kept up and the disk set the pace. Scans now read in parallel on flash
  storage, which has not been measured on a real volume yet.
- The first Driveleaf run took 730 ms longer, outside the read calls. The cause is
  still unknown. It happened only once, and `ntfs-reader` did not show it, though
  that engine ran second.
- None of this was right after a reboot. Driveleaf reads bypass the file cache, so
  its runs stay comparable, but a true cold start is still to be measured.

### The allocated total against the disk

| Source | Bytes | Difference |
|---|---|---|
| Sum of everything in the MFT | 569,518,313,472 | |
| Used clusters, according to NTFS | 570,217,070,592 | −0.12% |
| Used space, according to `Get-Volume` | 573,014,503,424 | −0.61% |

`Get-Volume` counts space Windows reserves for itself, which is not in any file.
The remaining 666 MiB are unexplained so far. Part of it may be recent changes not
yet written to disk, since the scan reads what is on disk, not what is in the cache.

## Speed against the candidate crates

Same 3,000,000-slot synthetic volume for every engine, from the OS file cache, so
this measures parsing and memory copies. Reproduce with `scripts/m0-synth.ps1`.

| Engine | Time | Peak memory | Kept in memory |
|---|---|---|---|
| **Driveleaf** | **0.377 s** | 375 MB | 275 MB |
| `mft` 0.9.1 | 1.110 s | 4 MB | Nothing |
| `ntfs-reader` 0.6.0, `MftScan` | 1.505 s | 78 MB | 47 MB |
| `ntfs-reader` 0.6.0, `Mft` | 1.978 s | 2,958 MB | 2,930 MB |

`mft` needs a dumped MFT and keeps no results, and `MftScan` keeps no names, so
neither one is doing the whole job that Driveleaf does here.

## Exactness on real NTFS volumes

`fixtures/ntfs3g/run.sh` formats an image with `mkntfs`, fills it through `ntfs-3g`,
and compares the parser with what the file system reports:

| Check | Result |
|---|---|
| Records matched one to one | 1,957 |
| Names checked | 2,359 |
| Extension records | 83 |
| Sizes, link counts, names, stream sizes, paths, folder totals | All match |
| Allocated total against volume used | 111,493,120 bytes each, 0.000% |

`fixtures/build.ps1` builds the same kind of test from a volume written by Windows,
with WOF compression, junctions and symbolic links. It needs administrator rights.

## The model and the queries

`driveleaf-cli bench <target>` times what the interface asks for.

| | 5 M entries (synthetic) | 739,000 entries (real C:, folder walk) |
|---|---|---|
| Scan and build the tree | 628 ms | 3,494 ms |
| Memory per entry | 71 bytes | 115 bytes |
| Children of a folder, sorted by name | 23 ms | 0.01 ms |
| Rank every file by size | 40 ms | 4.9 ms |
| Space per file type | 21 ms | 2.8 ms |
| Search all names | 25 ms | 6.6 ms |
| Treemap of the root | see below | 1.5 ms, 14,749 tiles |

The synthetic tree is a long chain of folders, which makes its treemap unrealistically
cheap, so that number is left out. Peak memory while scanning 5 M entries was 1.1 GB,
falling to 358 MB once the tree is built.

## Interface

200,000 synthetic tiles in a 1280×755 window, over `tauri::ipc::Response`.

| Tiles | Buffer | IPC (median) | GPU upload | Hit test (mean) | Full frame (mean) | Zoom FPS |
|---|---|---|---|---|---|---|
| 50,000 | 1.2 MB | 6.6 ms | 1.4 ms | 0.03 ms | 0.4 ms | 177 |
| 100,000 | 2.4 MB | 13.7 ms | 2.5 ms | 0.06 ms | 0.3 ms | 180 |
| 200,000 | 4.8 MB | 24.0 ms | 3.7 ms | 0.12 ms | 0.4 ms | 180 |

Memory of the whole process tree, with six WebView2 processes: 243 MB private and
428 MB working set when idle, 290 MB and 477 MB at peak.

## Not measured yet

- Against WizTree, on the same machine, after a reboot.
- WebGL2 without a GPU, in a VM or over RDP. The app falls back to fewer tiles and no
  animation there, which has only been checked by reading the code.
- ReFS, exFAT and network drives. The folder walk runs on them, but only NTFS has
  been tried.
- A real scan of several million files through the interface.

## Trademark screening

Searched on 2026-10-31. This is a screening, not legal advice.

| Register | "driveleaf" | "drive leaf" |
|---|---|---|
| USPTO | No results | No results |
| TMview (144 million marks, including EUIPO, USPTO and INPI) | No results | 3 unrelated marks |
| INPI, radical search, data to 2026-10-27 | No results | Not searched |

The three TMview hits are "DRIVE TO LESS and leaf design" (Canada, class 35,
registered), "GREEN LEAF DRIVERS." (US, class 9, cancelled) and a lawn care services
phrase (US, class 37, pending). None concerns disk software. A control search for
"nokia" returned 1,686 marks, so the empty results are real.
