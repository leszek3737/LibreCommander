use std::path::{Component, Path, PathBuf};

const ENV_VAR_EXPANSION_FACTOR: usize = 2;

/// Normalizes a path by resolving `.` and `..` components.
///
/// Removes redundant current-directory markers and collapses parent-directory
/// references where possible. The result never contains `./` sequences and
/// minimizes `../` segments. Parent-directory references beyond the root are
/// preserved on Unix; on Windows, `ParentDir` after a drive prefix is kept
/// (drive-relative navigation).
pub fn clean_path(path: &Path) -> PathBuf {
    let est_comps = path.as_os_str().len() / 4 + 1;
    let mut comps: Vec<Component<'_>> = Vec::with_capacity(est_comps.clamp(8, 32));

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match comps.last() {
                // RootDir: cannot ascend past root — drop the ParentDir.
                Some(Component::RootDir) => {}
                // Prefix (Windows drive letter): ParentDir after prefix is
                // drive-relative navigation (e.g. `C:..\foo`) and must be
                // preserved, not silently dropped.
                #[cfg(windows)]
                Some(Component::Prefix(_)) => {
                    comps.push(component);
                }
                Some(Component::Normal(_)) => {
                    comps.pop();
                }
                _ => {
                    comps.push(component);
                }
            },
            _ => {
                comps.push(component);
            }
        }
    }

    if comps.is_empty() {
        return PathBuf::from(".");
    }

    let mut out = PathBuf::new();
    for comp in comps {
        out.push(comp);
    }
    out
}

/// Expands a user-supplied path string.
///
/// Performs tilde expansion (`~` → home directory, `~/foo` → home/foo),
/// environment variable substitution (`$VAR`, `${VAR}`), and strips
/// surrounding whitespace. Returns a `PathBuf`; an empty or
/// whitespace-only input yields an empty path.
pub fn expand_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }

    if trimmed == "~" {
        return dirs::home_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
    }

    if let Some(rest) = stripped_tilde(trimmed)
        && let Some(home) = dirs::home_dir()
    {
        let expanded_rest = expand_env_vars(rest);
        let raw = home.join(expanded_rest.trim_start_matches('/'));
        return clean_path(&raw);
    }

    let expanded = expand_env_vars(trimmed);
    clean_path(&PathBuf::from(expanded))
}

fn stripped_tilde(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix("~/") {
        return Some(rest);
    }
    #[cfg(windows)]
    if let Some(rest) = s.strip_prefix("~\\") {
        return Some(rest);
    }
    None
}

/// Resolves a user-supplied path against a base directory.
///
/// If `input` is absolute (root or drive-letter), it is used directly;
/// otherwise it is joined with `base`. Both branches are normalized
/// via [`clean_path`].
pub fn resolve_user_path(base: &Path, input: &str) -> PathBuf {
    let expanded = expand_path(input);
    if expanded.is_absolute() {
        clean_path(&expanded)
    } else {
        clean_path(&base.join(expanded))
    }
}

/// Replaces `$VAR` and `${VAR}` tokens with their environment values.
///
/// Unknown variables are left unchanged (e.g. `$FOO` stays `$FOO`).
/// Dollar signs not followed by a valid variable name are passed through
/// as literal `$`. Returns the expanded string; the caller is responsible
/// for converting to a path.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len().saturating_mul(ENV_VAR_EXPANSION_FACTOR));
    let mut rest = input;

    while let Some(pos) = rest.find('$') {
        result.push_str(&rest[..pos]);
        // Safe: '$' is ASCII (1 byte), pos + 1 is a valid char boundary.
        let after = &rest[pos + 1..];

        if let Some((consumed, replacement)) = expand_brace_var(after) {
            result.push_str(&replacement);
            rest = &after[consumed..];
        } else if let Some((consumed, replacement)) = expand_dollar_var(after) {
            result.push_str(&replacement);
            rest = &after[consumed..];
        } else {
            result.push('$');
            rest = after;
        }
    }

    result.push_str(rest);
    result
}

