# M0 results

Foundation and spikes, measured on 2026-11-01. The real-volume MFT scan is the
one number still missing, because it needs an elevated process.

**Test machine:** Ryzen 7 9800X3D (16 threads), 47 GB RAM, Radeon RX 9070 XT,
Lexar NM1090 NVMe, Windows 11 build 26300, 180 Hz display. This is a fast
desktop, so read these as best-case numbers.

## Interface spike

Synthetic tiles from a real squarified layout, moved over `tauri::ipc::Response`
and drawn with WebGL2 instancing, in a 1280×755 window. Reproduce with
`scripts/spike-ui.ps1`.

| Tiles | Buffer | Generate | IPC (median) | GPU upload | Hit test (mean) | Full frame (mean) | Zoom FPS | Worst frame |
|---|---|---|---|---|---|---|---|---|
| 50,000 | 1.2 MB | 1.4 ms | 6.6 ms | 1.4 ms | 0.03 ms | 0.4 ms | 177 | 94.5 ms |
| 100,000 | 2.4 MB | 2.8 ms | 13.7 ms | 2.5 ms | 0.06 ms | 0.3 ms | 180 | 5.9 ms |
| 200,000 | 4.8 MB | 5.7 ms | 24.0 ms | 3.7 ms | 0.12 ms | 0.4 ms | 180 | 10.7 ms |

- FPS is capped by the 180 Hz display. A full frame takes well under 1 ms.
- The 94.5 ms frame is a single stall at the very start of the run, before the
  shaders warm up. It goes unnoticed in normal use, but M2 should watch it.
- Memory of the process tree (app and six WebView2 processes): 243 MB private
  and 428 MB working set when idle, 290 MB and 477 MB at peak. The app itself
  accounts for 38 MB of private memory.
- `driveleaf.exe` is 3.8 MB, against a 15 MB target.

## Engine spike

### Exactness on a real NTFS volume

`fixtures/ntfs3g/run.sh` formats an image with `mkntfs`, fills it through
`ntfs-3g`, and compares the parser with what the file system reports.

| Check | Result |
|---|---|
| Records matched one to one | 1,957 |
| Names checked | 2,359 |
| Extension records | 83 |
| Sizes, link counts, names and stream sizes | All match |
| Allocated total vs volume used | 111,493,120 vs 111,493,120 bytes (0.000%) |
| Corrupt or malformed records | 0 |

The image covers hard links, including one file with 400 names, plus sparse,
compressed, fragmented and alternate-stream files, and deleted records.

It was written by `ntfs-3g`, not by Windows, so Windows-only cases such as
WOF/CompactOS stay unproven until the real-volume run and the M1 fixtures.

### Speed against the candidate crates

Same 3,000,000-slot synthetic volume and same workload for every engine: decode
every in-use record, count, read the name and sum the sizes. The file sits in the
OS cache, so this measures parsing and memory copies, not the disk. Reproduce
with `scripts/m0-synth.ps1`.

| Engine | Time (median of 3) | Peak memory | Kept in memory | Notes |
|---|---|---|---|---|
| **driveleaf** | **0.377 s** | 375 MB | 275 MB | 95 bytes per entry |
| `mft` 0.9.1 | 1.110 s (1 run) | 4 MB | Nothing | Keeps no results; needs a dumped MFT |
| `ntfs-reader` 0.6.0, `MftScan` | 1.505 s | 78 MB | 47 MB | Keeps no names |
| `ntfs-reader` 0.6.0, `Mft` | 1.978 s | 2,958 MB | 2,930 MB | Holds the whole MFT |

- Driveleaf is 2.9 to 5.2 times faster here while also keeping the data the
  model needs, which is about 8 million records per second.
- 374 ms of its 377 ms went to the reader thread. Parsing keeps up with the
  read, so I/O sets the limit and the real-volume time depends on the disk.
- 95 bytes per entry fits the 150-byte target, though this is the raw scan
  output. The final tree is an M1 measurement.

### Directory walk on the real C: drive

The fallback scanner needs no elevation.

| Run | Time | Files | Folders | Unreadable | Peak memory |
|---|---|---|---|---|---|
| First | 9.9 s | 587,478 | 150,849 | 286 | 18 MB |
| Second, cached | 1.6 s | 587,478 | 150,849 | 286 | 16 MB |

## Trademark check

Searched on 2026-10-31. This is a screening, not legal advice.

| Register | "driveleaf" | "drive leaf" |
|---|---|---|
| USPTO | No results | No results |
| TMview (144 million marks, including EUIPO, USPTO and INPI) | No results | 3 unrelated marks |
| INPI, radical search, data to 2026-10-27 | No results | Not searched |

The three TMview hits are "DRIVE TO LESS and leaf design" (Canada, class 35,
registered), "GREEN LEAF DRIVERS." (US, class 9, cancelled) and a lawn care
services phrase (US, class 37, pending). None concerns disk software. A control
search for "nokia" returned 1,686 marks, so the empty results are real.

## Decisions

1. **Own parser, not a crate.** It is 2.9 to 5.2 times faster than the candidates
   on the same input and it passes the real-volume exactness check. It also
   stays small: the portable parser has no `unsafe` and depends only on `rayon`.
   `mft` cannot read a live volume, and `ntfs-reader` trades either memory or
   the results themselves for its speed. This stands unless the real-volume run
   shows a surprise.
2. **Tauri stays.** It beats the interface targets with room to spare, so the
   custom protocol fallback is not needed and `ipc::Response` stays.
3. **Develop in WSL.** Tauri cross-compiles to `x86_64-pc-windows-msvc` with
   `cargo-xwin`, in about 100 seconds from clean. Official releases stay on
   Windows CI runners, because Tauri calls cross-compiling a last resort.

## Still open

- **Real-volume MFT scan.** Run `scripts/m0-bench.ps1` as administrator. It only
  reads, and it records time, memory, and the allocated total against the used
  space for Driveleaf and `ntfs-reader`.
- **CI has never run.** The workflow needs a remote repository, and creating one
  is a public action waiting for approval.
- **WebGL2 without a GPU**, in a VM or over RDP, is an M2 test.
