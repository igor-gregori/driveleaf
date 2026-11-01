# Packaging

Templates for `winget` and Scoop. Nothing here is submitted anywhere until a
release is approved: see [docs/release.md](../docs/release.md).

Replace these placeholders, using the hashes from `SHA256SUMS.txt`:

| Placeholder | Value |
|---|---|
| `{{VERSION}}` | The version, such as `0.1.0` |
| `{{REPOSITORY}}` | The repository, such as `owner/driveleaf` |
| `{{INSTALLER_SHA256}}` | Hash of `Driveleaf_{{VERSION}}_x64-setup.exe` |
| `{{PORTABLE_SHA256}}` | Hash of `Driveleaf_{{VERSION}}_x64-portable.exe` |
