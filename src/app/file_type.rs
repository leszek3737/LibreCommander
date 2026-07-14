use crate::app::mime::{self, ends_with_ignore_ascii_case};
use crate::app::types::FileCategory;

// Source / document / config stay table-driven: category precedence differs from
// MIME (e.g. `.md` is Document, not Code; config exact names and prefixes).
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

const CONFIG_SUFFIXES: &[&str] = &[
    ".json",
    ".jsonc",
    ".xml",
    ".toml",
    ".yaml",
    ".yml",
    ".ini",
    ".conf",
    ".cfg",
    ".config",
    ".cnf",
    ".properties",
    ".plist",
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
    ".gitattributes",
    ".gitmodules",
    ".dockerignore",
];

const CONFIG_EXACT_NAMES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "tsconfig.json",
    "CMakeLists.txt",
    ".gitignore",
    ".env",
    ".editorconfig",
    "Makefile",
    "Dockerfile",
    "Vagrantfile",
    "Rakefile",
    "Gemfile",
    "justfile",
    "Brewfile",
    "Containerfile",
    "Jenkinsfile",
];

/// Dotless basenames matched case-insensitively (same set as `mime::dotless_config_mime`).
const CONFIG_DOTLESS_CASEFOLD: &[&str] = &[
    "makefile",
    "dockerfile",
    "containerfile",
    "vagrantfile",
    "rakefile",
    "gemfile",
    "justfile",
    "brewfile",
    "jenkinsfile",
];

const CONFIG_PREFIXES: &[&str] = &[".env."];

#[inline]
fn has_any_suffix(name: &str, suffixes: &[&str]) -> bool {
    suffixes
        .iter()
        .any(|suffix| ends_with_ignore_ascii_case(name, suffix))
}

// Case-sensitivity via `cfg!(target_os)` (cosmetic icons/colors only).
#[inline]
fn name_is_case_sensitive() -> bool {
    cfg!(target_os = "linux")
}

#[inline]
fn exact_name_match(name: &str, expected: &str) -> bool {
    if name_is_case_sensitive() {
        name == expected
    } else {
        name.eq_ignore_ascii_case(expected)
    }
}

#[inline]
fn prefix_match(name: &str, prefix: &str) -> bool {
    let (name_bytes, prefix_bytes) = (name.as_bytes(), prefix.as_bytes());
    if name_bytes.len() < prefix_bytes.len() {
        return false;
    }
    let head = &name_bytes[..prefix_bytes.len()];
    if name_is_case_sensitive() {
        head == prefix_bytes
    } else {
        head.eq_ignore_ascii_case(prefix_bytes)
    }
}

/// Archive / compressed container MIME types produced by [`mime::extension_mime`].
fn is_archive_mime(m: &str) -> bool {
    matches!(
        m,
        "application/zip"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/x-7z-compressed"
            | "application/vnd.rar"
            | "application/x-rar-compressed"
            | "application/zstd"
            | "application/x-lzma"
            | "application/vnd.ms-cab-compressed"
            | "application/x-iso9660-image"
            | "application/x-apple-diskimage"
            | "application/x-debian-package"
            | "application/x-rpm"
            | "application/vnd.android.package-archive"
            | "application/x-unix-archive"
            | "application/x-cpio"
            | "application/java-archive"
            | "application/x-xar"
            | "application/x-ace"
            | "application/x-arj"
            | "application/x-lzop"
            | "application/x-brotli"
    )
}

#[inline]
pub fn is_archive(name: &str) -> bool {
    mime::extension_mime(name).is_some_and(is_archive_mime)
}

#[inline]
pub fn is_image(name: &str) -> bool {
    match mime::extension_mime(name) {
        // djvu is a document in the panel category (see DOCUMENT_SUFFIXES).
        Some(m) if m.starts_with("image/") && m != "image/vnd.djvu" => true,
        Some("application/postscript") => true,
        _ => false,
    }
}

#[inline]
pub fn is_video(name: &str) -> bool {
    match mime::extension_mime(name) {
        Some(m) if m.starts_with("video/") => true,
        Some("application/vnd.rn-realmedia") => true,
        _ => false,
    }
}

#[inline]
pub fn is_audio(name: &str) -> bool {
    mime::extension_mime(name).is_some_and(|m| m.starts_with("audio/"))
}

#[inline]
pub fn is_font(name: &str) -> bool {
    match mime::extension_mime(name) {
        Some(m) if m.starts_with("font/") => true,
        Some("application/vnd.ms-fontobject") => true,
        _ => false,
    }
}

#[inline]
pub fn is_source_code(name: &str) -> bool {
    has_any_suffix(name, SOURCE_CODE_SUFFIXES)
}

