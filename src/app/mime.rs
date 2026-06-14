use std::path::Path;

use crate::app::types::FileCategory;

// Shared MIME literals: single source of truth for strings that appear both in
// the public classification tables below and in `mime_to_category`'s match arms.
// Naming them once stops the tables and the match from silently drifting apart
// (e.g. a value fixed in one place but forgotten in the other).
//
// MIME prefixes (tested with `str::starts_with`).
const PREFIX_IMAGE: &str = "image/";
const PREFIX_AUDIO: &str = "audio/";
const PREFIX_VIDEO: &str = "video/";
const PREFIX_OPENDOCUMENT: &str = "application/vnd.oasis.opendocument.";
const PREFIX_OOXML: &str = "application/vnd.openxmlformats-officedocument.";
//
// Text-bearing `application/*` MIME types.
const MIME_JSON: &str = "application/json";
const MIME_TOML: &str = "application/toml";
const MIME_YAML: &str = "application/yaml";
const MIME_X_YAML: &str = "application/x-yaml";
const MIME_XML: &str = "application/xml";
const MIME_JAVASCRIPT: &str = "application/javascript";
const MIME_TYPESCRIPT: &str = "application/typescript";
const MIME_ECMASCRIPT: &str = "application/ecmascript";
const MIME_SQL: &str = "application/sql";
const MIME_PHP: &str = "application/x-httpd-php";
const MIME_SH: &str = "application/x-sh";
const MIME_RTF: &str = "application/rtf";
//
// Binary `application/*` MIME types.
const MIME_ZIP: &str = "application/zip";
const MIME_TAR: &str = "application/x-tar";
const MIME_GZIP: &str = "application/gzip";
const MIME_X_GZIP: &str = "application/x-gzip";
const MIME_BZIP2: &str = "application/x-bzip2";
const MIME_XZ: &str = "application/x-xz";
const MIME_7Z: &str = "application/x-7z-compressed";
const MIME_RAR: &str = "application/vnd.rar";
const MIME_RAR_COMPRESSED: &str = "application/x-rar-compressed";
const MIME_ZSTD: &str = "application/zstd";
const MIME_PDF: &str = "application/pdf";
const MIME_MSWORD: &str = "application/msword";
const MIME_EPUB: &str = "application/epub+zip";
const MIME_WASM: &str = "application/wasm";

pub const TEXT_APPLICATION_MIMES: &[&str] = &[
    MIME_JSON,
    MIME_TOML,
    MIME_YAML,
    MIME_X_YAML,
    MIME_XML,
    MIME_JAVASCRIPT,
    MIME_TYPESCRIPT,
    MIME_ECMASCRIPT,
    MIME_SQL,
    MIME_PHP,
    MIME_SH,
    MIME_RTF,
];

pub const KNOWN_BINARY_MIMES: &[&str] = &[
    "application/octet-stream",
    MIME_ZIP,
    MIME_TAR,
    MIME_GZIP,
    MIME_X_GZIP,
    MIME_BZIP2,
    MIME_XZ,
    MIME_7Z,
    MIME_RAR,
    MIME_RAR_COMPRESSED,
    MIME_ZSTD,
    MIME_PDF,
    MIME_MSWORD,
    MIME_EPUB,
    MIME_WASM,
    "application/x-mach-binary",
    "application/x-dosexec",
    "application/x-executable",
    "application/x-sharedlib",
    "application/x-object",
];

pub const KNOWN_BINARY_PREFIXES: &[&str] = &[
    PREFIX_IMAGE,
    PREFIX_AUDIO,
    PREFIX_VIDEO,
    PREFIX_OPENDOCUMENT,
    PREFIX_OOXML,
    "application/vnd.ms-",
];

pub fn detect_mime_from_bytes(path: &Path, bytes: &[u8]) -> Option<&'static str> {
    // Both sources already yield `&'static str` (`infer` returns static MIME
    // strings, `extension_mime` returns table literals), so no allocation is
    // needed.
    infer::get(bytes).map(|kind| kind.mime_type()).or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(extension_mime)
    })
}

