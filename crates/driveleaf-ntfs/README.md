# driveleaf-ntfs

Fast NTFS Master File Table scanner, part of [Driveleaf](https://driveleaf.app), a fast,
open-source disk space analyzer for Windows.

Reads the Master File Table of an NTFS volume in parallel and decodes every
record: names, parents, sizes, hard links, streams, compression and reparse
points. The parser works on any `BlockSource`, has no `unsafe`, and treats every
byte as untrusted, so it runs and is tested on any platform. The Windows volume
reader lives in the `win` module.

See the [repository](https://github.com/igor-gregori/driveleaf) for the architecture notes and benchmarks.

Licensed under either of MIT or Apache-2.0 at your option.
