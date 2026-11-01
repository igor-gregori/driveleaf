//! Turns the raw results of an MFT scan into a [`Model`].

use driveleaf_ntfs::{Chunk, Geometry, RAW_DIRECTORY, RAW_EXTENSION, ScanOutput};

use crate::model::{
    FLAG_CLOUD, FLAG_COMPRESSED, FLAG_DIRECTORY, FLAG_HARD_LINK, FLAG_HIDDEN, FLAG_LINK,
    FLAG_METAFILE, FLAG_SPARSE, FLAG_STREAMS, FLAG_SYNTHETIC, FLAG_SYSTEM, Model, NodeData, NodeId,
    ROOT, ScanMode, TreeBuilder, VolumeSummary,
};

const ROOT_RECORD: u32 = 5;
/// NTFS keeps the records below this one for itself.
const FIRST_USER_RECORD: u32 = 24;
const NO_NODE: u32 = u32::MAX;

const SEEN: u8 = 1;
const DIRECTORY: u8 = 2;
const NAMED: u8 = 4;

const ATTR_HIDDEN: u32 = 0x0002;
const ATTR_SYSTEM: u32 = 0x0004;
const ATTR_SPARSE: u32 = 0x0200;
const ATTR_COMPRESSED: u32 = 0x0800;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_OPEN: u32 = 0x0004_0000;
const ATTR_RECALL_ON_DATA: u32 = 0x0040_0000;
const ATTR_CLOUD: u32 = ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA;

/// Display names of the nodes the scanner makes up.
pub const METADATA_NAME: &str = "$NTFS metadata";
pub const ORPHANS_NAME: &str = "<Orphaned files>";
pub const UNNAMED_NAME: &str = "<Unnamed records>";

/// A record's facts, merged from its base and extension records.
#[derive(Clone, Copy, Default)]
struct Slot {
    sequence: u16,
    attributes: u32,
    reparse_tag: u32,
    modified: u64,
    logical: u64,
    stream: u64,
    allocated: u64,
    state: u8,
}

/// Tag bit of reparse points that lead somewhere else, like symbolic links and
/// junctions. Others, such as WOF compression or cloud files, are the file itself.
const TAG_NAME_SURROGATE: u32 = 0x2000_0000;
/// The command aliases of Store apps, which lead elsewhere but lack that bit.
const TAG_APP_EXEC_LINK: u32 = 0x8000_001B;

/// True for a reparse tag that points at something else.
pub(crate) fn is_link_tag(tag: u32) -> bool {
    tag & TAG_NAME_SURROGATE != 0 || tag == TAG_APP_EXEC_LINK
}

/// Turns Windows file attributes and a reparse tag into model flags.
pub(crate) fn node_flags(attributes: u32, reparse_tag: u32) -> u16 {
    let mut flags = 0;
    if attributes & ATTR_HIDDEN != 0 {
        flags |= FLAG_HIDDEN;
    }
    if attributes & ATTR_SYSTEM != 0 {
        flags |= FLAG_SYSTEM;
    }
    if attributes & ATTR_SPARSE != 0 {
        flags |= FLAG_SPARSE;
    }
    if attributes & ATTR_COMPRESSED != 0 {
        flags |= FLAG_COMPRESSED;
    }
    if attributes & ATTR_CLOUD != 0 {
        flags |= FLAG_CLOUD;
    }
    if is_link_tag(reparse_tag) {
        flags |= FLAG_LINK;
    }
    flags
}

/// Keeps the node order independent of how the parsers happened to finish.
fn in_record_order(chunks: &[Chunk]) -> Vec<&Chunk> {
    let mut ordered: Vec<&Chunk> = chunks.iter().collect();
    ordered.sort_by_key(|chunk| chunk.files.first().map_or(u32::MAX, |file| file.record));
    ordered
}

