#![no_main]

use driveleaf_ntfs::record::{
    AttributeBody, Attributes, FileName, RecordHeader, StandardInfo, apply_fixups, attribute_list,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut record = data.to_vec();
    // A real record passes fixups, so also try the input as if it already had.
    let _ = apply_fixups(&mut record);
    let Ok(header) = RecordHeader::parse(&record) else {
        return;
    };
    let mut attributes = Attributes::new(&record, &header);
    for attribute in attributes.by_ref() {
        if let AttributeBody::Resident(value) = attribute.body {
            let _ = FileName::parse(value);
            let _ = StandardInfo::parse(value);
            let _ = attribute_list(value).count();
        }
    }
    let _ = attributes.malformed();
});