#[inline]
pub fn is_document(name: &str) -> bool {
    has_any_suffix(name, DOCUMENT_SUFFIXES)
}

#[inline]
pub fn is_config(name: &str) -> bool {
    CONFIG_EXACT_NAMES
        .iter()
        .any(|&n| exact_name_match(name, n))
        || CONFIG_DOTLESS_CASEFOLD
            .iter()
            .any(|&n| name.eq_ignore_ascii_case(n))
        || CONFIG_PREFIXES.iter().any(|&p| prefix_match(name, p))
        || has_any_suffix(name, CONFIG_SUFFIXES)
}

pub fn category(name: &str, is_dir: bool, is_exec: bool, is_link: bool) -> FileCategory {
    if is_link {
        return FileCategory::Symlink;
    }
    if is_dir {
        return FileCategory::Dir;
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
    if is_font(name) {
        return FileCategory::Font;
    }
    if is_exec {
        return FileCategory::Executable;
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
        assert!(!is_source_code("data.txt"));
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
        assert!(is_config("ignore.DOCKERIGNORE"));
    }

    #[test]
    fn test_dotless_config_casefold() {
        assert!(is_config("makefile"));
        assert!(is_config("Makefile"));
        assert!(is_config("MAKEFILE"));
        assert!(is_config("dockerfile"));
        assert!(is_config("Dockerfile"));
        assert_eq!(
            category("makefile", false, false, false),
            FileCategory::Config
        );
    }

    #[test]
    fn test_category_flags_take_priority() {
        assert_eq!(
            category("archive.zip", true, false, false),
            FileCategory::Dir
        );
        assert_eq!(
            category("archive.zip", false, false, true),
            FileCategory::Symlink
        );
        assert_eq!(
            category(".archive.zip", false, false, false),
            FileCategory::Archive
        );
        assert_eq!(
            category("mybinary", false, true, false),
            FileCategory::Executable
        );
    }

    #[test]
    fn test_category_by_extension() {
        assert_eq!(
            category("archive.tar.zst", false, false, false),
            FileCategory::Archive
        );
        assert_eq!(
            category("photo.avif", false, false, false),
            FileCategory::Image
        );
        assert_eq!(
            category("movie.webm", false, false, false),
            FileCategory::Video
        );
        assert_eq!(
            category("component.ts", false, false, false),
            FileCategory::Code
        );
        assert_eq!(
            category("song.flac", false, false, false),
            FileCategory::Audio
        );
        assert_eq!(
            category("manual.pdf", false, false, false),
            FileCategory::Document
        );
        assert_eq!(category("main.rs", false, false, false), FileCategory::Code);
        assert_eq!(
            category("config.toml", false, false, false),
            FileCategory::Config
        );
        assert_eq!(
            category("font.woff2", false, false, false),
            FileCategory::Font
        );
        assert_eq!(
            category("file.unknown", false, false, false),
            FileCategory::Other
        );
    }

    #[test]
    fn test_hidden_file_gets_real_type() {
        assert_eq!(
            category(".script.sh", false, false, false),
            FileCategory::Code
        );
    }

    #[test]
    fn test_hidden_archive_is_archive() {
        assert_eq!(
            category(".backup.zip", false, false, false),
            FileCategory::Archive
        );
    }

    #[test]
    fn test_hidden_image_is_image() {
        assert_eq!(
            category(".photo.jpg", false, false, false),
            FileCategory::Image
        );
    }

    #[test]
    fn test_symlink_overrides_everything() {
        assert_eq!(category("link", true, false, true), FileCategory::Symlink);
        assert_eq!(
            category(".hidden_link", false, false, true),
            FileCategory::Symlink
        );
        assert_eq!(
            category("exec_link", false, true, true),
            FileCategory::Symlink
        );
    }

    #[test]
    fn test_extensionless_executable_is_executable() {
        assert_eq!(
            category("mybinary", false, true, false),
            FileCategory::Executable
        );
    }

    #[test]
    fn test_case_insensitive_matching() {
        assert!(is_archive("FILE.ZIP"));
        assert!(is_image("PHOTO.JPG"));
        assert!(is_source_code("MAIN.RS"));
        assert!(is_config("SETTINGS.JSON"));
    }

    #[test]
    fn test_empty_name_no_crash() {
        assert!(!is_archive(""));
        assert!(!is_image(""));
        assert!(!is_source_code(""));
    }

    #[test]
    fn test_short_name_no_crash() {
        assert!(!is_archive("."));
        assert!(!is_image(".z"));
        assert!(!is_source_code("a"));
    }
}
