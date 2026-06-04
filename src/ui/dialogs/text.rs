use std::borrow::Cow;
use std::path::{MAIN_SEPARATOR, Path};

fn truncate_suffix<'a>(s: &'a str, max_width: usize) -> Cow<'a, str> {
    if max_width > 3 {
        let suffix_budget = max_width - 3;
        let total_width: usize = s
            .chars()
            .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum();
        if total_width <= max_width {
            return Cow::Borrowed(s);
        }
        let mut remaining = total_width;
        let mut split_idx = s.len();
        for (idx, ch) in s.char_indices() {
            if remaining <= suffix_budget {
                split_idx = idx;
                break;
            }
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            remaining -= cw;
        }
        Cow::Owned(format!("...{}", &s[split_idx..]))
    } else {
        if max_width == 0 {
            return Cow::Owned(String::new());
        }
        let total_width: usize = s
            .chars()
            .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum();
        if total_width <= max_width {
            return Cow::Borrowed(s);
        }
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
        Cow::Owned(out)
    }
}

pub(super) fn truncate_path<'a>(path: &'a str, max_width: usize) -> Cow<'a, str> {
    let total_width = unicode_width::UnicodeWidthStr::width(path);
    if total_width <= max_width {
        return Cow::Borrowed(path);
    }
    let p = Path::new(path);
    let file_ref: &str = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
    let file_width = unicode_width::UnicodeWidthStr::width(file_ref);
    if file_width >= max_width {
        return truncate_suffix(file_ref, max_width);
    }
    let dir_ref: &str = p.parent().and_then(|d| d.to_str()).unwrap_or("");
    if dir_ref.is_empty() {
        return truncate_suffix(path, max_width);
    }
    let budget = max_width - file_width - 1;
    let dir_part = truncate_suffix(dir_ref, budget);
    let result = format!("{dir_part}{MAIN_SEPARATOR}{file_ref}");
    Cow::Owned(result)
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
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if col + cw > w {
            lines += 1;
            col = cw;
        } else {
            col += cw;
        }
    }
    lines
}
