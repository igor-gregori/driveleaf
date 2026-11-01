# Architecture

## The one rule

**Data stays in Rust, and the frontend only receives what is on screen.** A whole
tree never crosses the IPC boundary, because millions of entries in JSON would
freeze the web view. Listings come in pages, and the treemap arrives as a binary
buffer of at most 200,000 tiles.

## Crates

```text
driveleaf-ntfs      MFT parser (portable, no unsafe) and Windows volume reader
driveleaf-core      model, aggregation, queries, folder walker, removal rules
driveleaf-treemap   squarified layout, with no GUI dependency
driveleaf-win       Windows calls: folder listing, volumes, elevation, shell
driveleaf-cli       command line front end and benchmarks
apps/desktop        Tauri app: IPC commands in src-tauri, React in src
```

## Scanning a volume

1. Open `\\.\C:` and read the geometry with `FSCTL_GET_NTFS_VOLUME_DATA`.
2. Read record 0 and follow it to the fragments of `$MFT:$DATA` and to
   `$MFT:$BITMAP`, which says which records are in use. A heavily fragmented MFT
   keeps its run list in extension records, found through `$ATTRIBUTE_LIST`.
3. Split the MFT into blocks of up to 4 MiB, trimmed to the used records. Reads are
   unbuffered, so repeated scans measure the disk, and they run on 8 threads on
   flash storage and 1 on a spinning disk.
4. Parse each block on the rayon pool: fixups, `$STANDARD_INFORMATION`, `$FILE_NAME`,
   `$DATA` and every other non-resident attribute. Names become WTF-8 in a pool per
   block, so names Windows allows but Unicode does not survive.

Every record gets visited anyway, so an extension record simply credits what it
holds to its base record, with no need to follow `$ATTRIBUTE_LIST` for ordinary
files.

The parser works on a `BlockSource`, never a Windows handle, so tests can run it
on images, and a capture of a scan can be replayed anywhere.

## From records to a tree

`driveleaf_core::mft_tree::build_model` merges records, then links names to folders
by parent reference, checking the sequence number. A reference to a folder that no
longer exists sends the file to `<Orphaned files>`, where its space still counts.
NTFS's own files go under `$NTFS metadata`. A folder walk fills the same model
through the same `TreeBuilder`.

`TreeBuilder::finish` links children with a counting sort, walks the tree breadth
first, aggregates sizes in reverse order, and sorts each folder by size. Nodes
that never reach the root, as a cycle in a damaged volume would, go under the
orphans.

## The model

Everything sits in contiguous vectors indexed by `u32`, with all names in a single
pool, and no `String` or `Box` per entry. That costs about 71 to 115 bytes per
entry, and dropping a model of millions of entries is instant. After aggregation
the tree is immutable: other sort orders work on index vectors, and removing a
node only flags it and adjusts its ancestors.

## The treemap

`layout_tree` opens folders largest first from a priority queue, so when the tile
budget runs out it is the smallest detail that goes missing. A parent always comes
before its children, so scanning the tiles backwards finds the deepest one under a
point, which is how hit testing works in the frontend, without any IPC. Anything
under a pixel is dropped and shows as part of its folder.

Each tile is 24 bytes: `x, y, w, h: f32`, then a style word and the node ID. The
renderer uploads the buffer once and draws it with instancing, so zooming only
changes a uniform. Colors come from CSS variables, which keeps the treemap in step
with the theme. Software rendering gets fewer tiles and no animation.

## Removing files

Every removal passes through `driveleaf_core::safety`, in portable code with tests:
no synthetic nodes, no metafiles, no protected locations, and no names that Windows
paths cannot express. The command then confirms that the path still holds the file
with the scanned ID, because the scan is a snapshot, and sends it to the Recycle
Bin through `IFileOperation`, which asks before anything would be lost for good.
Whatever the outcome, the app checks the disk to see what is really gone.
