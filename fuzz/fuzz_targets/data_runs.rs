#![no_main]

use driveleaf_ntfs::Runs;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut runs = Runs::new(data);
    let _ = runs.by_ref().count();
    let _ = runs.malformed();
});
