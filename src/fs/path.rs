use std::path::{Component, Path, PathBuf};

pub fn clean_path(path: &Path) -> PathBuf {
    let mut comps: Vec<Component<'_>> = Vec::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match comps.last() {
                Some(Component::RootDir | Component::Prefix(_)) => {}
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

pub fn expand_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }

    if trimmed == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }

    if let Some(rest) = stripped_tilde(trimmed) {
        if let Some(home) = dirs::home_dir() {
            let expanded_rest = expand_env_vars(rest);
            return home.join(expanded_rest);
        }
        return PathBuf::from(trimmed);
    }

    let expanded = expand_env_vars(trimmed);
    PathBuf::from(expanded)
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

pub fn resolve_user_path(base: &Path, input: &str) -> PathBuf {
    let expanded = expand_path(input);
    if expanded.is_absolute() {
        clean_path(&expanded)
    } else {
        clean_path(&base.join(expanded))
    }
}

fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '$' {
            if let Some((consumed, replacement)) = expand_brace_var(&chars, i) {
                result.push_str(&replacement);
                i += consumed;
                continue;
            }
            if let Some((consumed, replacement)) = expand_dollar_var(&chars, i) {
                result.push_str(&replacement);
                i += consumed;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn expand_brace_var(chars: &[char], i: usize) -> Option<(usize, String)> {
    if i + 1 >= chars.len() || chars[i + 1] != '{' {
        return None;
    }
    let end = find_brace_close(chars, i + 2)?;
    if end > i + 2 {
        let var_name: String = chars[i + 2..end].iter().collect();
        if let Some(val) = env_var(&var_name) {
            return Some((end - i + 1, val));
        }
        let literal: String = chars[i..=end].iter().collect();
        Some((end - i + 1, literal))
    } else {
        Some((end - i + 1, "${}".to_string()))
    }
}

fn expand_dollar_var(chars: &[char], i: usize) -> Option<(usize, String)> {
    if i + 1 >= chars.len() || !is_env_name_start(chars[i + 1]) {
        return None;
    }
    let start = i + 1;
    let mut end = start;
    while end < chars.len() && is_env_name_char(chars[end]) {
        end += 1;
    }
    let var_name: String = chars[start..end].iter().collect();
    if let Some(val) = env_var(&var_name) {
        Some((end - i, val))
    } else {
        let literal: String = chars[i..end].iter().collect();
        Some((end - i, literal))
    }
}

fn find_brace_close(chars: &[char], from: usize) -> Option<usize> {
    let mut depth = 1;
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
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
    std::env::var_os(name).and_then(|v| v.into_string().ok())
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
}
