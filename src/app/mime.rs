use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::app::types::FileCategory;

pub fn detect_mime(path: &Path) -> Option<String> {
    if path.is_dir() {
        return Some("inode/directory".to_string());
    }

    let fallback = || {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(extension_mime)
            .map(str::to_string)
    };

    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return fallback(),
    };
    let mut buf = [0; 8192];
    let len = match file.read(&mut buf) {
        Ok(len) => len,
        Err(_) => return fallback(),
    };

    detect_mime_from_bytes(path, &buf[..len]).or_else(fallback)
}

pub fn detect_mime_from_bytes(path: &Path, bytes: &[u8]) -> Option<String> {
    infer::get(bytes)
        .map(|kind| kind.mime_type().to_string())
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(extension_mime)
                .map(str::to_string)
        })
}

pub fn mime_to_category(mime: &str) -> FileCategory {
    if mime == "inode/directory" {
        return FileCategory::Dir;
    }
    if is_document_mime(mime) {
        return FileCategory::Document;
    }
    if mime.starts_with("image/") {
        return FileCategory::Image;
    }
    if mime.starts_with("audio/") {
        return FileCategory::Audio;
    }
    if mime.starts_with("video/") {
        return FileCategory::Video;
    }
    if is_config_mime(mime) {
        return FileCategory::Config;
    }
    if mime == "text/plain" {
        return FileCategory::Document;
    }
    if mime.starts_with("text/") {
        return FileCategory::Code;
    }
    if is_archive_mime(mime) {
        return FileCategory::Archive;
    }
    if is_code_mime(mime) {
        return FileCategory::Code;
    }

    FileCategory::Other
}

pub fn category_from_ext(name: &str) -> FileCategory {
    let name_lower = name.to_ascii_lowercase();
    let ext = name_lower.rsplit_once('.').map(|(_, ext)| ext);

    if matches!(ext, Some("ini" | "conf" | "cfg")) {
        return FileCategory::Config;
    }

    extension_mime(name).map_or(FileCategory::Other, mime_to_category)
}

pub fn extension_mime(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return Some("application/gzip");
    }
    if name.ends_with(".tar.bz2") || name.ends_with(".tbz") || name.ends_with(".tbz2") {
        return Some("application/x-bzip2");
    }
    if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        return Some("application/x-xz");
    }

    let ext = name.rsplit_once('.')?.1;
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
        "psd" => Some("image/vnd.adobe.photoshop"),
        "xcf" => Some("image/x-xcf"),
        "ai" | "eps" => Some("application/postscript"),

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
        "au" => Some("audio/basic"),

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
        "z" => Some("application/gzip"),

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
        "rst" => Some("text/x-rst"),

        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "eot" => Some("application/vnd.ms-fontobject"),

        "json" => Some("application/json"),
        "jsonc" => Some("application/json"),
        "toml" => Some("application/toml"),
        "yaml" | "yml" => Some("application/yaml"),
        "xml" => Some("application/xml"),
        "ini" | "conf" | "cfg" => Some("text/plain"),
        "plist" => Some("application/x-plist"),
        "lock" => Some("application/json"),

        "txt" | "log" => Some("text/plain"),
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
        "r" | "jl" => Some("text/x-r"),
        "scala" | "sc" => Some("text/x-scala"),
        "clj" | "cljs" => Some("text/x-clojure"),
        "ex" | "exs" => Some("text/x-elixir"),
        "erl" | "hrl" => Some("text/x-erlang"),
        "hs" | "lhs" => Some("text/x-haskell"),
        "ml" | "mli" => Some("text/x-ocaml"),
        "nim" => Some("text/x-nim"),
        "zig" => Some("text/x-zig"),
        "dart" => Some("text/x-dart"),
        "ps1" | "bat" | "cmd" => Some("text/x-shellscript"),
        "scss" | "sass" | "less" => Some("text/x-scss"),
        "vue" => Some("text/x-vue"),
        "svelte" => Some("text/x-svelte"),
        _ => None,
    }
}

