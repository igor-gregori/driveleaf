# Releasing

Everything below reaches the public, so nothing here runs without the maintainer's
explicit approval. Do the steps in order.

## Before the first release

- [ ] Confirm the name. The trademark screening in `benchmarks.md` was clean, but
      it is not legal advice.
- [ ] Create the GitHub organization and repository, and push `main`.
- [ ] Add `repository` and `homepage` to `[workspace.package]`, plus `readme`,
      `keywords` and `categories` to each crate that gets published.
- [ ] Add the repository URL to `packaging/`.
- [ ] Turn on private vulnerability reporting, which `SECURITY.md` promises.
- [ ] Watch CI go green, which has never run. Fix what only shows up there.
- [ ] Commit the Windows fixture: `fixtures/windows-ntfs.img.gz` and its manifest.
- [ ] Ask the SignPath Foundation for free code signing, after checking its current
      conditions. Without a signature, SmartScreen warns every user.
- [ ] Register a domain, if wanted. `driveleaf.dev`, `.app`, `.org` and `.io` were
      free on 2026-10-31, and `driveleaf.com` was taken.

## Every release

1. Update the version in `Cargo.toml`, `apps/desktop/package.json` and
   `apps/desktop/src-tauri/tauri.conf.json`.
2. Update `CHANGELOG.md` and regenerate the notices:
   `python3 scripts/third-party-notices.py`.
3. Run the elevated checks on real hardware: `scripts/m0-bench.ps1`, then scan,
   reopen elevated, cancel, resize and send a throwaway folder to the Recycle Bin.
4. Tag `vX.Y.Z` and push the tag. The release workflow builds on Windows and stops
   at a draft release.
5. Install from the draft, on a clean machine if possible, and publish it.
6. Update `packaging/winget` and `packaging/scoop` with the version and the hashes
   from `SHA256SUMS.txt`, then submit them to `microsoft/winget-pkgs` and to a Scoop
   bucket.
7. Publish the crates, dependencies first:

   ```bash
   cargo publish -p driveleaf-ntfs
   cargo publish -p driveleaf-treemap
   cargo publish -p driveleaf-win
   cargo publish -p driveleaf-core
   ```