/// Assembles the tree. `root_name` is what the root displays as, such as `C:\`.
///
/// A file with several hard links gets one node per name. The first name in
/// record order carries the size, and the others are marked as links, so the
/// space counts once however many folders show the file.
pub fn build_model(output: &ScanOutput, geometry: &Geometry, root_name: &str) -> Model {
    let stats = &output.stats;
    // Sized by what was read, never by what the MFT claims, which a forged
    // volume could set to billions. Records past the end just get dropped.
    let mut slots = vec![Slot::default(); output.read_limit()];
    let chunks = in_record_order(&output.chunks);
    for chunk in &chunks {
        for file in &chunk.files {
            let Some(slot) = slots.get_mut(file.record as usize) else {
                continue;
            };
            slot.logical = slot.logical.saturating_add(file.logical_size);
            slot.stream = slot.stream.saturating_add(file.stream_size);
            slot.allocated = slot.allocated.saturating_add(file.allocated);
            if file.flags & RAW_EXTENSION == 0 {
                slot.sequence = file.sequence;
                slot.attributes = file.attributes;
                slot.modified = file.modified;
                slot.reparse_tag = file.reparse_tag;
                slot.state |= SEEN;
                if file.flags & RAW_DIRECTORY != 0 {
                    slot.state |= DIRECTORY;
                }
            }
        }
    }

    let name_bytes = stats.name_bytes as usize;
    let mut tree = TreeBuilder::new(root_name, stats.names as usize + 4, name_bytes + 64);
    let synthetic = |flags| NodeData {
        flags: flags | FLAG_SYNTHETIC,
        ..NodeData::default()
    };
    let metadata = tree.add(
        ROOT,
        METADATA_NAME.as_bytes(),
        synthetic(FLAG_DIRECTORY | FLAG_METAFILE),
    );
    let orphans = tree.add(ROOT, ORPHANS_NAME.as_bytes(), synthetic(FLAG_DIRECTORY));
    let unnamed = tree.add(metadata, UNNAMED_NAME.as_bytes(), synthetic(FLAG_METAFILE));

    // Folders can come after their contents in the MFT, so their node numbers
    // are worked out before any node gets added.
    let first_free = tree.len() as u32;
    let mut folder_nodes = vec![NO_NODE; slots.len()];
    if let Some(root) = folder_nodes.get_mut(ROOT_RECORD as usize) {
        *root = ROOT;
    }
    let mut next = first_free;
    for chunk in &chunks {
        for name in &chunk.names {
            let Some(slot) = slots.get(name.record as usize) else {
                continue;
            };
            if slot.state & SEEN == 0 || name.record == ROOT_RECORD {
                continue;
            }
            if slot.state & DIRECTORY != 0 && folder_nodes[name.record as usize] == NO_NODE {
                folder_nodes[name.record as usize] = next;
            }
            next += 1;
        }
    }

    for chunk in &chunks {
        for name in &chunk.names {
            let record = name.record as usize;
            let Some(slot) = slots.get_mut(record) else {
                continue;
            };
            if slot.state & SEEN == 0 || name.record == ROOT_RECORD {
                continue;
            }
            let first_name = slot.state & NAMED == 0;
            slot.state |= NAMED;
            let slot = *slot;

            let is_directory = slot.state & DIRECTORY != 0;
            let mut flags = node_flags(slot.attributes, slot.reparse_tag);
            if is_directory {
                flags |= FLAG_DIRECTORY;
            }
            // Windows also puts ordinary files in the last reserved records, so
            // the number alone does not make something a metafile.
            let metafile = name.record < FIRST_USER_RECORD && chunk.name(name).starts_with(b"$");
            if metafile {
                flags |= FLAG_METAFILE;
            }
            if slot.stream > 0 {
                flags |= FLAG_STREAMS;
            }
            if !first_name {
                flags |= FLAG_HARD_LINK;
            }

            let parent = parent_node(
                name.parent,
                name.parent_sequence,
                name.record,
                &slots,
                &folder_nodes,
            )
            .unwrap_or(orphans);
            let parent = if parent == ROOT && metafile {
                metadata
            } else {
                parent
            };
            tree.add(
                parent,
                chunk.name(name),
                NodeData {
                    flags,
                    file_id: name.record,
                    logical: slot.logical,
                    allocated: slot.allocated,
                    modified: slot.modified,
                },
            );
        }
    }

    if let Some(root) = slots.get(ROOT_RECORD as usize) {
        tree.update(
            ROOT,
            NodeData {
                flags: node_flags(root.attributes, 0),
                file_id: ROOT_RECORD,
                logical: 0,
                allocated: root.allocated,
                modified: root.modified,
            },
        );
    }

    // Space held by records without any name still has to add up to the disk.
    let (logical, allocated) = slots
        .iter()
        .enumerate()
        .filter(|(record, slot)| *record != ROOT_RECORD as usize && slot.state & NAMED == 0)
        .fold((0_u64, 0_u64), |(logical, allocated), (_, slot)| {
            (
                logical.saturating_add(slot.logical),
                allocated.saturating_add(slot.allocated),
            )
        });
    tree.add_sizes(unnamed, logical, allocated);

    let cluster = u64::from(geometry.bytes_per_cluster);
    let summary = VolumeSummary {
        mode: ScanMode::Mft,
        used_bytes: geometry
            .free_clusters
            .map(|free| (geometry.total_clusters - free) * cluster),
        total_bytes: Some(geometry.total_clusters * cluster),
        problems: stats.corrupt_records + stats.malformed_records + stats.unreadable_blocks,
    };
    tree.finish(ORPHANS_NAME, summary)
}

