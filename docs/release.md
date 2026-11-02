# Releasing

Everything below reaches the public, so nothing here runs without the maintainer's
explicit approval. Do the steps in order.

## Before the first release

- [x] Confirm the name. The trademark screening in `benchmarks.md` was clean, but
      it is not legal advice.
- [x] Check the author name and email on the commits, which become public with the
      first push.
- [x] Create the repository, `igor-gregori/driveleaf`, and push `main`.
- [x] Add `repository` and `homepage` to `[workspace.package]`, plus `readme`,
      `keywords` and `categories` to each crate that gets published.
- [x] Point `packaging/` at the website. The repository fills in per release.
- [x] Turn on private vulnerability reporting, which `SECURITY.md` promises.
- [x] Watch CI go green. The Windows fixture needed fixes that only showed there:
      runners cannot compress with WOF, and diskpart detaches on its own.
- [ ] Ask the SignPath Foundation for free code signing. Its terms ask for a
      project that is "already released in the form that should be signed", so
      this comes after 0.1.0 is public. It also asks for MFA on every account, a
      "Code signing policy" section on the website, and makes SignPath
      Foundation the publisher shown by Windows.
- [x] Register a domain: `driveleaf.app`, on Cloudflare. The website in `site/`
      deploys to GitHub Pages.
- [ ] Point the domain's DNS at GitHub Pages and turn on Cloudflare Web Analytics.

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