#[must_use]
pub fn mime_to_category(mime: &str) -> FileCategory {
    if mime == "inode/directory" {
        return FileCategory::Dir;
    }
    if mime.starts_with(PREFIX_IMAGE) {
        return if mime == "image/vnd.djvu" {
            FileCategory::Document
        } else {
            FileCategory::Image
        };
    }
    if mime.starts_with(PREFIX_AUDIO) {
        return FileCategory::Audio;
    }
    if mime.starts_with(PREFIX_VIDEO) {
        return FileCategory::Video;
    }
    if mime.starts_with("text/") {
        return match mime {
            "text/plain"
            | "text/markdown"
            | "text/csv"
            | "text/tab-separated-values"
            | "text/x-rst" => FileCategory::Document,
            "text/x-asciidoc" => FileCategory::Document,
            "text/xml" | "text/yaml" | "text/config" | "text/x-makefile" | "text/x-dockerfile"
            | "text/x-justfile" | "text/x-groovy" => FileCategory::Config,
            _ => FileCategory::Code,
        };
    }
    if mime.starts_with("application/") {
        return match mime {
            MIME_JSON | MIME_TOML | MIME_YAML | MIME_X_YAML | MIME_XML => FileCategory::Config,
            MIME_PDF
            | MIME_MSWORD
            | MIME_RTF
            | MIME_EPUB
            | "application/x-mobipocket-ebook"
            | "application/vnd.amazon.ebook"
            | "application/vnd.ms-htmlhelp"
            | "application/x-tex" => FileCategory::Document,
            m if m.starts_with(PREFIX_OPENDOCUMENT) => FileCategory::Document,
            m if m.starts_with(PREFIX_OOXML) => FileCategory::Document,
            MIME_JAVASCRIPT | MIME_TYPESCRIPT | MIME_ECMASCRIPT | MIME_SQL | MIME_WASM
            | MIME_PHP | MIME_SH => FileCategory::Code,
            "application/vnd.ms-fontobject" => FileCategory::Font,
            // NOTE: phf/trie intentionally avoided here — a linear match is fine
            // at TUI scale. Shared MIME constants keep these arms in sync with
            // the public tables above.
            MIME_ZIP
            | MIME_TAR
            | MIME_GZIP
            | MIME_X_GZIP
            | MIME_BZIP2
            | MIME_XZ
            | MIME_7Z
            | MIME_RAR
            | MIME_RAR_COMPRESSED
            | MIME_ZSTD
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
            | "application/x-brotli" => FileCategory::Archive,
            "application/vnd.rn-realmedia" => FileCategory::Video,
            "application/x-plist" => FileCategory::Config,
            "application/postscript" => FileCategory::Image,
            _ => FileCategory::Other,
        };
    }
    if mime.starts_with("font/") {
        return FileCategory::Font;
    }
    FileCategory::Other
}
/// Determine [`FileCategory`] from a filename (basename or full path).
///
/// Tries MIME detection first via [`extension_mime`], then falls back to
/// [`file_type::category`](crate::app::file_type::category).
#[must_use]
pub fn category_from_ext(name: &str) -> FileCategory {
    let basename = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if let Some(mime) = extension_mime(basename) {
        return mime_to_category(mime);
    }
    crate::app::file_type::category(basename, false, false, false)
}

fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    let (s, suffix) = (s.as_bytes(), suffix.as_bytes());
    s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

#[must_use]
fn dotless_config_mime(name: &str) -> Option<&'static str> {
    // ASCII case-insensitive compare without allocating a lowercased copy.
    let eq = |candidate: &str| name.eq_ignore_ascii_case(candidate);
    if eq("makefile") {
        Some("text/x-makefile")
    } else if eq("dockerfile") || eq("containerfile") {
        Some("text/x-dockerfile")
    } else if eq("vagrantfile") || eq("rakefile") || eq("gemfile") || eq("brewfile") {
        Some("text/x-ruby")
    } else if eq("justfile") {
        Some("text/x-justfile")
    } else if eq("jenkinsfile") {
        Some("text/x-groovy")
    } else {
        None
    }
}

