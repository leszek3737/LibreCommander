use std::borrow::Cow;
use std::path::Path;

use crate::app::mime::{KNOWN_BINARY_MIMES, KNOWN_BINARY_PREFIXES, TEXT_APPLICATION_MIMES};

const NUL_BYTE_SCAN_LIMIT: usize = 8192;

/// Returns `true` when `mime` starts with `"image/"`.
pub(crate) fn is_image_mime(mime: Option<&str>) -> bool {
    mime.is_some_and(|m| m.starts_with("image/"))
}

/// Decides whether a file should be opened in text mode.
///
/// Checks are performed in order of specificity:
/// 1. Known source-code / config extensions → **text** (even with binary MIME
///    or NUL bytes).
/// 2. Known binary MIME → **binary**.
/// 3. Presence of NUL bytes in the first [`NUL_BYTE_SCAN_LIMIT`] → **binary**.
/// 4. MIME that explicitly signals text (`text/…` or an entry in
///    [`TEXT_APPLICATION_MIMES`]) → **text**.
/// 5. Fallback: assume **text**.
pub(crate) fn should_open_as_text(path: &Path, mime: Option<&str>, bytes: &[u8]) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or(Cow::Borrowed(""));

    if crate::app::file_type::is_source_code(&name) || crate::app::file_type::is_config(&name) {
        return true;
    }

    if let Some(mime) = mime
        && is_known_binary_mime(mime)
    {
        return false;
    }

    let scan_limit = bytes.len().min(NUL_BYTE_SCAN_LIMIT);
    if bytes[..scan_limit].contains(&0) {
        return false;
    }

    if let Some(mime) = mime
        && (mime.starts_with("text/") || is_text_application_mime(mime))
    {
        return true;
    }

    true
}

/// Returns `true` for `application/*` MIME types that carry human-readable
/// content (JSON, TOML, XML, source code, etc.).
pub(crate) fn is_text_application_mime(mime: &str) -> bool {
    TEXT_APPLICATION_MIMES.contains(&mime)
}

/// Returns `true` when a MIME type is unambiguously binary — images, audio,
/// video, archives, documents, executables, etc.
pub(crate) fn is_known_binary_mime(mime: &str) -> bool {
    KNOWN_BINARY_PREFIXES
        .iter()
        .any(|prefix| mime.starts_with(prefix))
        || KNOWN_BINARY_MIMES.contains(&mime)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_mime_some() {
        assert!(is_image_mime(Some("image/png")));
        assert!(is_image_mime(Some("image/jpeg")));
        assert!(is_image_mime(Some("image/svg+xml")));
    }

    #[test]
    fn test_is_image_mime_none_or_other() {
        assert!(!is_image_mime(None));
        assert!(!is_image_mime(Some("text/plain")));
        assert!(!is_image_mime(Some("application/pdf")));
    }

    #[test]
    fn test_is_text_application_mime_known() {
        assert!(is_text_application_mime("application/json"));
        assert!(is_text_application_mime("application/toml"));
        assert!(is_text_application_mime("application/yaml"));
        assert!(is_text_application_mime("application/x-yaml"));
        assert!(is_text_application_mime("application/xml"));
        assert!(is_text_application_mime("application/javascript"));
        assert!(is_text_application_mime("application/sql"));
        assert!(is_text_application_mime("application/x-sh"));
    }

    #[test]
    fn test_is_text_application_mime_unknown() {
        assert!(!is_text_application_mime("application/zip"));
        assert!(!is_text_application_mime("application/pdf"));
        assert!(!is_text_application_mime("image/png"));
        assert!(!is_text_application_mime("text/plain"));
    }

    #[test]
    fn test_is_known_binary_mime_by_prefix() {
        assert!(is_known_binary_mime("image/png"));
        assert!(is_known_binary_mime("audio/mpeg"));
        assert!(is_known_binary_mime("video/mp4"));
        assert!(is_known_binary_mime(
            "application/vnd.oasis.opendocument.text"
        ));
        assert!(is_known_binary_mime(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        ));
        assert!(is_known_binary_mime("application/vnd.ms-excel"));
    }

    #[test]
    fn test_is_known_binary_mime_exact() {
        assert!(is_known_binary_mime("application/octet-stream"));
        assert!(is_known_binary_mime("application/zip"));
        assert!(is_known_binary_mime("application/pdf"));
        assert!(is_known_binary_mime("application/wasm"));
        assert!(is_known_binary_mime("application/epub+zip"));
    }

    #[test]
    fn test_is_known_binary_mime_text_mimes_are_not_binary() {
        assert!(!is_known_binary_mime("text/plain"));
        assert!(!is_known_binary_mime("application/json"));
        assert!(!is_known_binary_mime("application/toml"));
        assert!(!is_known_binary_mime("application/xml"));
    }

    #[test]
    fn test_should_open_as_text_existing_tests_still_pass() {
        assert!(should_open_as_text(
            Path::new("README"),
            Some("text/plain"),
            b"hello"
        ));
        assert!(should_open_as_text(
            Path::new("main.rs"),
            Some("application/octet-stream"),
            b"fn main() {}"
        ));
        assert!(should_open_as_text(
            Path::new("config.toml"),
            Some("application/octet-stream"),
            b"key = \"value\""
        ));
        assert!(!should_open_as_text(
            Path::new("archive.zip"),
            Some("application/zip"),
            b"PK\0\0"
        ));
        assert!(!should_open_as_text(
            Path::new("image.png"),
            Some("image/png"),
            b"\x89PNG\r\n"
        ));
        assert!(!should_open_as_text(
            Path::new("unknown.bin"),
            None,
            b"abc\0def"
        ));
    }

    #[test]
    fn test_should_open_as_text_nul_byte_scan_limit() {
        let mut data = vec![b'a'; NUL_BYTE_SCAN_LIMIT + 16];
        data[NUL_BYTE_SCAN_LIMIT + 4] = 0;
        assert!(should_open_as_text(Path::new("data.txt"), None, &data));
    }

    #[test]
    #[cfg(unix)]
    fn test_should_open_as_text_non_utf8_filename_with_known_extension() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"\xff.rs"));
        assert!(should_open_as_text(
            path,
            Some("application/octet-stream"),
            b"fn"
        ));
    }

    #[test]
    fn test_should_open_as_text_nul_byte_within_limit() {
        let mut data = vec![b'a'; NUL_BYTE_SCAN_LIMIT];
        data[42] = 0;
        assert!(!should_open_as_text(Path::new("data.bin"), None, &data));
    }
}
