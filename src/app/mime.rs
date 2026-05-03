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

    infer::get(&buf[..len])
        .map(|kind| kind.mime_type().to_string())
        .or_else(fallback)
}

pub fn mime_to_category(mime: &str) -> FileCategory {
    if mime == "inode/directory" {
        return FileCategory::Dir;
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
    if mime == "text/plain" {
        return FileCategory::Document;
    }
    if mime.starts_with("text/") {
        return FileCategory::Code;
    }
    if is_config_mime(mime) {
        return FileCategory::Config;
    }
    if is_archive_mime(mime) {
        return FileCategory::Archive;
    }
    if is_document_mime(mime) {
        return FileCategory::Document;
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

        "mp4" => Some("video/mp4"),
        "m4v" => Some("video/x-m4v"),
        "mkv" => Some("video/x-matroska"),
        "mov" => Some("video/quicktime"),
        "avi" => Some("video/x-msvideo"),
        "webm" => Some("video/webm"),
        "mpeg" | "mpg" => Some("video/mpeg"),

        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "flac" => Some("audio/flac"),
        "ogg" | "oga" => Some("audio/ogg"),
        "m4a" => Some("audio/mp4"),
        "aac" => Some("audio/aac"),
        "opus" => Some("audio/opus"),

        "zip" => Some("application/zip"),
        "tar" => Some("application/x-tar"),
        "gz" => Some("application/gzip"),
        "bz2" => Some("application/x-bzip2"),
        "xz" => Some("application/x-xz"),
        "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/vnd.rar"),
        "zst" => Some("application/zstd"),

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

        "json" => Some("application/json"),
        "toml" => Some("application/toml"),
        "yaml" | "yml" => Some("application/yaml"),
        "xml" => Some("application/xml"),
        "ini" | "conf" | "cfg" => Some("text/plain"),

        "txt" | "log" => Some("text/plain"),
        "md" | "markdown" => Some("text/markdown"),
        "rs" => Some("text/x-rust"),
        "py" => Some("text/x-python"),
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
    )
}

fn is_document_mime(mime: &str) -> bool {
    mime == "application/pdf"
        || mime == "application/msword"
        || mime == "application/rtf"
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
    fn mime_to_category_maps_primary_types() {
        assert_eq!(mime_to_category("inode/directory"), FileCategory::Dir);
        assert_eq!(mime_to_category("image/png"), FileCategory::Image);
        assert_eq!(mime_to_category("audio/mpeg"), FileCategory::Audio);
        assert_eq!(mime_to_category("video/mp4"), FileCategory::Video);
        assert_eq!(mime_to_category("text/x-rust"), FileCategory::Code);
        assert_eq!(mime_to_category("text/plain"), FileCategory::Document);
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
    }
}
