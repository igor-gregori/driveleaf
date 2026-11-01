# Starter issues

To open as issues once the repository is public. Each one is small, self-contained
and touches a single area.

- **Add a translation.** Copy `apps/desktop/src/i18n/en.json`, translate it, and
  register the language in `index.ts`.
- **Teach the categories more extensions.** `crates/driveleaf-core/src/categories.rs`
  groups files for the treemap colors. Find common extensions that land in Other.
- **Remember the sort order.** The tree forgets its sort column between scans.
- **Show a tooltip on treemap tiles.** The status bar shows the hovered item, but a
  tooltip next to the cursor would be easier to read.
- **Copy several paths.** Let the top files list select more than one row.
- **Export from the app.** `driveleaf-cli scan` writes CSV, and the app could offer
  the same through a save dialog.
- **Add a test for `--scan`.** Reopening elevated passes the drive letter on the
  command line, and more of that path could be covered.
- **Document the capture format.** `crates/driveleaf-ntfs/src/replay.rs` is simple,
  but only the code describes it.
