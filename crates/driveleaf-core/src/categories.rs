//! File categories, used to color the treemap and to group the file types view.

/// What kind of content a node holds. The numbers are part of the treemap wire
/// format: the frontend maps them to theme colors.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Directory = 0,
    Other = 1,
    Document = 2,
    Image = 3,
    Video = 4,
    Audio = 5,
    Archive = 6,
    Code = 7,
    Application = 8,
    System = 9,
}

impl Category {
    /// Decodes a category stored as a byte. Unknown values read as [`Category::Other`].
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => Category::Directory,
            2 => Category::Document,
            3 => Category::Image,
            4 => Category::Video,
            5 => Category::Audio,
            6 => Category::Archive,
            7 => Category::Code,
            8 => Category::Application,
            9 => Category::System,
            _ => Category::Other,
        }
    }

    /// Stable name for the frontend, which translates it.
    pub fn key(self) -> &'static str {
        match self {
            Category::Directory => "directory",
            Category::Other => "other",
            Category::Document => "document",
            Category::Image => "image",
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Archive => "archive",
            Category::Code => "code",
            Category::Application => "application",
            Category::System => "system",
        }
    }
}

/// Longest extension worth looking at. Longer suffixes are not extensions.
const MAX_EXTENSION: usize = 12;

/// The lowercase extension of a WTF-8 file name, if it has one.
pub fn extension(name: &[u8]) -> Option<([u8; MAX_EXTENSION], usize)> {
    let dot = name.iter().rposition(|&byte| byte == b'.')?;
    let raw = &name[dot + 1..];
    if dot == 0 || raw.is_empty() || raw.len() > MAX_EXTENSION {
        return None;
    }
    let mut lower = [0_u8; MAX_EXTENSION];
    for (target, source) in lower.iter_mut().zip(raw) {
        *target = source.to_ascii_lowercase();
    }
    Some((lower, raw.len()))
}

/// Classifies a file by its name.
pub fn categorize(name: &[u8]) -> Category {
    let Some((extension, length)) = extension(name) else {
        return Category::Other;
    };
    match &extension[..length] {
        b"doc" | b"docx" | b"odt" | b"rtf" | b"txt" | b"md" | b"pdf" | b"xls" | b"xlsx"
        | b"ods" | b"csv" | b"ppt" | b"pptx" | b"odp" | b"epub" | b"mobi" | b"tex" | b"pages"
        | b"numbers" | b"key" | b"one" | b"msg" | b"eml" => Category::Document,
        b"jpg" | b"jpeg" | b"png" | b"gif" | b"bmp" | b"tif" | b"tiff" | b"webp" | b"heic"
        | b"heif" | b"avif" | b"svg" | b"ico" | b"raw" | b"cr2" | b"cr3" | b"nef" | b"arw"
        | b"dng" | b"psd" | b"ai" | b"xcf" | b"exr" | b"hdr" | b"tga" | b"dds" => Category::Image,
        b"mp4" | b"mkv" | b"avi" | b"mov" | b"wmv" | b"webm" | b"m4v" | b"mpg" | b"mpeg"
        | b"flv" | b"ts" | b"m2ts" | b"vob" | b"3gp" => Category::Video,
        b"mp3" | b"flac" | b"wav" | b"aac" | b"ogg" | b"opus" | b"m4a" | b"wma" | b"aiff"
        | b"alac" | b"mid" | b"midi" => Category::Audio,
        b"zip" | b"7z" | b"rar" | b"tar" | b"gz" | b"bz2" | b"xz" | b"zst" | b"tgz" | b"cab"
        | b"iso" | b"img" | b"vhd" | b"vhdx" | b"vmdk" | b"qcow2" | b"wim" | b"esd" | b"dmg" => {
            Category::Archive
        }
        b"rs" | b"c" | b"h" | b"cpp" | b"hpp" | b"cc" | b"cs" | b"java" | b"kt" | b"go" | b"py"
        | b"rb" | b"php" | b"js" | b"jsx" | b"tsx" | b"mjs" | b"json" | b"toml" | b"yaml"
        | b"yml" | b"xml" | b"html" | b"htm" | b"css" | b"scss" | b"sql" | b"sh" | b"ps1"
        | b"bat" | b"cmd" | b"lua" | b"swift" | b"dart" | b"vue" | b"pdb" | b"map" | b"lock" => {
            Category::Code
        }
        b"exe" | b"msi" | b"msix" | b"appx" | b"apk" | b"jar" | b"deb" | b"rpm" | b"com"
        | b"scr" | b"pak" | b"unity3d" | b"asar" | b"bundle" => Category::Application,
        b"dll" | b"sys" | b"drv" | b"ocx" | b"mui" | b"cat" | b"inf" | b"log" | b"etl"
        | b"evtx" | b"dat" | b"bin" | b"db" | b"sqlite" | b"ldb" | b"tmp" | b"cache"
        | b"manifest" | b"mum" | b"dmp" | b"pf" | b"lib" | b"obj" | b"o" | b"a" | b"so" => {
            Category::System
        }
        _ => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_ignore_case_and_only_count_the_last_dot() {
        assert_eq!(categorize(b"Report.PDF"), Category::Document);
        assert_eq!(categorize(b"backup.tar.GZ"), Category::Archive);
        assert_eq!(categorize(b"song.flac"), Category::Audio);
        assert_eq!(categorize(b"setup.exe"), Category::Application);
    }

    #[test]
    fn names_without_a_real_extension_are_other() {
        assert_eq!(categorize(b"README"), Category::Other);
        assert_eq!(categorize(b".gitignore"), Category::Other);
        assert_eq!(categorize(b"trailing."), Category::Other);
        assert_eq!(categorize(b"a.reallylongsuffixhere"), Category::Other);
        assert_eq!(categorize(b"mystery.xyz"), Category::Other);
    }

    #[test]
    fn stored_bytes_round_trip() {
        for category in [
            Category::Directory,
            Category::Other,
            Category::Document,
            Category::Image,
            Category::Video,
            Category::Audio,
            Category::Archive,
            Category::Code,
            Category::Application,
            Category::System,
        ] {
            assert_eq!(Category::from_byte(category as u8), category);
        }
        assert_eq!(Category::from_byte(200), Category::Other);
    }
}
