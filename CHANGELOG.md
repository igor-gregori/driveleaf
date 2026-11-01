# Changelog

## 0.1.0

The first version.

- Scans NTFS volumes by reading the Master File Table in parallel, and everything
  else with a parallel folder walk.
- Shows a drive as a folder tree and a WebGL2 treemap, selected in sync, with
  animated zoom into folders.
- Lists the largest files and the space per file type, and searches every name
  with `Ctrl+K`.
- Opens items in Explorer, copies paths, shows properties and sends to the Recycle
  Bin after a confirmation.
- Counts hard links once, and counts compressed, sparse and CompactOS files by the
  clusters they really use.
- Comes in English and Brazilian Portuguese, with light and dark themes that follow
  the system.
- `driveleaf-cli` prints a whole drive as CSV or JSON.