/// Finds the folder a name belongs to, or `None` when it points at a folder that
/// no longer exists. A sequence number that differs means the record was reused.
fn parent_node(
    parent: u32,
    sequence: u16,
    record: u32,
    slots: &[Slot],
    folder_nodes: &[u32],
) -> Option<NodeId> {
    if parent == record {
        return None;
    }
    let node = *folder_nodes.get(parent as usize)?;
    if node == NO_NODE {
        return None;
    }
    let slot = slots.get(parent as usize)?;
    (sequence == 0 || sequence == slot.sequence).then_some(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FLAG_DELETED;
    use driveleaf_ntfs::synth::{
        AttrSpec, ImageLayout, ROOT_RECORD as SYNTH_ROOT, RecordSpec, TreeTotals, generate_tree,
        write_image,
    };
    use driveleaf_ntfs::{ScanControl, ScanOptions, scan};
    use std::io::Cursor;

    const CLUSTER: u64 = 4096;
    const ATTR_REPARSE: u32 = 0x0400;
    const LAYOUT: ImageLayout = ImageLayout {
        bytes_per_cluster: 4096,
        bytes_per_record: 1024,
        record_count: 256,
        fragments: 1,
    };

    fn root_dir() -> (u64, RecordSpec) {
        let spec = RecordSpec::directory(5)
            .attr(AttrSpec::standard(0x06))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "."));
        (SYNTH_ROOT, spec)
    }

    fn file(parent: u64, sequence: u16, name: &str, bytes: u64) -> RecordSpec {
        RecordSpec::file(1)
            .attr(AttrSpec::standard(0x20))
            .attr(AttrSpec::name(parent, sequence, 3, name))
            .attr(AttrSpec::non_resident_data(b"", bytes, bytes))
    }

    fn model_of(layout: &ImageLayout, records: Vec<(u64, RecordSpec)>) -> Model {
        let mut image = Cursor::new(Vec::new());
        write_image(&mut image, layout, records).unwrap();
        let bytes = image.into_inner();
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        let output = scan(
            &bytes[..],
            &geometry,
            &ScanOptions::default(),
            &ScanControl::default(),
        )
        .unwrap();
        build_model(&output, &geometry, "X:\\")
    }

    fn find(model: &Model, path: &str) -> NodeId {
        (0..model.len() as NodeId)
            .find(|&node| model.display_path(node) == path)
            .unwrap_or_else(|| panic!("{path} is not in the tree"))
    }

    fn mft_bytes(layout: &ImageLayout) -> u64 {
        (layout.record_count * 1024).next_multiple_of(CLUSTER)
            + layout.record_count.div_ceil(8).next_multiple_of(CLUSTER)
    }

    #[test]
    fn a_generated_tree_adds_up_to_its_totals() {
        let layout = ImageLayout {
            record_count: 30_000,
            fragments: 4,
            ..LAYOUT
        };
        let mut totals = TreeTotals::default();
        let records = generate_tree(layout.record_count, CLUSTER, &mut totals);
        let model = model_of(&layout, records);
        assert_eq!(
            model.allocated(ROOT),
            totals.allocated_bytes + mft_bytes(&layout)
        );
        assert_eq!(
            model.logical(ROOT),
            totals.logical_bytes + layout.record_count * 1024
        );
        assert_eq!(model.files(ROOT) as u64, totals.files + 1);
    }

    #[test]
    fn folders_may_come_after_their_contents() {
        let child = file(40, 1, "inside.bin", 8192);
        let folder = RecordSpec::directory(1)
            .attr(AttrSpec::standard(0x10))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "late"));
        let model = model_of(&LAYOUT, vec![root_dir(), (30, child), (40, folder)]);
        assert_eq!(model.allocated(find(&model, "X:\\late")), 8192);
        assert_eq!(model.allocated(find(&model, "X:\\late\\inside.bin")), 8192);
    }

    #[test]
    fn metafiles_gather_under_their_own_node() {
        let model = model_of(&LAYOUT, vec![root_dir()]);
        let mft = find(&model, "X:\\$NTFS metadata\\$MFT");
        assert_ne!(model.flags(mft) & FLAG_METAFILE, 0);
        assert_eq!(model.allocated(mft), mft_bytes(&LAYOUT));
        assert_eq!(model.allocated(ROOT), mft_bytes(&LAYOUT));
    }

    #[test]
    fn stale_parent_references_become_orphans_and_still_count() {
        // The folder at record 40 has sequence 7, but the file remembers 3:
        // its real parent was deleted and the record reused.
        let folder = RecordSpec::directory(7)
            .attr(AttrSpec::standard(0x10))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "reused"));
        let stale = file(40, 3, "lost.bin", 4096);
        let missing = file(90, 1, "nowhere.bin", 4096);
        let model = model_of(
            &LAYOUT,
            vec![root_dir(), (30, stale), (31, missing), (40, folder)],
        );
        let orphans = find(&model, "X:\\<Orphaned files>");
        assert_eq!(model.files(orphans), 2);
        assert_eq!(model.allocated(orphans), 8192);
        assert_eq!(model.allocated(ROOT), 8192 + mft_bytes(&LAYOUT));
    }

    #[test]
    fn hard_links_count_at_their_first_name_only() {
        let spec = RecordSpec::file(1)
            .links(2)
            .attr(AttrSpec::standard(0x20))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "first"))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "second"))
            .attr(AttrSpec::non_resident_data(b"", 4096, 4096));
        let model = model_of(&LAYOUT, vec![root_dir(), (30, spec)]);
        let first = find(&model, "X:\\first");
        let second = find(&model, "X:\\second");
        assert_eq!(model.flags(first) & FLAG_HARD_LINK, 0);
        assert_ne!(model.flags(second) & FLAG_HARD_LINK, 0);
        assert_eq!(model.allocated(second), 4096);
        assert_eq!(model.weight(second), 0);
        assert_eq!(model.allocated(ROOT), 4096 + mft_bytes(&LAYOUT));
        assert_eq!(model.other_links(first), [second]);
    }

    #[test]
    fn extension_records_merge_into_their_file() {
        let base = file(SYNTH_ROOT, 5, "big.bin", 8192);
        let extension =
            RecordSpec::extension_of(30).attr(AttrSpec::non_resident_data(b"S\0", 4096, 4096));
        let model = model_of(&LAYOUT, vec![root_dir(), (30, base), (31, extension)]);
        let node = find(&model, "X:\\big.bin");
        assert_eq!(model.allocated(node), 12_288);
        assert_eq!(model.logical(node), 8192);
        assert_ne!(model.flags(node) & FLAG_STREAMS, 0);
    }

    #[test]
    fn records_without_names_land_in_the_unnamed_node() {
        let spec = RecordSpec::file(1).attr(AttrSpec::non_resident_data(b"", 4096, 4096));
        let model = model_of(&LAYOUT, vec![root_dir(), (30, spec)]);
        let unnamed = find(&model, "X:\\$NTFS metadata\\<Unnamed records>");
        assert_eq!(model.allocated(unnamed), 4096);
        assert_eq!(model.allocated(ROOT), 4096 + mft_bytes(&LAYOUT));
    }

    #[test]
    fn only_dollar_names_in_reserved_records_are_metafiles() {
        // Windows was seen keeping bootTel.dat in record 19.
        let ordinary = RecordSpec::file(1)
            .attr(AttrSpec::standard(0x20))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "bootTel.dat"));
        let reserved = RecordSpec::file(1)
            .attr(AttrSpec::standard(0x06))
            .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, "$Reserved"));
        let model = model_of(&LAYOUT, vec![root_dir(), (19, ordinary), (20, reserved)]);
        assert_eq!(
            model.flags(find(&model, "X:\\bootTel.dat")) & FLAG_METAFILE,
            0
        );
        let moved = find(&model, "X:\\$NTFS metadata\\$Reserved");
        assert_ne!(model.flags(moved) & FLAG_METAFILE, 0);
    }

    #[test]
    fn only_tags_that_point_elsewhere_are_links() {
        let reparse = |name, tag: u32| {
            RecordSpec::file(1)
                .attr(AttrSpec::standard(ATTR_REPARSE))
                .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, name))
                .attr(AttrSpec::Resident {
                    type_code: driveleaf_ntfs::record::ATTR_REPARSE_POINT,
                    name: Vec::new(),
                    value: tag.to_le_bytes().to_vec(),
                })
        };
        let model = model_of(
            &LAYOUT,
            vec![
                root_dir(),
                (30, reparse("symlink", 0xA000_000C)),
                (31, reparse("junction", 0xA000_0003)),
                (32, reparse("wof", 0x8000_0017)),
                (33, reparse("cloud", 0x9000_001A)),
                (34, reparse("alias", 0x8000_001B)),
            ],
        );
        let flags = |path| model.flags(find(&model, path));
        assert_ne!(flags("X:\\symlink") & FLAG_LINK, 0);
        assert_ne!(flags("X:\\junction") & FLAG_LINK, 0);
        assert_ne!(flags("X:\\alias") & FLAG_LINK, 0);
        assert_eq!(flags("X:\\wof") & FLAG_LINK, 0);
        assert_eq!(flags("X:\\cloud") & FLAG_LINK, 0);
    }

    #[test]
    fn a_reparse_value_outside_the_record_keeps_its_tag() {
        let named = |name, attributes: u32, field: u32| {
            let AttrSpec::Resident {
                type_code,
                name,
                mut value,
            } = AttrSpec::name(SYNTH_ROOT, 5, 3, name)
            else {
                panic!("names are resident");
            };
            value[0x38..0x3C].copy_from_slice(&attributes.to_le_bytes());
            value[0x3C..0x40].copy_from_slice(&field.to_le_bytes());
            RecordSpec::file(1)
                .attr(AttrSpec::standard(attributes))
                .attr(AttrSpec::Resident {
                    type_code,
                    name,
                    value,
                })
        };
        let model = model_of(
            &LAYOUT,
            vec![
                root_dir(),
                (30, named("alias", ATTR_REPARSE, 0x8000_001B)),
                // Not a reparse point, so that field is an extended attribute size.
                (31, named("plain", 0, 0xA000_000C)),
            ],
        );
        assert_ne!(model.flags(find(&model, "X:\\alias")) & FLAG_LINK, 0);
        assert_eq!(model.flags(find(&model, "X:\\plain")) & FLAG_LINK, 0);
    }

    #[test]
    fn attribute_flags_reach_the_nodes() {
        let hidden = |name, attributes| {
            RecordSpec::file(1)
                .attr(AttrSpec::standard(attributes))
                .attr(AttrSpec::name(SYNTH_ROOT, 5, 3, name))
        };
        let model = model_of(
            &LAYOUT,
            vec![
                root_dir(),
                (30, hidden("hidden", ATTR_HIDDEN | ATTR_SYSTEM)),
                (31, hidden("cloud", ATTR_REPARSE | ATTR_RECALL_ON_DATA)),
            ],
        );
        let flags = |path| model.flags(find(&model, path));
        assert_ne!(flags("X:\\hidden") & FLAG_HIDDEN, 0);
        assert_ne!(flags("X:\\hidden") & FLAG_SYSTEM, 0);
        assert_ne!(flags("X:\\cloud") & FLAG_CLOUD, 0);
        assert_eq!(flags("X:\\cloud") & FLAG_LINK, 0);
    }

    #[test]
    fn the_tree_does_not_depend_on_thread_timing() {
        let layout = ImageLayout {
            record_count: 20_000,
            fragments: 3,
            ..LAYOUT
        };
        let mut totals = TreeTotals::default();
        let records = generate_tree(layout.record_count, CLUSTER, &mut totals);
        let first = model_of(&layout, records.clone());
        let second = model_of(&layout, records);
        assert_eq!(first.len(), second.len());
        for node in (0..first.len() as NodeId).step_by(97) {
            assert_eq!(first.name_bytes(node), second.name_bytes(node));
            assert_eq!(first.allocated(node), second.allocated(node));
            assert_eq!(
                first.flags(node) & !FLAG_DELETED,
                second.flags(node) & !FLAG_DELETED
            );
        }
    }

    #[test]
    fn references_past_the_read_area_are_dropped_without_allocating_for_them() {
        // An extension record claiming a base near the end of the 32-bit range
        // would need a table of billions of slots if it were believed.
        let forged = RecordSpec::extension_of(0xFFFF_FFF0)
            .attr(AttrSpec::non_resident_data(b"", 4096, 4096));
        let far_parent = file(0xFFFF_FFF1, 1, "stray.bin", 4096);
        let model = model_of(&LAYOUT, vec![root_dir(), (30, forged), (31, far_parent)]);
        let stray = find(&model, "X:\\<Orphaned files>\\stray.bin");
        assert_eq!(model.allocated(stray), 4096);
    }

    #[test]
    fn the_model_stays_under_the_memory_target() {
        let layout = ImageLayout {
            record_count: 100_000,
            fragments: 1,
            ..LAYOUT
        };
        let mut totals = TreeTotals::default();
        let records = generate_tree(layout.record_count, CLUSTER, &mut totals);
        let model = model_of(&layout, records);
        let per_entry = model.memory_bytes() / model.len();
        assert!(per_entry <= 150, "{per_entry} bytes per entry");
    }
}