/// Determine MIME type from a filename (basename or full path).
///
/// Extracts the basename from `name`, checks dotless config files
/// (Makefile, Dockerfile, etc.), compound extensions (.tar.gz, .tar.bz2, …),
/// then falls back to single-extension lookup.
#[must_use]
pub fn extension_mime(name: &str) -> Option<&'static str> {
    let basename = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if let Some(mime) = dotless_config_mime(basename) {
        return Some(mime);
    }

    if ends_with_ignore_ascii_case(basename, ".tar.gz")
        || ends_with_ignore_ascii_case(basename, ".tgz")
    {
        return Some("application/gzip");
    }
    if ends_with_ignore_ascii_case(basename, ".tar.bz2")
        || ends_with_ignore_ascii_case(basename, ".tbz")
        || ends_with_ignore_ascii_case(basename, ".tbz2")
    {
        return Some("application/x-bzip2");
    }
    if ends_with_ignore_ascii_case(basename, ".tar.xz")
        || ends_with_ignore_ascii_case(basename, ".txz")
    {
        return Some("application/x-xz");
    }

    let ext = basename.rsplit_once('.')?.1.to_ascii_lowercase();

    image_mime(&ext)
        .or_else(|| video_mime(&ext))
        .or_else(|| audio_mime(&ext))
        .or_else(|| archive_mime(&ext))
        .or_else(|| document_mime(&ext))
        .or_else(|| config_mime(&ext))
        .or_else(|| code_mime(&ext))
        .or_else(|| font_mime(&ext))
}

#[must_use]
fn image_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "tif" | "tiff" => Some("image/tiff"),
        "avif" => Some("image/avif"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),
        "icns" => Some("image/icns"),
        "raw" => Some("image/x-raw"),
        "cr2" => Some("image/x-canon-cr2"),
        "nef" => Some("image/x-nikon-nef"),
        "arw" => Some("image/x-sony-arw"),
        "dng" => Some("image/x-adobe-dng"),
        "orf" => Some("image/x-olympus-orf"),
        "psd" => Some("image/vnd.adobe.photoshop"),
        "xcf" => Some("image/x-xcf"),
        "ai" | "eps" => Some("application/postscript"),
        _ => None,
    }
}

#[must_use]
fn video_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "mp4" => Some("video/mp4"),
        "m4v" => Some("video/x-m4v"),
        "mkv" => Some("video/x-matroska"),
        "mov" => Some("video/quicktime"),
        "avi" => Some("video/x-msvideo"),
        "webm" => Some("video/webm"),
        "mpeg" | "mpg" => Some("video/mpeg"),
        "wmv" => Some("video/x-ms-wm"),
        "flv" => Some("video/x-flv"),
        "ogv" => Some("video/ogg"),
        "3gp" => Some("video/3gpp"),
        "3g2" => Some("video/3gpp2"),
        "mts" | "m2ts" => Some("video/mp2t"),
        "vob" => Some("video/mpeg"),
        "rm" | "rmvb" => Some("application/vnd.rn-realmedia"),
        "asf" => Some("video/x-ms-asf"),
        _ => None,
    }
}

#[must_use]
fn audio_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "flac" => Some("audio/flac"),
        "ogg" | "oga" => Some("audio/ogg"),
        "m4a" => Some("audio/mp4"),
        "aac" => Some("audio/aac"),
        "opus" => Some("audio/opus"),
        "wma" => Some("audio/x-ms-wma"),
        "aiff" | "aif" => Some("audio/aiff"),
        "mid" | "midi" => Some("audio/midi"),
        "amr" => Some("audio/amr"),
        "alac" => Some("audio/mp4"),
        "ape" => Some("audio/ape"),
        "mpc" => Some("audio/musepack"),
        "au" => Some("audio/basic"),
        _ => None,
    }
}

