use std::borrow::Cow;
use std::path::{MAIN_SEPARATOR, Path};

/// Width-aware truncation of `s` to at most `max_width` display columns
/// (per `unicode-width`).
///
/// If `s` already fits, it is returned unchanged. Otherwise the result is
/// shortened in one of two asymmetric ways, depending on whether an ellipsis
/// fits:
///
/// - `max_width > 3`: keep a **trailing suffix** and prepend `"..."`, so the
///   most significant tail (e.g. a file name) stays visible. The kept suffix
///   has width `max_width - 3`.
/// - `max_width <= 3`: there is no room for an ellipsis, so keep a leading
///   **prefix** that fits in `max_width` columns (no ellipsis at all).
///
/// So despite the name, the `<= 3` branch keeps a prefix, not a suffix; the
/// name reflects the common (ellipsis) case. The returned string never exceeds
/// `max_width` columns, and zero-width chars (e.g. combining marks) stay
/// attached to their base char. Truncation is char- and width-based, not
/// grapheme-cluster-aware; at the suffix boundary a zero-width combining mark
/// may be retained without its preceding base character.
fn truncate_suffix<'a>(s: &'a str, max_width: usize) -> Cow<'a, str> {
    if max_width == 0 {
        return Cow::Owned(String::new());
    }
    if max_width > 3 {
        // Ellipsis case. Single reverse pass: locate the widest trailing
        // suffix that fits in `max_width - 3` while summing the total width,
        // so we can also detect the no-truncation case without a second scan.
        let suffix_budget = max_width - 3;
        let mut total_width = 0;
        let mut tail_width = 0;
        let mut split_idx = s.len();
        // Once a char no longer fits, the (contiguous) suffix is fixed; keep
        // scanning only to finish summing the total width.
        let mut suffix_done = false;
        for (idx, ch) in s.char_indices().rev() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            total_width += cw;
            if !suffix_done {
                if tail_width + cw <= suffix_budget {
                    tail_width += cw;
                    split_idx = idx;
                } else {
                    suffix_done = true;
                }
            }
        }
        if total_width <= max_width {
            return Cow::Borrowed(s);
        }
        let tail = &s[split_idx..];
        let mut out = String::with_capacity(3 + tail.len());
        out.push_str("...");
        out.push_str(tail);
        Cow::Owned(out)
    } else {
        // No room for an ellipsis. Single forward pass: keep the widest leading
        // prefix that fits in `max_width`, returning the input untouched if it
        // turns out to fit in full.
        let mut width = 0;
        for (idx, ch) in s.char_indices() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + cw > max_width {
                return Cow::Owned(s[..idx].to_owned());
            }
            width += cw;
        }
        Cow::Borrowed(s)
    }
}

pub(super) fn truncate_path<'a>(path: &'a str, max_width: usize) -> Cow<'a, str> {
    let total_width = unicode_width::UnicodeWidthStr::width(path);
    if total_width <= max_width {
        return Cow::Borrowed(path);
    }
    let p = Path::new(path);
    // When the path has no final component (e.g. it ends in `..`, `.`, or a
    // separator that resolves to nothing) `file_name()` is `None`; the same
    // holds for the (extremely rare) non-UTF8 file name. In either case there
    // is no file name to anchor on, so fall back to plain width-aware
    // truncation of the whole path rather than dropping the last component.
    let Some(file_ref) = p.file_name().and_then(|f| f.to_str()) else {
        return truncate_suffix(path, max_width);
    };
    let file_width = unicode_width::UnicodeWidthStr::width(file_ref);
    if file_width >= max_width {
        return truncate_suffix(file_ref, max_width);
    }
    let dir_ref: &str = p.parent().and_then(|d| d.to_str()).unwrap_or("");
    if dir_ref.is_empty() {
        return truncate_suffix(path, max_width);
    }
    let budget = max_width - file_width - 1;
    let dir_cow = truncate_suffix(dir_ref, budget);
    // At a tiny budget the dir part may be a width-limited prefix that ends in
    // a separator; strip it so re-joining with the file name does not produce
    // a doubled separator (e.g. "ab//file.txt").
    let dir_part: &str = dir_cow.strip_suffix(MAIN_SEPARATOR).unwrap_or(&dir_cow);
    let sep_len = MAIN_SEPARATOR.len_utf8();
    let mut result = String::with_capacity(dir_part.len() + sep_len + file_ref.len());
    result.push_str(dir_part);
    result.push(MAIN_SEPARATOR);
    result.push_str(file_ref);
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
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + cw > w {
            lines += 1;
            col = cw;
        } else {
            col += cw;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{truncate_path, truncate_suffix};

    // --- truncate_path ---

    #[test]
    fn truncate_path_empty_path() {
        assert_eq!(truncate_path("", 10), "");
    }

    #[cfg(unix)]
    #[test]
    fn truncate_path_normal_case() {
        assert_eq!(truncate_path("/home/user/notes.txt", 14), "...r/notes.txt");
    }

    #[cfg(unix)]
    #[test]
    fn truncate_path_trailing_slash_keeps_last_component() {
        assert_eq!(truncate_path("/home/user/docs/", 10), "...er/docs");
    }

    #[cfg(unix)]
    #[test]
    fn truncate_path_ending_in_parent_dir_keeps_tail() {
        // `file_name()` is None for a path ending in "..", so we must fall back
        // to whole-path truncation rather than dropping that last component.
        assert_eq!(truncate_path("/home/user/..", 8), "...er/..");
    }

    #[cfg(unix)]
    #[test]
    fn truncate_path_tiny_budget_no_double_separator() {
        // The dir prefix "ab/" must not be re-joined into "ab//x".
        assert_eq!(truncate_path("ab/cd/x", 5), "ab/x");
    }

    #[cfg(unix)]
    #[test]
    fn truncate_path_wide_cjk_components() {
        assert_eq!(truncate_path("/项目/数据/报告.txt", 12), "/项/报告.txt");
    }

    #[cfg(unix)]
    #[test]
    fn truncate_path_emoji_components() {
        assert_eq!(truncate_path("/tmp/🚀🚀🚀/log", 10), "...🚀/log");
    }

    // --- truncate_suffix ---

    #[test]
    fn truncate_suffix_fits_unchanged() {
        assert_eq!(truncate_suffix("abc", 5), "abc");
    }

    #[test]
    fn truncate_suffix_zero_width_is_empty() {
        assert_eq!(truncate_suffix("abc", 0), "");
    }

    #[test]
    fn truncate_suffix_ellipsis_branch() {
        // max_width > 3: keep a trailing suffix behind an ellipsis.
        assert_eq!(truncate_suffix("abcdefgh", 5), "...gh");
    }

    #[test]
    fn truncate_suffix_prefix_branch() {
        // max_width <= 3: no room for an ellipsis, keep a leading prefix.
        assert_eq!(truncate_suffix("abcdef", 3), "abc");
    }

    #[test]
    fn truncate_suffix_wide_chars_in_suffix() {
        assert_eq!(truncate_suffix("数据库系统", 6), "...统");
    }

    #[test]
    fn truncate_suffix_combining_mark_in_suffix() {
        // A combining mark (width 0) stays attached to its base char.
        assert_eq!(truncate_suffix("aaaaae\u{0301}", 5), "...ae\u{0301}");
    }

    #[test]
    fn truncate_suffix_combining_mark_in_prefix() {
        assert_eq!(truncate_suffix("e\u{0301}fgh", 2), "e\u{0301}f");
    }
}
