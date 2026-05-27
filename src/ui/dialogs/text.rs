use std::path::{MAIN_SEPARATOR, Path};

fn truncate_suffix(s: &str, max_width: usize) -> String {
    if max_width > 3 {
        let suffix_budget = max_width - 3;
        let mut width = 0;
        let mut split_idx = s.len();
        for (idx, ch) in s.char_indices().rev() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + cw > suffix_budget {
                split_idx = idx + ch.len_utf8();
                break;
            }
            width += cw;
            split_idx = idx;
        }
        format!("...{}", &s[split_idx..])
    } else {
        let mut out = String::new();
        let mut width = 0;
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + cw > max_width {
                break;
            }
            width += cw;
            out.push(ch);
        }
        out
    }
}

pub(super) fn truncate_path(path: &str, max_width: usize) -> String {
    let total_width = unicode_width::UnicodeWidthStr::width(path);
    if total_width <= max_width {
        return path.to_string();
    }
    let p = Path::new(path);
    let file = p
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_width = unicode_width::UnicodeWidthStr::width(file.as_str());
    if file_width >= max_width {
        return truncate_suffix(file.as_str(), max_width);
    }
    if dir.is_empty() {
        return truncate_suffix(path, max_width);
    }
    let budget = max_width - file_width - 1;
    let dir_part = truncate_suffix(dir.as_str(), budget);
    format!("{dir_part}{MAIN_SEPARATOR}{file}")
}

pub fn wrapped_line_count(text: &str, available_width: u16) -> usize {
    if text.is_empty() || available_width == 0 {
        return 0;
    }
    let w = usize::from(available_width);
    let mut lines = 1;
    let mut col = 0;
    for ch in text.chars() {
        if ch == '\n' {
            lines += 1;
            col = 0;
            continue;
        }
        let cw = unicode_width::UnicodeWidthChar::width(ch)
            .unwrap_or(1)
            .max(1);
        if col + cw > w {
            lines += 1;
            col = cw;
        } else {
            col += cw;
        }
    }
    lines
}
