#![no_main]

use driveleaf_ntfs::{Geometry, ScanControl, ScanOptions, scan};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(boot) = data.get(..512) else {
        return;
    };
    let Ok(geometry) = Geometry::from_boot_sector(boot) else {
        return;
    };
    let options = ScanOptions {
        block_bytes: 1 << 16,
        buffers: 2,
        readers: 1,
    };
    let _ = scan(data, &geometry, &options, &ScanControl::default());
});
