//! A made-up disk with plausible folders, for screenshots and demos. It holds no
//! data at all: every file only declares its size.

use crate::synth::{AttrSpec, ROOT_RECORD, RecordSpec};

const FIRST_USER_RECORD: u64 = 24;
const CLUSTER: u64 = 4096;
const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;
const KIB: u64 = 1 << 10;

/// One folder of the demo disk: its path, then `(stem, extension, count, size)`.
type Group = (
    &'static str,
    &'static [(&'static str, &'static str, u32, u64)],
);

const DISK: &[Group] = &[
    (
        "Games/Starfall Odyssey/Content/Paks",
        &[
            ("pakchunk", "pak", 24, 3 * GIB),
            ("global", "ucas", 6, 2 * GIB),
        ],
    ),
    (
        "Games/Starfall Odyssey/Binaries",
        &[
            ("StarfallOdyssey", "exe", 1, 180 * MIB),
            ("engine", "dll", 60, 12 * MIB),
        ],
    ),
    (
        "Games/Neon Drift/data",
        &[
            ("track", "pak", 40, 900 * MIB),
            ("audio", "bank", 30, 240 * MIB),
        ],
    ),
    (
        "Games/Harbor Tales/assets",
        &[
            ("level", "assets", 80, 300 * MIB),
            ("shader", "bin", 200, 8 * MIB),
        ],
    ),
    ("Videos/Family/2024", &[("VID", "mp4", 60, 900 * MIB)]),
    ("Videos/Family/2025", &[("VID", "mp4", 75, 1100 * MIB)]),
    ("Videos/Trips/Patagonia", &[("clip", "mov", 40, 2 * GIB)]),
    ("Videos/Trips/Lisbon", &[("clip", "mkv", 24, 1400 * MIB)]),
    ("Videos/Courses", &[("lesson", "mp4", 120, 280 * MIB)]),
    (
        "Photos/2023",
        &[("IMG", "jpg", 1400, 4 * MIB), ("RAW", "dng", 220, 28 * MIB)],
    ),
    (
        "Photos/2024",
        &[("IMG", "jpg", 1800, 4 * MIB), ("RAW", "dng", 340, 28 * MIB)],
    ),
    (
        "Photos/2025",
        &[
            ("IMG", "heic", 2400, 3 * MIB),
            ("RAW", "cr3", 500, 30 * MIB),
        ],
    ),
    ("Music/Library/Jazz", &[("track", "flac", 480, 38 * MIB)]),
    ("Music/Library/Rock", &[("track", "flac", 620, 34 * MIB)]),
    (
        "Music/Library/Podcasts",
        &[("episode", "mp3", 300, 62 * MIB)],
    ),
    (
        "Documents/Work/Reports",
        &[
            ("report", "pdf", 340, 3 * MIB),
            ("budget", "xlsx", 90, 900 * KIB),
        ],
    ),
    (
        "Documents/Work/Presentations",
        &[("deck", "pptx", 60, 48 * MIB)],
    ),
    (
        "Documents/Personal/Taxes",
        &[("return", "pdf", 40, 2 * MIB)],
    ),
    (
        "Documents/Personal/Books",
        &[("book", "epub", 180, 6 * MIB)],
    ),
    (
        "Projects/atlas/target/release",
        &[
            ("libatlas", "rlib", 300, 22 * MIB),
            ("atlas", "exe", 4, 90 * MIB),
        ],
    ),
    ("Projects/atlas/src", &[("module", "rs", 400, 28 * KIB)]),
    (
        "Projects/storefront/node_modules/.cache",
        &[
            ("chunk", "js", 3000, 90 * KIB),
            ("types", "json", 1500, 40 * KIB),
        ],
    ),
    (
        "Projects/storefront/src",
        &[
            ("component", "tsx", 500, 12 * KIB),
            ("style", "css", 120, 16 * KIB),
        ],
    ),
    (
        "Projects/models",
        &[("checkpoint", "safetensors", 6, 7 * GIB)],
    ),
    (
        "Downloads",
        &[
            ("ubuntu-desktop", "iso", 3, 6 * GIB),
            ("installer", "exe", 40, 180 * MIB),
            ("backup", "zip", 16, 2 * GIB),
            ("archive", "7z", 12, 900 * MIB),
        ],
    ),
    ("Backups/Phone", &[("backup", "tar", 8, 12 * GIB)]),
    ("Backups/Laptop", &[("system", "vhdx", 2, 60 * GIB)]),
    (
        "Program Files/Studio Suite",
        &[
            ("studio", "exe", 6, 220 * MIB),
            ("plugin", "dll", 340, 14 * MIB),
        ],
    ),
    (
        "Program Files/Browser",
        &[
            ("browser", "dll", 40, 40 * MIB),
            ("resources", "pak", 12, 90 * MIB),
        ],
    ),
    (
        "Windows/System32",
        &[
            ("system", "dll", 3200, 900 * KIB),
            ("driver", "sys", 500, 400 * KIB),
        ],
    ),
    (
        "Windows/WinSxS",
        &[
            ("component", "dll", 6000, 700 * KIB),
            ("manifest", "manifest", 8000, 6 * KIB),
        ],
    ),
    (
        "Windows/Logs",
        &[
            ("trace", "etl", 300, 8 * MIB),
            ("setup", "log", 200, 600 * KIB),
        ],
    ),
];

/// Calls `emit` for each record of the demo disk.
pub fn for_each_demo_record(mut emit: impl FnMut(u64, RecordSpec)) {
    let root = RecordSpec::directory(5)
        .attr(AttrSpec::standard(0x06))
        .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "."));
    emit(ROOT_RECORD, root);

    let mut number = FIRST_USER_RECORD;
    let mut folders: Vec<(String, u64)> = Vec::new();
    let mut seed = 0x2545_F491_u32;
    for (path, files) in DISK {
        let mut parent = ROOT_RECORD;
        let mut sequence = 5;
        let mut walked = String::new();
        for part in path.split('/') {
            walked.push('/');
            walked.push_str(part);
            if let Some(found) = folders.iter().find(|folder| folder.0 == walked) {
                parent = found.1;
            } else {
                emit(
                    number,
                    RecordSpec::directory(1)
                        .attr(AttrSpec::standard(0x10))
                        .attr(AttrSpec::name(parent, sequence, 3, part)),
                );
                folders.push((walked.clone(), number));
                parent = number;
                number += 1;
            }
            sequence = 1;
        }
        for (stem, extension, count, size) in *files {
            for index in 0..*count {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                // Between half and twice the typical size, skewed low like real files.
                let unit = f64::from(seed) / f64::from(u32::MAX);
                let bytes = ((*size as f64) * (0.5 + 1.5 * unit * unit)) as u64;
                let name = format!("{stem}-{index:04}.{extension}");
                emit(
                    number,
                    RecordSpec::file(1)
                        .attr(AttrSpec::standard(0x20))
                        .attr(AttrSpec::name(parent, sequence, 3, &name))
                        .attr(AttrSpec::non_resident_data(
                            b"",
                            bytes,
                            bytes.next_multiple_of(CLUSTER),
                        )),
                );
                number += 1;
            }
        }
    }
}

/// How many record slots the demo disk needs.
pub fn demo_record_count() -> u64 {
    let mut last = 0;
    for_each_demo_record(|number, _| last = number);
    last + 1024
}
