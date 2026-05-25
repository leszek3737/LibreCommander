use std::path::Path;

pub(crate) fn is_image_mime(mime: Option<&str>) -> bool {
    mime.is_some_and(|m| m.starts_with("image/"))
}

pub(crate) fn should_open_as_text(path: &Path, mime: Option<&str>, bytes: &[u8]) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if crate::app::file_type::is_source_code(name) || crate::app::file_type::is_config(name) {
        return true;
    }

    if let Some(mime) = mime
        && is_known_binary_mime(mime)
    {
        return false;
    }

    if bytes.contains(&0) {
        return false;
    }

    if let Some(mime) = mime
        && (mime.starts_with("text/") || is_text_application_mime(mime))
    {
        return true;
    }

    true
}

pub(crate) fn is_text_application_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/json"
            | "application/toml"
            | "application/yaml"
            | "application/x-yaml"
            | "application/xml"
            | "application/javascript"
            | "application/typescript"
            | "application/ecmascript"
            | "application/sql"
            | "application/x-httpd-php"
            | "application/x-sh"
            | "application/rtf"
    )
}

pub(crate) fn is_known_binary_mime(mime: &str) -> bool {
    mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
        || mime.starts_with("application/vnd.oasis.opendocument.")
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.ms-")
        || matches!(
            mime,
            "application/octet-stream"
                | "application/zip"
                | "application/x-tar"
                | "application/gzip"
                | "application/x-gzip"
                | "application/x-bzip2"
                | "application/x-xz"
                | "application/x-7z-compressed"
                | "application/vnd.rar"
                | "application/x-rar-compressed"
                | "application/zstd"
                | "application/pdf"
                | "application/msword"
                | "application/epub+zip"
                | "application/wasm"
                | "application/x-mach-binary"
                | "application/x-dosexec"
                | "application/x-executable"
                | "application/x-sharedlib"
                | "application/x-object"
        )
}
