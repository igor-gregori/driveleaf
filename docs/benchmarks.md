# Benchmarks and accuracy

Measured in November 2026 on one machine: Ryzen 7 9800X3D (16 threads), 47 GB RAM,
Radeon RX 9070 XT, Lexar NM1090 NVMe, Windows 11 build 26300, 180 Hz display. That is
a fast desktop, so read these as best-case numbers.

## Targets for 0.1

| Target | Result | Status |
|---|---|---|
| Scan time within 20% of WizTree | Not compared yet | Open |
| Public reference: 0.6 M files in about 1.3 s on NVMe | 0.54 M files in 0.32 s | Beaten |
| Engine memory up to 150 bytes per entry | 71 to 115 bytes | Met |
| Allocated total within 1% of used space | −0.12% on a real 534 GB drive | Met |
| `.exe` up to 15 MB | 4.3 MB, and a 1.7 MB installer | Met |
| 60 FPS while zooming with 200,000 tiles | 180 FPS, the display's limit | Met |
| Navigation under 100 ms at 5 M files | 21 to 40 ms per query | Met |
| No freezing during a scan | The scan runs on its own threads | Met |

## Scanning a real drive

The C: drive, 534 GB used, 711,933 records and 544,163 files, read as administrator.
Each engine ran 3 times, and the workload was the same: decode every in-use record,
count, read the name and sum the sizes. Reproduce with `scripts/m0-bench.ps1`.

| Engine | Median | First run | Peak memory |
|---|---|---|---|
| **Driveleaf** | **0.316 s** | 0.317 s | 138 MB |
| `ntfs-reader` 0.6.0, `Mft` | 0.948 s | 1.627 s | 950 MB |
| `ntfs-reader` 0.6.0, `MftScan` | 1.391 s | 1.388 s | 118 MB |

- Driveleaf read the MFT on 8 threads, which is the default on flash storage. On one
  thread the median was 0.394 s, so reading in parallel saved 20%. The disk still sets
  the pace, since about 270 ms of the 316 ms go to reading.
- All 754,844 records parsed, with none corrupt, malformed or unreadable.
- Whichever engine runs first pays about 700 ms, once. It hit Driveleaf in an earlier
  round, and `ntfs-reader` in this one. The cause is unknown.
- None of this was right after a reboot. Driveleaf reads bypass the file cache, so
  its runs stay comparable, but a true cold start is still to be measured.

### The allocated total against the disk

| Source | Bytes | Difference |
|---|---|---|
| Sum of everything in the MFT | 573,468,119,040 | |
| Used clusters, according to NTFS | 574,166,876,160 | −0.12% |
| Used space, according to `Get-Volume` | 577,754,071,040 | −0.74% |

`Get-Volume` counts space Windows reserves for itself, which is not in any file.

The remaining 666 MiB belong to no record. Adding up the data runs of every attribute
of every record gives the same total as the scanner, one attribute type at a time,
and no cluster is claimed twice. The gap was the same to the byte in rounds with 4 GB
of changes between them, so it is not writes waiting to reach the disk. What holds
those clusters is unknown.

### The MFT against a folder walk

The same drive read twice: a capture of the MFT, then a walk through the documented
folder API. 472,215 files appear in both.

- 472,101 have the same size. Of the other 114, 53 changed between the two readings,
  since their dates differ too. The remaining 61 are mostly `.etl` and `.log` files,
  and the listing shows 0 bytes for 48 of them where the MFT has the real size.
- 374,524 have the same size on disk. Another 97,008 fit inside their MFT record and
  use no clusters, so the MFT says 0 and the listing gives a few hundred bytes. The
  MFT also adds up every stream of a file, and the listing only the main one.
- The MFT holds 56,504 more paths, nearly all in folders the walk had no permission
  to open, such as `WindowsApps` and parts of `System32`.

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

`fixtures/build.ps1` builds the same kind of test from a 64 MB volume written by
Windows, with WOF compression, junctions and symbolic links:

| Check | Result |
|---|---|
| Files and folders Windows lists | 571, all found with their kind and size |
| Hard links, compression, sparse files, streams, links, metafiles | All match |
| Allocated total against the volume's `$Bitmap` | 11,948,032 bytes each |

`Get-Volume` reports 4 MiB more on this volume, in no file and outside the bitmap.

Building it needs administrator rights. The image records the security identifier of
the account that built it, so it stays out of the repository and CI builds its own.

## The model and the queries

`driveleaf-cli bench <target>` times what the interface asks for.

| | 5 M entries (synthetic) | 795,000 entries (real C:, MFT) | 739,000 entries (real C:, folder walk) |
|---|---|---|---|
| Scan and build the tree | 628 ms | 132 ms | 3,494 ms |
| Memory per entry | 71 bytes | 84 bytes | 115 bytes |
| Children of a folder, sorted by name | 23 ms | 0.01 ms | 0.01 ms |
| Rank every file by size | 40 ms | 4.7 ms | 4.9 ms |
| Space per file type | 21 ms | 3.7 ms | 2.8 ms |
| Search all names | 25 ms | 7.6 ms | 6.6 ms |
| Treemap of the root | see below | 1.7 ms, 15,277 tiles | 1.5 ms, 14,749 tiles |

The MFT column replays a capture of the drive, so its 132 ms leave out the disk. The
synthetic tree is a long chain of folders, which makes its treemap unrealistically
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
