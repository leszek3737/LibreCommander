fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.get(s.len().saturating_sub(suffix.len())..)
        .map_or(false, |tail| tail.eq_ignore_ascii_case(suffix))
}

pub fn is_archive(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".tar.gz")
        || ends_with_ignore_ascii_case(name, ".tar.bz2")
        || ends_with_ignore_ascii_case(name, ".tar.xz")
        || ends_with_ignore_ascii_case(name, ".tar")
        || ends_with_ignore_ascii_case(name, ".gz")
        || ends_with_ignore_ascii_case(name, ".zip")
        || ends_with_ignore_ascii_case(name, ".bz2")
        || ends_with_ignore_ascii_case(name, ".xz")
        || ends_with_ignore_ascii_case(name, ".7z")
        || ends_with_ignore_ascii_case(name, ".rar")
}

pub fn is_image(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".jpg")
        || ends_with_ignore_ascii_case(name, ".jpeg")
        || ends_with_ignore_ascii_case(name, ".png")
        || ends_with_ignore_ascii_case(name, ".gif")
        || ends_with_ignore_ascii_case(name, ".bmp")
        || ends_with_ignore_ascii_case(name, ".svg")
}

pub fn is_source_code(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".rs")
        || ends_with_ignore_ascii_case(name, ".py")
        || ends_with_ignore_ascii_case(name, ".js")
        || ends_with_ignore_ascii_case(name, ".ts")
        || ends_with_ignore_ascii_case(name, ".c")
        || ends_with_ignore_ascii_case(name, ".h")
        || ends_with_ignore_ascii_case(name, ".cpp")
        || ends_with_ignore_ascii_case(name, ".go")
        || ends_with_ignore_ascii_case(name, ".java")
}

pub fn is_document(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".pdf")
        || ends_with_ignore_ascii_case(name, ".doc")
        || ends_with_ignore_ascii_case(name, ".docx")
        || ends_with_ignore_ascii_case(name, ".xls")
        || ends_with_ignore_ascii_case(name, ".xlsx")
        || ends_with_ignore_ascii_case(name, ".odt")
}

pub fn is_audio(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".mp3")
        || ends_with_ignore_ascii_case(name, ".wav")
        || ends_with_ignore_ascii_case(name, ".flac")
        || ends_with_ignore_ascii_case(name, ".ogg")
        || ends_with_ignore_ascii_case(name, ".m4a")
}

pub fn is_video(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".mp4")
        || ends_with_ignore_ascii_case(name, ".avi")
        || ends_with_ignore_ascii_case(name, ".mkv")
        || ends_with_ignore_ascii_case(name, ".mov")
        || ends_with_ignore_ascii_case(name, ".webm")
}

pub fn is_config(name: &str) -> bool {
    ends_with_ignore_ascii_case(name, ".json")
        || ends_with_ignore_ascii_case(name, ".toml")
        || ends_with_ignore_ascii_case(name, ".yaml")
        || ends_with_ignore_ascii_case(name, ".yml")
        || ends_with_ignore_ascii_case(name, ".ini")
        || ends_with_ignore_ascii_case(name, ".conf")
        || ends_with_ignore_ascii_case(name, ".cfg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_archive_tar() {
        assert!(is_archive("file.tar"));
        assert!(is_archive("archive.TAR"));
        assert!(is_archive("backup.tar.gz"));
    }

    #[test]
    fn test_is_archive_zip() {
        assert!(is_archive("files.zip"));
        assert!(is_archive("data.7z"));
        assert!(is_archive("backup.rar"));
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
    }

    #[test]
    fn test_is_source_code_negative() {
        assert!(!is_source_code("image.png"));
        assert!(!is_source_code("data.json"));
    }
}
