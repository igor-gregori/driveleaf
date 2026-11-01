# Security policy

Driveleaf often runs as administrator and reads raw disk structures, so security
reports matter a lot here.

## Reporting a problem

Please report vulnerabilities privately, through GitHub's **Report a vulnerability**
button on the Security tab of this repository, rather than in a public issue.
Include what you did, what happened and which version you used. Expect a first
reply within a week.

## What counts

Especially welcome are reports about:

- a crafted volume, image or file name that crashes the scanner, exhausts memory
  or reaches outside the scanned folder;
- any way to remove or change a file without the confirmation, or to bypass the
  protected locations;
- code running with the app's privileges, through DLL loading, the web view or the
  IPC commands.

## How the app defends itself

- **The parser trusts nothing.** It has no `unsafe` code, every offset is bounds
  checked, and tables are sized by what was actually read, never by what the disk
  claims. A randomized test and three fuzz targets cover it.
- **`unsafe` is contained** in `driveleaf-win` and the `win` module of
  `driveleaf-ntfs`, around Windows calls.
- **Actions take node IDs from the scan**, never paths from the web view. Before
  removing anything, the backend checks that every name is a plain Windows name,
  that the location is not protected, and that the path still holds the scanned
  file.
- **Removal goes to the Recycle Bin.** Windows asks first if an item cannot go
  there. Nothing is ever deleted permanently without that question.
- **The web view is locked down**: a strict content security policy, no remote
  content, no `fs` or `shell` plugins and minimal capabilities.
- **DLLs load from System32 only**, both for imports and for later loads, so a DLL
  planted next to the program cannot run when it is elevated.

The app runs its whole process elevated when you ask for that. Splitting a small
elevated helper from the interface is planned for after 0.1.
