use crate::app::types::FileCategory;

const ARCHIVE_SUFFIXES: &[&str] = &[
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".zip", ".tar", ".gz", ".bz2", ".xz", ".zst",
    ".7z", ".rar", ".tgz", ".tbz", ".tbz2", ".txz", ".tzst", ".lz", ".lzma", ".lzo", ".br", ".cab",
    ".iso", ".dmg", ".pkg", ".deb", ".rpm", ".apk", ".ar", ".cpio", ".jar", ".war", ".ear", ".xar",
    ".z", ".ace", ".arj",
];

const IMAGE_SUFFIXES: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".svg", ".webp", ".avif", ".heic", ".heif", ".tif",
    ".tiff", ".ico", ".icns", ".raw", ".cr2", ".nef", ".orf", ".arw", ".dng", ".psd", ".xcf",
    ".ai", ".eps",
];

const SOURCE_CODE_SUFFIXES: &[&str] = &[
    ".rs", ".py", ".pyw", ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".c", ".h", ".cc", ".hh",
    ".cpp", ".cxx", ".hpp", ".hxx", ".go", ".java", ".kt", ".kts", ".swift", ".m", ".mm", ".cs",
    ".fs", ".fsx", ".vb", ".php", ".rb", ".lua", ".pl", ".pm", ".r", ".jl", ".scala", ".sc",
    ".clj", ".cljs", ".ex", ".exs", ".erl", ".hrl", ".hs", ".lhs", ".ml", ".mli", ".nim", ".zig",
    ".v", ".sv", ".dart", ".sh", ".bash", ".zsh", ".fish", ".ps1", ".bat", ".cmd", ".sql", ".html",
    ".htm", ".css", ".scss", ".sass", ".less", ".vue", ".svelte", ".wasm",
];

const DOCUMENT_SUFFIXES: &[&str] = &[
    ".pdf",
    ".doc",
    ".docx",
    ".xls",
    ".xlsx",
    ".ppt",
    ".pptx",
    ".odt",
    ".ods",
    ".odp",
    ".rtf",
    ".txt",
    ".md",
    ".markdown",
    ".rst",
    ".adoc",
    ".tex",
    ".epub",
    ".mobi",
    ".azw",
    ".azw3",
    ".djvu",
    ".chm",
    ".csv",
    ".tsv",
    ".log",
];

const AUDIO_SUFFIXES: &[&str] = &[
    ".mp3", ".wav", ".flac", ".ogg", ".oga", ".opus", ".m4a", ".aac", ".wma", ".aiff", ".aif",
    ".alac", ".ape", ".mid", ".midi", ".mpc", ".amr", ".au",
];

const VIDEO_SUFFIXES: &[&str] = &[
    ".mp4", ".avi", ".mkv", ".mov", ".webm", ".m4v", ".mpg", ".mpeg", ".wmv", ".flv", ".ogv",
    ".3gp", ".3g2", ".mts", ".m2ts", ".vob", ".rm", ".rmvb", ".asf",
];

const CONFIG_SUFFIXES: &[&str] = &[
    ".json",
    ".jsonc",
    ".toml",
    ".yaml",
    ".yml",
    ".ini",
    ".conf",
    ".cfg",
    ".config",
    ".cnf",
    ".env",
    ".properties",
    ".plist",
    ".xml",
    ".desktop",
    ".service",
    ".timer",
    ".socket",
    ".mount",
    ".automount",
    ".target",
    ".path",
    ".slice",
    ".scope",
    ".lock",
    ".editorconfig",
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
    ".dockerignore",
];

fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.get(s.len().saturating_sub(suffix.len())..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
}

fn has_any_suffix(name: &str, suffixes: &[&str]) -> bool {
    suffixes
        .iter()
        .any(|suffix| ends_with_ignore_ascii_case(name, suffix))
}

pub fn is_archive(name: &str) -> bool {
    has_any_suffix(name, ARCHIVE_SUFFIXES)
}

pub fn is_image(name: &str) -> bool {
    has_any_suffix(name, IMAGE_SUFFIXES)
}

pub fn is_source_code(name: &str) -> bool {
    has_any_suffix(name, SOURCE_CODE_SUFFIXES)
}

pub fn is_document(name: &str) -> bool {
    has_any_suffix(name, DOCUMENT_SUFFIXES)
}

pub fn is_audio(name: &str) -> bool {
    has_any_suffix(name, AUDIO_SUFFIXES)
}

pub fn is_video(name: &str) -> bool {
    has_any_suffix(name, VIDEO_SUFFIXES)
}

pub fn is_config(name: &str) -> bool {
    has_any_suffix(name, CONFIG_SUFFIXES)
}

