# Contributing to Driveleaf

Thanks for helping out. Driveleaf has one job: show what fills a disk, quickly
and accurately, in a pleasant app. It is not a PC cleaner, a duplicate finder or a
sync tool, so ideas in those directions are likely to be declined.

## Getting set up

From Linux or WSL:

```bash
sudo apt-get install -y build-essential pkg-config clang lld llvm nsis ntfs-3g
cargo install --locked cargo-xwin
(cd apps/desktop && npm install)
```

From Windows: install Git, Rust (`stable-msvc`), Node.js LTS and the MSVC Build
Tools, then run `npm install` in `apps/desktop`.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
(cd apps/desktop && npm run build && npm test)
```

Changes to the parser should also pass `bash fixtures/ntfs3g/run.sh`, which checks
it file by file against a real NTFS volume. Add a test that fails without your
change, and prefer a real volume case over a hand-made one when the two differ.

To try the app from WSL, `bash scripts/dev.sh` runs Vite with hot reload and opens
the Windows build, which needs rebuilding after Rust changes.

Reading a live volume needs an elevated process, so tests use images. To work on
something only a real disk shows, run `driveleaf-cli capture C: c.dlmft` once as
administrator, and then use `c.dlmft` as a target for `mft`, `scan` and `bench`
without elevation. A capture holds every file name on the volume, so never share
one.

## Rules that keep the project clean

- **Licensing.** Everything here is `MIT OR Apache-2.0`. Do not copy code from
  WinDirStat (GPL), squirreldisk (AGPL) or anything else with a stricter license,
  and do not decompile or inspect closed tools. Work from public documentation of
  NTFS and from the squarified treemap paper by Bruls, Huizing and van Wijk.
- **`unsafe`.** It belongs in `driveleaf-win` and in the `win` module of
  `driveleaf-ntfs`, and nowhere else. Every block gets a `SAFETY` comment.
- **Untrusted input.** The parser may run as administrator on a volume someone
  else made. Treat every byte as hostile: check bounds, avoid sizes taken from
  the disk without a cap, and turn bad data into a counter rather than a panic.
- **Data stays in Rust.** The frontend receives only what is on screen, and
  actions take node IDs, never paths from JavaScript.
- **No network.** The app stays offline: no telemetry, no update checks, no
  remote content.

## Translations

All interface text lives in `apps/desktop/src/i18n`. To add a language, copy
`en.json`, translate it and register it in `index.ts`. A test checks that every
language has the same keys and placeholders.
