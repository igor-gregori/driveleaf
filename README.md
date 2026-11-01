# Driveleaf

A fast, open-source disk space analyzer for Windows. Driveleaf reads the NTFS
Master File Table directly instead of walking folders, shows the result as a
treemap, and keeps its scan engine reusable as Rust crates.

Status: early development. Nothing is released yet.

## Layout

| Path | What it is |
|---|---|
| `crates/driveleaf-ntfs` | MFT parser (portable, no `unsafe`) and Windows volume layer |
| `crates/driveleaf-core` | Disk usage model and the directory-walk fallback |
| `crates/driveleaf-treemap` | Squarified treemap layout, with no GUI dependency |
| `crates/driveleaf-cli` | Command line front end and benchmarks |
| `apps/desktop` | Tauri 2 + React + shadcn/ui app with a WebGL2 treemap |
| `fixtures` | Real NTFS volume checks |
| `spikes` | Throwaway experiments behind milestone decisions |
| `docs` | Decisions and measurements |

## Developing

Everything builds from Linux or WSL, and nothing needs installing on Windows
beyond the WebView2 runtime it already ships with.

You need Rust, Node.js and these packages:

```bash
sudo apt-get install -y build-essential pkg-config clang lld llvm nsis
cargo install --locked cargo-xwin
```

Common tasks:

```bash
cargo test                       # engine tests, on Linux
bash fixtures/ntfs3g/run.sh      # parser against a real NTFS volume, needs ntfs-3g
bash scripts/dev.sh              # Vite here, the Windows app there, with hot reload
bash scripts/windows-build.sh    # release binaries, copied to the Windows disk
```

Native Windows development works too: install the MSVC Build Tools, Rust and
Node.js, then use `npm run tauri dev` in `apps/desktop`. Official release builds
always come from native Windows CI runners, because Tauri does not officially
support cross-compiling Windows installers.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option.