#[must_use]
fn archive_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "zip" => Some("application/zip"),
        "tar" => Some("application/x-tar"),
        "gz" => Some("application/gzip"),
        "bz2" => Some("application/x-bzip2"),
        "xz" => Some("application/x-xz"),
        "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/vnd.rar"),
        "zst" => Some("application/zstd"),
        "lz" | "lzma" => Some("application/x-lzma"),
        "cab" => Some("application/vnd.ms-cab-compressed"),
        "iso" => Some("application/x-iso9660-image"),
        "dmg" => Some("application/x-apple-diskimage"),
        "deb" => Some("application/x-debian-package"),
        "rpm" => Some("application/x-rpm"),
        "apk" => Some("application/vnd.android.package-archive"),
        "ar" => Some("application/x-unix-archive"),
        "cpio" => Some("application/x-cpio"),
        "jar" | "war" | "ear" => Some("application/java-archive"),
        "pkg" | "xar" => Some("application/x-xar"),
        "ace" => Some("application/x-ace"),
        "arj" => Some("application/x-arj"),
        "lzo" => Some("application/x-lzop"),
        "br" => Some("application/x-brotli"),
        "z" => Some("application/gzip"),
        _ => None,
    }
}

#[must_use]
fn document_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some("application/pdf"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        "rtf" => Some("application/rtf"),
        "csv" => Some("text/csv"),
        "tsv" => Some("text/tab-separated-values"),
        "epub" => Some("application/epub+zip"),
        "djvu" => Some("image/vnd.djvu"),
        "mobi" => Some("application/x-mobipocket-ebook"),
        "azw" | "azw3" => Some("application/vnd.amazon.ebook"),
        "chm" => Some("application/vnd.ms-htmlhelp"),
        "tex" => Some("application/x-tex"),
        "txt" | "log" => Some("text/plain"),
        "rst" => Some("text/x-rst"),
        "adoc" => Some("text/x-asciidoc"),
        _ => None,
    }
}

#[must_use]
fn config_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "json" => Some("application/json"),
        "jsonc" => Some("application/json"),
        "toml" => Some("application/toml"),
        "yaml" | "yml" => Some("application/yaml"),
        "xml" => Some("application/xml"),
        "ini" | "conf" | "cfg" => Some("text/config"),
        "plist" => Some("application/x-plist"),
        "lock" => Some("text/config"),
        "config" | "cnf" | "env" | "properties" | "desktop" | "gitignore" | "gitattributes"
        | "gitmodules" | "dockerignore" | "editorconfig" => Some("text/config"),
        _ => None,
    }
}

#[must_use]
fn code_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "md" | "markdown" => Some("text/markdown"),
        "rs" => Some("text/x-rust"),
        "py" | "pyw" => Some("text/x-python"),
        "js" | "mjs" | "cjs" => Some("application/javascript"),
        "ts" | "tsx" => Some("application/typescript"),
        "jsx" => Some("text/jsx"),
        "c" => Some("text/x-c"),
        "h" => Some("text/x-c-header"),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some("text/x-c++"),
        "go" => Some("text/x-go"),
        "java" => Some("text/x-java-source"),
        "kt" | "kts" => Some("text/x-kotlin"),
        "rb" => Some("text/x-ruby"),
        "php" => Some("application/x-httpd-php"),
        "sh" | "bash" | "zsh" | "fish" => Some("application/x-sh"),
        "html" | "htm" => Some("text/html"),
        "css" => Some("text/css"),
        "sql" => Some("application/sql"),
        "wasm" => Some("application/wasm"),
        "swift" => Some("text/x-swift"),
        "cs" => Some("text/x-csharp"),
        "lua" => Some("text/x-lua"),
        "pl" | "pm" => Some("text/x-perl"),
        "r" => Some("text/x-r"),
        "jl" => Some("text/x-julia"),
        "scala" | "sc" => Some("text/x-scala"),
        "clj" | "cljs" => Some("text/x-clojure"),
        "ex" | "exs" => Some("text/x-elixir"),
        "erl" | "hrl" => Some("text/x-erlang"),
        "hs" | "lhs" => Some("text/x-haskell"),
        "ml" | "mli" => Some("text/x-ocaml"),
        "nim" => Some("text/x-nim"),
        "zig" => Some("text/x-zig"),
        "dart" => Some("text/x-dart"),
        "ps1" => Some("text/x-shellscript"),
        "bat" | "cmd" => Some("text/x-msdos-batch"),
        "scss" | "sass" | "less" => Some("text/x-scss"),
        "vue" => Some("text/x-vue"),
        "svelte" => Some("text/x-svelte"),
        "m" => Some("text/x-objective-c"),
        "mm" => Some("text/x-objective-c++"),
        "fs" | "fsx" => Some("text/x-fsharp"),
        "vb" => Some("text/x-vb"),
        "v" | "sv" => Some("text/x-verilog"),
        _ => None,
    }
}