fn expand_brace_var(after_dollar: &str) -> Option<(usize, String)> {
    if !after_dollar.starts_with('{') {
        return None;
    }
    let inner = &after_dollar[1..];
    let close = find_brace_close(inner)?;
    let total = 1 + close + 1;
    let var_name = &inner[..close];
    let Some(first_char) = var_name.chars().next() else {
        return Some((total, "${}".to_string()));
    };
    if !is_env_name_start(first_char) {
        return Some((total, format!("${{{var_name}}}")));
    }
    if let Some(val) = env_var(var_name) {
        return Some((total, val));
    }
    Some((total, format!("${{{var_name}}}")))
}

fn expand_dollar_var(after_dollar: &str) -> Option<(usize, String)> {
    let first = after_dollar.chars().next()?;
    if !is_env_name_start(first) {
        return None;
    }
    let var_end = after_dollar
        .char_indices()
        .take_while(|&(_, c)| is_env_name_char(c))
        .last()
        .map_or(0, |(i, c)| i + c.len_utf8());
    let var_name = &after_dollar[..var_end];
    if let Some(val) = env_var(var_name) {
        Some((var_end, val))
    } else {
        Some((var_end, format!("${var_name}")))
    }
}

fn find_brace_close(s: &str) -> Option<usize> {
    let mut depth = 1u32;
    let mut byte_pos = 0;
    for c in s.chars() {
        let c_len = c.len_utf8();
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(byte_pos);
                }
            }
            _ => {}
        }
        byte_pos += c_len;
    }
    None
}