fn is_config_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/json"
            | "application/toml"
            | "application/yaml"
            | "application/x-yaml"
            | "text/yaml"
            | "application/xml"
            | "text/xml"
    )
}

fn is_archive_mime(mime: &str) -> bool {
    matches!(
        mime,
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
            | "application/vnd.rn-realmedia"
    )
}

fn is_document_mime(mime: &str) -> bool {
    mime == "application/pdf"
        || mime == "application/msword"
        || mime == "application/rtf"
        || mime == "text/markdown"
        || mime == "text/csv"
        || mime == "text/tab-separated-values"
        || mime == "application/epub+zip"
        || mime == "image/vnd.djvu"
        || mime == "application/x-mobipocket-ebook"
        || mime == "application/vnd.amazon.ebook"
        || mime == "application/vnd.ms-htmlhelp"
        || mime == "application/x-tex"
        || mime == "text/x-rst"
        || mime.starts_with("application/vnd.oasis.opendocument.")
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.ms-")
}

fn is_code_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/javascript"
            | "application/typescript"
            | "application/ecmascript"
            | "application/sql"
            | "application/wasm"
            | "application/x-httpd-php"
            | "application/x-sh"
            | "text/x-rust"
            | "text/x-python"
            | "text/x-c"
            | "text/x-c-header"
            | "text/x-c++"
            | "text/x-go"
            | "text/x-java-source"
            | "text/x-kotlin"
            | "text/x-ruby"
            | "text/x-swift"
            | "text/x-csharp"
            | "text/x-lua"
            | "text/x-perl"
            | "text/x-r"
            | "text/x-scala"
            | "text/x-clojure"
            | "text/x-elixir"
            | "text/x-erlang"
            | "text/x-haskell"
            | "text/x-ocaml"
            | "text/x-nim"
            | "text/x-zig"
            | "text/x-dart"
            | "text/x-scss"
            | "text/x-vue"
            | "text/x-svelte"
            | "text/x-jsx"
    )
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
        assert_eq!(category_from_ext("font.ttf"), FileCategory::Other);
        assert_eq!(category_from_ext("font.woff2"), FileCategory::Other);
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
        assert_eq!(mime_to_category("font/ttf"), FileCategory::Other);
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
    }

    #[test]
    fn extension_mime_returns_correct_types() {
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
        assert_eq!(
            extension_mime("test.psd"),
            Some("image/vnd.adobe.photoshop")
        );
        assert_eq!(extension_mime("test.xcf"), Some("image/x-xcf"));
    }

    #[test]
    fn is_archive_mime_covers_new_types() {
        assert!(is_archive_mime("application/x-debian-package"));
        assert!(is_archive_mime("application/x-rpm"));
        assert!(is_archive_mime("application/java-archive"));
        assert!(is_archive_mime("application/x-iso9660-image"));
        assert!(is_archive_mime("application/x-apple-diskimage"));
        assert!(is_archive_mime("application/vnd.android.package-archive"));
    }

    #[test]
    fn is_document_mime_covers_new_types() {
        assert!(is_document_mime("text/csv"));
        assert!(is_document_mime("text/tab-separated-values"));
        assert!(is_document_mime("application/epub+zip"));
        assert!(is_document_mime("image/vnd.djvu"));
    }

    #[test]
    fn is_code_mime_covers_new_types() {
        assert!(is_code_mime("text/x-swift"));
        assert!(is_code_mime("text/x-csharp"));
        assert!(is_code_mime("text/x-lua"));
        assert!(is_code_mime("text/x-haskell"));
        assert!(is_code_mime("text/x-ocaml"));
        assert!(is_code_mime("text/x-nim"));
        assert!(is_code_mime("text/x-zig"));
        assert!(is_code_mime("text/x-vue"));
        assert!(is_code_mime("text/x-svelte"));
    }
}