#[must_use]
fn font_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "eot" => Some("application/vnd.ms-fontobject"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_from_ext_maps_common_media() {
        assert_eq!(category_from_ext("photo.JPG"), FileCategory::Image);
        assert_eq!(category_from_ext("movie.webm"), FileCategory::Video);
        assert_eq!(category_from_ext("song.flac"), FileCategory::Audio);
        assert_eq!(category_from_ext("icon.heic"), FileCategory::Image);
        assert_eq!(category_from_ext("photo.heif"), FileCategory::Image);
        assert_eq!(category_from_ext("logo.icns"), FileCategory::Image);
    }

    #[test]
    fn category_from_ext_maps_archives_documents_config_and_code() {
        assert_eq!(category_from_ext("backup.tar.gz"), FileCategory::Archive);
        assert_eq!(category_from_ext("report.pdf"), FileCategory::Document);
        assert_eq!(category_from_ext("config.toml"), FileCategory::Config);
        assert_eq!(category_from_ext("settings.ini"), FileCategory::Config);
        assert_eq!(category_from_ext("main.rs"), FileCategory::Code);
        assert_eq!(category_from_ext("readme.txt"), FileCategory::Document);
        assert_eq!(category_from_ext("unknown.bin"), FileCategory::Other);
        assert_eq!(category_from_ext("movie.rm"), FileCategory::Video);
        assert_eq!(category_from_ext("movie.rmvb"), FileCategory::Video);
        assert_eq!(category_from_ext("logo.ai"), FileCategory::Image);
        assert_eq!(category_from_ext("logo.eps"), FileCategory::Image);
        assert_eq!(category_from_ext("settings.plist"), FileCategory::Config);
    }

    #[test]
    fn category_from_ext_new_document_types() {
        assert_eq!(category_from_ext("data.csv"), FileCategory::Document);
        assert_eq!(category_from_ext("data.tsv"), FileCategory::Document);
        assert_eq!(category_from_ext("book.epub"), FileCategory::Document);
        assert_eq!(category_from_ext("scan.djvu"), FileCategory::Document);
    }

    #[test]
    fn category_from_ext_new_archive_types() {
        assert_eq!(category_from_ext("pkg.deb"), FileCategory::Archive);
        assert_eq!(category_from_ext("pkg.rpm"), FileCategory::Archive);
        assert_eq!(category_from_ext("app.apk"), FileCategory::Archive);
        assert_eq!(category_from_ext("disk.iso"), FileCategory::Archive);
        assert_eq!(category_from_ext("disk.dmg"), FileCategory::Archive);
        assert_eq!(category_from_ext("lib.jar"), FileCategory::Archive);
    }

    #[test]
    fn category_from_ext_config_types() {
        assert_eq!(category_from_ext("tsconfig.jsonc"), FileCategory::Config);
    }

    #[test]
    fn category_from_ext_font_types() {
        assert_eq!(category_from_ext("font.ttf"), FileCategory::Font);
        assert_eq!(category_from_ext("font.woff2"), FileCategory::Font);
        assert_eq!(category_from_ext("font.eot"), FileCategory::Font);
    }

    #[test]
    fn mime_to_category_maps_primary_types() {
        assert_eq!(mime_to_category("inode/directory"), FileCategory::Dir);
        assert_eq!(mime_to_category("image/png"), FileCategory::Image);
        assert_eq!(mime_to_category("image/heic"), FileCategory::Image);
        assert_eq!(mime_to_category("audio/mpeg"), FileCategory::Audio);
        assert_eq!(mime_to_category("video/mp4"), FileCategory::Video);
        assert_eq!(mime_to_category("text/x-rust"), FileCategory::Code);
        assert_eq!(mime_to_category("text/plain"), FileCategory::Document);
        assert_eq!(mime_to_category("text/markdown"), FileCategory::Document);
        assert_eq!(mime_to_category("text/xml"), FileCategory::Config);
        assert_eq!(mime_to_category("text/yaml"), FileCategory::Config);
        assert_eq!(mime_to_category("text/csv"), FileCategory::Document);
        assert_eq!(
            mime_to_category("text/tab-separated-values"),
            FileCategory::Document
        );
        assert_eq!(
            mime_to_category("application/epub+zip"),
            FileCategory::Document
        );
        assert_eq!(mime_to_category("image/vnd.djvu"), FileCategory::Document);
        assert_eq!(mime_to_category("font/ttf"), FileCategory::Font);
        assert_eq!(
            mime_to_category("application/vnd.ms-fontobject"),
            FileCategory::Font
        );
    }

    #[test]
    fn mime_to_category_maps_structured_application_types() {
        assert_eq!(mime_to_category("application/json"), FileCategory::Config);
        assert_eq!(mime_to_category("application/zip"), FileCategory::Archive);
        assert_eq!(mime_to_category("application/pdf"), FileCategory::Document);
        assert_eq!(mime_to_category("application/wasm"), FileCategory::Code);
        assert_eq!(
            mime_to_category("application/octet-stream"),
            FileCategory::Other
        );
        assert_eq!(
            mime_to_category("application/x-debian-package"),
            FileCategory::Archive
        );
        assert_eq!(mime_to_category("application/x-rpm"), FileCategory::Archive);
        assert_eq!(
            mime_to_category("application/java-archive"),
            FileCategory::Archive
        );
        assert_eq!(
            mime_to_category("application/x-iso9660-image"),
            FileCategory::Archive
        );
        assert_eq!(
            mime_to_category("application/x-apple-diskimage"),
            FileCategory::Archive
        );
        assert_eq!(
            mime_to_category("application/vnd.rn-realmedia"),
            FileCategory::Video
        );
        assert_eq!(
            mime_to_category("application/postscript"),
            FileCategory::Image
        );
        assert_eq!(
            mime_to_category("application/x-plist"),
            FileCategory::Config
        );
    }

    #[test]
    fn extension_mime_image_document_config() {
        assert_eq!(extension_mime("test.heic"), Some("image/heic"));
        assert_eq!(extension_mime("test.heif"), Some("image/heif"));
        assert_eq!(extension_mime("test.icns"), Some("image/icns"));
        assert_eq!(extension_mime("test.csv"), Some("text/csv"));
        assert_eq!(
            extension_mime("test.tsv"),
            Some("text/tab-separated-values")
        );
        assert_eq!(extension_mime("test.epub"), Some("application/epub+zip"));
        assert_eq!(extension_mime("test.djvu"), Some("image/vnd.djvu"));
        assert_eq!(extension_mime("test.jsonc"), Some("application/json"));
        assert_eq!(
            extension_mime("test.psd"),
            Some("image/vnd.adobe.photoshop")
        );
        assert_eq!(extension_mime("test.xcf"), Some("image/x-xcf"));
    }

    #[test]
    fn extension_mime_archive_font_media() {
        assert_eq!(
            extension_mime("test.deb"),
            Some("application/x-debian-package")
        );
        assert_eq!(extension_mime("test.rpm"), Some("application/x-rpm"));
        assert_eq!(
            extension_mime("test.apk"),
            Some("application/vnd.android.package-archive")
        );
        assert_eq!(
            extension_mime("test.iso"),
            Some("application/x-iso9660-image")
        );
        assert_eq!(
            extension_mime("test.dmg"),
            Some("application/x-apple-diskimage")
        );
        assert_eq!(extension_mime("test.jar"), Some("application/java-archive"));
        assert_eq!(extension_mime("test.ttf"), Some("font/ttf"));
        assert_eq!(extension_mime("test.woff2"), Some("font/woff2"));
        assert_eq!(extension_mime("test.wmv"), Some("video/x-ms-wm"));
        assert_eq!(extension_mime("test.flv"), Some("video/x-flv"));
        assert_eq!(extension_mime("test.ogv"), Some("video/ogg"));
        assert_eq!(extension_mime("test.3gp"), Some("video/3gpp"));
        assert_eq!(extension_mime("test.wma"), Some("audio/x-ms-wma"));
        assert_eq!(extension_mime("test.aiff"), Some("audio/aiff"));
        assert_eq!(extension_mime("test.mid"), Some("audio/midi"));
    }
}