pub fn category(
    name: &str,
    is_dir: bool,
    is_exec: bool,
    is_link: bool,
    is_hidden: bool,
) -> FileCategory {
    if is_dir {
        return FileCategory::Dir;
    }
    if is_link {
        return FileCategory::Symlink;
    }
    if is_hidden {
        return FileCategory::Hidden;
    }
    if is_exec {
        return FileCategory::Executable;
    }
    if is_source_code(name) {
        return FileCategory::Code;
    }
    if is_config(name) {
        return FileCategory::Config;
    }
    if is_archive(name) {
        return FileCategory::Archive;
    }
    if is_image(name) {
        return FileCategory::Image;
    }
    if is_video(name) {
        return FileCategory::Video;
    }
    if is_audio(name) {
        return FileCategory::Audio;
    }
    if is_document(name) {
        return FileCategory::Document;
    }
    FileCategory::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_archive_tar() {
        assert!(is_archive("file.tar"));
        assert!(is_archive("archive.TAR"));
        assert!(is_archive("backup.tar.gz"));
        assert!(is_archive("backup.tar.bz2"));
        assert!(is_archive("backup.tar.xz"));
        assert!(is_archive("backup.tar.zst"));
    }

    #[test]
    fn test_is_archive_zip() {
        assert!(is_archive("files.zip"));
        assert!(is_archive("data.7z"));
        assert!(is_archive("backup.rar"));
        assert!(is_archive("package.deb"));
        assert!(is_archive("disk.iso"));
    }

    #[test]
    fn test_is_archive_negative() {
        assert!(!is_archive("document.txt"));
        assert!(!is_archive("image.png"));
    }

    #[test]
    fn test_is_image_jpg() {
        assert!(is_image("photo.jpg"));
        assert!(is_image("image.JPEG"));
    }

    #[test]
    fn test_is_image_png() {
        assert!(is_image("screenshot.png"));
        assert!(is_image("icon.PNG"));
        assert!(is_image("picture.webp"));
        assert!(is_image("photo.HEIC"));
    }

    #[test]
    fn test_is_image_negative() {
        assert!(!is_image("document.txt"));
        assert!(!is_image("code.rs"));
    }

    #[test]
    fn test_is_source_code_rust() {
        assert!(is_source_code("main.rs"));
        assert!(is_source_code("lib.RS"));
    }

    #[test]
    fn test_is_source_code_python() {
        assert!(is_source_code("script.py"));
        assert!(is_source_code("module.PY"));
    }

    #[test]
    fn test_is_source_code_js() {
        assert!(is_source_code("app.js"));
        assert!(is_source_code("component.ts"));
        assert!(is_source_code("component.tsx"));
        assert!(is_source_code("script.sh"));
    }

    #[test]
    fn test_is_source_code_negative() {
        assert!(!is_source_code("image.png"));
        assert!(!is_source_code("data.json"));
    }

    #[test]
    fn test_new_document_extensions() {
        assert!(is_document("notes.md"));
        assert!(is_document("book.epub"));
        assert!(is_document("slides.pptx"));
    }

    #[test]
    fn test_new_audio_extensions() {
        assert!(is_audio("track.opus"));
        assert!(is_audio("voice.aac"));
    }

    #[test]
    fn test_new_video_extensions() {
        assert!(is_video("clip.m4v"));
        assert!(is_video("movie.mpeg"));
        assert!(!is_video("component.ts"));
    }

    #[test]
    fn test_new_config_extensions() {
        assert!(is_config("settings.jsonc"));
        assert!(is_config(".editorconfig"));
        assert!(is_config("ignore.GITIGNORE"));
    }

    #[test]
    fn test_category_flags_take_priority() {
        assert_eq!(
            category("archive.zip", true, false, false, false),
            FileCategory::Dir
        );
        assert_eq!(
            category("archive.zip", false, false, true, false),
            FileCategory::Symlink
        );
        assert_eq!(
            category(".archive.zip", false, false, false, true),
            FileCategory::Hidden
        );
        assert_eq!(
            category("archive.zip", false, true, false, false),
            FileCategory::Executable
        );
    }

    #[test]
    fn test_category_by_extension() {
        assert_eq!(
            category("archive.tar.zst", false, false, false, false),
            FileCategory::Archive
        );
        assert_eq!(
            category("photo.avif", false, false, false, false),
            FileCategory::Image
        );
        assert_eq!(
            category("movie.webm", false, false, false, false),
            FileCategory::Video
        );
        assert_eq!(
            category("component.ts", false, false, false, false),
            FileCategory::Code
        );
        assert_eq!(
            category("song.flac", false, false, false, false),
            FileCategory::Audio
        );
        assert_eq!(
            category("manual.pdf", false, false, false, false),
            FileCategory::Document
        );
        assert_eq!(
            category("main.rs", false, false, false, false),
            FileCategory::Code
        );
        assert_eq!(
            category("config.toml", false, false, false, false),
            FileCategory::Config
        );
        assert_eq!(
            category("file.unknown", false, false, false, false),
            FileCategory::Other
        );
    }
}