fn is_env_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_env_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn env_var(name: &str) -> Option<String> {
    let val = std::env::var_os(name)?;
    match val.to_str() {
        Some(s) => Some(s.to_string()),
        None => {
            crate::debug_log!("env var '{name}' has non-UTF-8 value, skipping");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_curdir() {
        assert_eq!(clean_path(Path::new("a/./b")), PathBuf::from("a/b"));
    }

    #[test]
    fn clean_parentdir() {
        assert_eq!(clean_path(Path::new("a/b/../c")), PathBuf::from("a/c"));
    }

    #[test]
    #[cfg(unix)]
    fn clean_parentdir_at_root() {
        assert_eq!(clean_path(Path::new("/a/../../b")), PathBuf::from("/b"));
    }

    #[test]
    fn clean_leading_parentdir() {
        assert_eq!(clean_path(Path::new("../../a")), PathBuf::from("../../a"));
    }

    #[test]
    fn clean_empty_result() {
        assert_eq!(clean_path(Path::new("./")), PathBuf::from("."));
    }

    #[test]
    #[cfg(unix)]
    fn clean_root_stays() {
        assert_eq!(clean_path(Path::new("/")), PathBuf::from("/"));
    }

    #[test]
    fn clean_multiple_curdir() {
        assert_eq!(clean_path(Path::new("a/./b/./c")), PathBuf::from("a/b/c"));
    }

    #[test]
    fn clean_complex() {
        assert_eq!(clean_path(Path::new("a/b/c/../../d")), PathBuf::from("a/d"));
    }

    #[test]
    fn clean_parentdir_past_prefix() {
        let input = Path::new("../a");
        assert_eq!(clean_path(input), PathBuf::from("../a"));
    }

    #[test]
    #[cfg(windows)]
    fn clean_parentdir_after_prefix_preserved() {
        assert_eq!(
            clean_path(Path::new(r"C:..\foo")),
            PathBuf::from(r"C:..\foo")
        );
        assert_eq!(
            clean_path(Path::new(r"C:..\..\foo")),
            PathBuf::from(r"C:..\..\foo")
        );
    }

    #[test]
    fn expand_tilde_only() {
        let result = expand_path("~");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(result, home);
        }
    }

    #[test]
    fn expand_tilde_with_subpath() {
        let result = expand_path("~/Documents");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(result, home.join("Documents"));
        }
    }

    #[test]
    fn expand_no_tilde() {
        assert_eq!(expand_path("/tmp"), PathBuf::from("/tmp"));
    }

    #[test]
    fn expand_empty() {
        assert_eq!(expand_path(""), PathBuf::new());
    }

    #[test]
    fn expand_whitespace_only() {
        assert_eq!(expand_path("   "), PathBuf::new());
    }

    #[test]
    fn expand_env_dollar_brace() {
        let expanded = expand_path("${HOME}/test");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expanded, PathBuf::from(format!("{home}/test")));
        }
    }

    #[test]
    fn expand_unknown_var_stays() {
        assert_eq!(
            expand_path("$LC_NONEXISTENT_VAR_XYZ/path"),
            PathBuf::from("$LC_NONEXISTENT_VAR_XYZ/path")
        );
    }

    #[test]
    fn expand_brace_unknown_stays() {
        assert_eq!(
            expand_path("${LC_NONEXISTENT_VAR_XYZ}/path"),
            PathBuf::from("${LC_NONEXISTENT_VAR_XYZ}/path")
        );
    }

    #[test]
    fn resolve_absolute() {
        assert_eq!(
            resolve_user_path(Path::new("/base/dir"), "/tmp/a"),
            PathBuf::from("/tmp/a")
        );
    }

    #[test]
    #[cfg(unix)]
    fn resolve_relative() {
        assert_eq!(
            resolve_user_path(Path::new("/base/dir"), "../x"),
            PathBuf::from("/base/x")
        );
    }

    #[test]
    fn resolve_with_dot() {
        assert_eq!(
            resolve_user_path(Path::new("/base/dir"), "./sub"),
            PathBuf::from("/base/dir/sub")
        );
    }

    #[test]
    #[cfg(unix)]
    fn resolve_tilde() {
        let result = resolve_user_path(Path::new("/base/dir"), "~/test");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(result, home.join("test"));
        }
    }

    #[test]
    fn env_var_dollar_simple() {
        let result = expand_path("$HOME/docs");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(result, PathBuf::from(format!("{home}/docs")));
        }
    }

    #[test]
    fn env_var_brace() {
        let result = expand_path("${HOME}/docs");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(result, PathBuf::from(format!("{home}/docs")));
        }
    }

    #[test]
    fn env_var_unknown_stays() {
        let var_name = format!("LC_TEST_UNSET_{}", std::process::id());
        let input = format!("${var_name}/path");
        assert_eq!(expand_path(&input), PathBuf::from(&input));
    }

    #[test]
    fn env_var_brace_unknown_stays() {
        let var_name = format!("LC_TEST_UNSET_{}", std::process::id());
        let input = format!("${{{var_name}}}/path");
        assert_eq!(expand_path(&input), PathBuf::from(&input));
    }

    #[test]
    fn env_var_empty_brace() {
        assert_eq!(expand_path("${}/path"), PathBuf::from("${}/path"));
    }

    #[test]
    fn env_var_unclosed_brace() {
        assert_eq!(expand_path("${HOME/path"), PathBuf::from("${HOME/path"));
    }

    #[test]
    fn env_var_multiple() {
        let result = expand_path("$HOME/a/$HOME/b");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(result, PathBuf::from(format!("{home}/a/{home}/b")));
        }
    }

    #[test]
    fn env_var_dollar_not_var() {
        assert_eq!(expand_path("cost$5"), PathBuf::from("cost$5"));
    }

    #[test]
    fn env_var_dollar_at_end() {
        assert_eq!(expand_path("end$"), PathBuf::from("end$"));
    }

    #[test]
    fn find_brace_close_simple() {
        assert_eq!(find_brace_close("FOO}rest"), Some(3));
    }

    #[test]
    fn find_brace_close_empty_var() {
        assert_eq!(find_brace_close("}rest"), Some(0));
    }

    #[test]
    fn find_brace_close_nested() {
        assert_eq!(find_brace_close("FOO${BAR}}rest"), Some(9));
    }

    #[test]
    fn find_brace_close_balanced_inner() {
        assert_eq!(find_brace_close("a{b}c}rest"), Some(5));
    }

    #[test]
    fn find_brace_close_no_close() {
        assert_eq!(find_brace_close("FOO${BAR"), None);
    }

    #[test]
    fn find_brace_close_unbalanced_inner() {
        assert_eq!(find_brace_close("a{b{c}rest"), None);
    }

    #[test]
    fn expand_nested_braces_unknown() {
        assert_eq!(
            expand_path("${FOO${BAR}}/path"),
            PathBuf::from("${FOO${BAR}}/path")
        );
    }

    #[test]
    fn expand_brace_unicode_varname() {
        let var_name = format!("LC_TEST_UNI_{}", std::process::id());
        let input = format!("${{{var_name}}}/日本語");
        assert_eq!(expand_path(&input), PathBuf::from(&input));
    }

    #[test]
    fn expand_dollar_unicode_after() {
        let result = expand_env_vars("$HOME/日本語");
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(result, format!("{home}/日本語"));
        }
    }
}
