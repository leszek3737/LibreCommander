use std::path::{Path, PathBuf};

use regex::Regex;

use crate::app::paths;

/// A single entry parsed from a user menu file.
#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub hotkey: char,
    pub title: String,
    pub command: String,
    pub condition: Option<Regex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuWarning {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ParsedMenu {
    pub entries: Vec<MenuEntry>,
    pub warnings: Vec<MenuWarning>,
}

impl PartialEq for MenuEntry {
    fn eq(&self, other: &Self) -> bool {
        self.hotkey == other.hotkey
            && self.title == other.title
            && self.command == other.command
            && match (&self.condition, &other.condition) {
                (None, None) => true,
                (Some(a), Some(b)) => a.as_str() == b.as_str(),
                _ => false,
            }
    }
}

/// Shell-escape a single string value using single-quote wrapping.
/// Internal single quotes become `'\''`.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Context supplied by the caller for `%`-substitutions.
pub struct SubstContext<'a> {
    /// Name of the file under the cursor (not the full path).
    pub current_file: &'a str,
    /// Active panel directory.
    pub active_dir: &'a Path,
    /// Opposite panel directory.
    pub other_dir: &'a Path,
    /// Tagged / selected files; if empty, falls back to `current_file`.
    pub tagged: &'a [PathBuf],
}

/// Perform MC-compatible substitutions in `cmd`.
pub fn apply_substitutions(cmd: &str, ctx: &SubstContext<'_>) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            None => out.push('%'),
            Some('%') => out.push('%'),
            Some('f') => out.push_str(&shell_quote(ctx.current_file)),
            Some('d') => out.push_str(&shell_quote(&ctx.active_dir.display().to_string())),
            Some('D') => out.push_str(&shell_quote(&ctx.other_dir.display().to_string())),
            Some('t' | 's') => {
                if ctx.tagged.is_empty() {
                    out.push_str(&shell_quote(ctx.current_file));
                } else {
                    let quoted: Vec<String> = ctx
                        .tagged
                        .iter()
                        .map(|p| shell_quote(&tagged_name(p, ctx.active_dir)))
                        .collect();
                    out.push_str(&quoted.join(" "));
                }
            }
            Some(other) => {
                // Unknown token: pass through verbatim.
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

fn tagged_name(path: &Path, active_dir: &Path) -> String {
    path.strip_prefix(active_dir)
        .ok()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| path.display().to_string())
}

/// Parse the menu file content and return all entries.
pub fn parse_menu(content: &str) -> Vec<MenuEntry> {
    parse_menu_with_warnings(content).entries
}

/// Parse the menu file content and return entries plus non-fatal warnings.
pub fn parse_menu_with_warnings(content: &str) -> ParsedMenu {
    let mut entries: Vec<MenuEntry> = Vec::new();
    let mut warnings: Vec<MenuWarning> = Vec::new();
    let mut lines = content.lines().enumerate().peekable();
    let mut pending_condition: Option<String> = None;
    let mut pending_condition_line: usize = 0;

    while let Some((line_idx, line)) = lines.next() {
        // Skip blank lines and comments.
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        // Condition line (`+ f <regex>`): consumed before the hotkey line.
        if line.starts_with('+') {
            pending_condition = parse_condition(line.trim());
            pending_condition_line = line_idx + 1;
            continue;
        }

        // Hotkey line: first char is the hotkey, rest (trimmed) is the title.
        let mut chars = line.chars();
        let hotkey = match chars.next() {
            Some(c) if !c.is_whitespace() => c,
            _ => continue,
        };
        let title = chars.as_str().trim().to_string();
        if title.is_empty() {
            continue;
        }

        // Collect indented body lines until a blank line or non-indented line.
        let mut body_lines: Vec<String> = Vec::new();
        // Collect trailing condition lines that follow the body.
        let mut condition: Option<String> = pending_condition.take();
        let mut condition_line = pending_condition_line;

        while let Some((_, next)) = lines.peek() {
            let trimmed = next.trim();
            if trimmed.is_empty() {
                // Blank line ends the entry; consume it.
                let _ = lines.next();
                break;
            }
            if next.starts_with('+') {
                // Condition line for this entry.
                let (cond_line_idx, cond_line) = lines.next().unwrap_or_default();
                condition = parse_condition(cond_line.trim());
                condition_line = cond_line_idx + 1;
                continue;
            }
            if next.starts_with('\t') || next.starts_with(' ') {
                body_lines.push(trimmed.to_string());
                let _ = lines.next();
            } else {
                // Next hotkey line; leave it for the outer loop.
                break;
            }
        }

        if body_lines.is_empty() {
            continue;
        }

        let compiled_condition = match condition {
            Some(s) => match Regex::new(&s) {
                Ok(re) => Some(re),
                Err(err) => {
                    warnings.push(MenuWarning {
                        line: condition_line,
                        message: format!("Invalid filename regex `{s}`: {err}"),
                    });
                    continue;
                }
            },
            None => None,
        };

        entries.push(MenuEntry {
            hotkey,
            title,
            command: body_lines.join("\n"),
            condition: compiled_condition,
        });
    }

    ParsedMenu { entries, warnings }
}

/// Parse a condition line of the form `+ f <regex>`.
/// Returns `Some(regex_string)` for filename-regex conditions,
/// `None` for unsupported condition types.
fn parse_condition(line: &str) -> Option<String> {
    // Strip leading `+` and whitespace.
    let rest = line.trim_start_matches('+').trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    match parts.next() {
        Some("f") => parts.next().map(|r| r.trim().to_string()),
        _ => None,
    }
}

/// Filter entries whose condition passes for `filename`.
/// Entries without a condition always pass.
pub fn filter_entries<'a>(entries: &'a [MenuEntry], filename: &str) -> Vec<&'a MenuEntry> {
    entries
        .iter()
        .filter(|e| match &e.condition {
            None => true,
            Some(re) => re.is_match(filename),
        })
        .collect()
}

/// Return the first existing menu file path, or `None`.
pub fn locate_menu_file(panel_dir: &Path) -> Option<PathBuf> {
    let local = panel_dir.join(".mc.menu");
    if local.exists() {
        return Some(local);
    }
    if let Some(cfg) = paths::user_menu_path()
        && cfg.exists()
    {
        return Some(cfg);
    }
    None
}

#[derive(Debug, Clone)]
pub struct LoadedMenu {
    pub entries: Vec<MenuEntry>,
    pub warnings: Vec<MenuWarning>,
}

/// Load and parse entries from the best menu file, preserving non-fatal warnings.
pub fn load_menu_with_warnings(panel_dir: &Path, filename: &str) -> Result<LoadedMenu, String> {
    let path = locate_menu_file(panel_dir).ok_or_else(|| {
        format!(
            "No user menu file found (searched: {}/.mc.menu, ~/.config/lc/menu)",
            panel_dir.display()
        )
    })?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read menu file {}: {e}", path.display()))?;
    let parsed = parse_menu_with_warnings(&content);
    let entries = filter_entries(&parsed.entries, filename)
        .into_iter()
        .cloned()
        .collect();
    Ok(LoadedMenu {
        entries,
        warnings: parsed.warnings,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx<'a>(
        file: &'a str,
        active: &'a Path,
        other: &'a Path,
        tagged: &'a [PathBuf],
    ) -> SubstContext<'a> {
        SubstContext {
            current_file: file,
            active_dir: active,
            other_dir: other,
            tagged,
        }
    }

    // --- shell_quote ---

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn test_shell_quote_with_spaces() {
        assert_eq!(shell_quote("my file.txt"), "'my file.txt'");
    }

    #[test]
    fn test_shell_quote_with_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    // --- apply_substitutions ---

    #[test]
    fn test_subst_percent_f() {
        let active = PathBuf::from("/home/user");
        let other = PathBuf::from("/tmp");
        let c = ctx("file.txt", &active, &other, &[]);
        assert_eq!(apply_substitutions("echo %f", &c), "echo 'file.txt'");
    }

    #[test]
    fn test_subst_percent_f_spaces() {
        let active = PathBuf::from("/home/user");
        let other = PathBuf::from("/tmp");
        let c = ctx("my document.pdf", &active, &other, &[]);
        assert_eq!(
            apply_substitutions("xdg-open %f", &c),
            "xdg-open 'my document.pdf'"
        );
    }

    #[test]
    fn test_subst_percent_d() {
        let active = PathBuf::from("/home/user/docs");
        let other = PathBuf::from("/tmp");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(apply_substitutions("ls %d", &c), "ls '/home/user/docs'");
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_subst_percent_D() {
        let active = PathBuf::from("/home/user");
        let other = PathBuf::from("/mnt/backup");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(apply_substitutions("cp %f %D", &c), "cp 'f' '/mnt/backup'");
    }

    #[test]
    fn test_subst_percent_t_no_tagged_falls_back_to_f() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let c = ctx("file.rs", &active, &other, &[]);
        assert_eq!(
            apply_substitutions("tar czf a.tgz %t", &c),
            "tar czf a.tgz 'file.rs'"
        );
    }

    #[test]
    fn test_subst_percent_s_is_alias_for_t() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let tagged = vec![PathBuf::from("/a/x.txt"), PathBuf::from("/a/y.txt")];
        let c = ctx("x.txt", &active, &other, &tagged);
        let result = apply_substitutions("tar czf a.tgz %s", &c);
        assert!(result.contains("'x.txt'"));
        assert!(result.contains("'y.txt'"));
    }

    #[test]
    fn test_subst_percent_t_multiple_files() {
        let active = PathBuf::from("/src");
        let other = PathBuf::from("/dst");
        let tagged = vec![PathBuf::from("/src/a b.txt"), PathBuf::from("/src/c.txt")];
        let c = ctx("a b.txt", &active, &other, &tagged);
        let result = apply_substitutions("cp %t /dst/", &c);
        assert_eq!(result, "cp 'a b.txt' 'c.txt' /dst/");
    }

    #[test]
    fn test_subst_percent_t_keeps_relative_paths_under_active_dir() {
        let active = PathBuf::from("/src");
        let other = PathBuf::from("/dst");
        let tagged = vec![PathBuf::from("/src/dir/a.txt")];
        let c = ctx("dir/a.txt", &active, &other, &tagged);
        let result = apply_substitutions("cp %t /dst/", &c);
        assert_eq!(result, "cp 'dir/a.txt' /dst/");
    }

    #[test]
    fn test_subst_double_percent_literal() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(apply_substitutions("echo 100%%", &c), "echo 100%");
    }

    #[test]
    fn test_subst_unknown_token_passthrough() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(apply_substitutions("echo %z", &c), "echo %z");
    }

    // --- parse_menu ---

    #[test]
    fn test_parse_simple_entry() {
        let src = "A  Archive\n\ttar czf a.tgz\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hotkey, 'A');
        assert_eq!(entries[0].title, "Archive");
        assert_eq!(entries[0].command, "tar czf a.tgz");
        assert!(entries[0].condition.is_none());
    }

    #[test]
    fn test_parse_multiple_entries() {
        let src = "A  Archive\n\ttar czf a.tgz\n\nB  Build\n\tcargo build\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].hotkey, 'A');
        assert_eq!(entries[1].hotkey, 'B');
    }

    #[test]
    fn test_parse_multi_line_body() {
        let src = "R  Run\n\texport FOO=bar\n\tcargo run\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "export FOO=bar\ncargo run");
    }

    #[test]
    fn test_parse_comments_ignored() {
        let src = "# This is a comment\nA  Archive\n\ttar czf a.tgz\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hotkey, 'A');
    }

    #[test]
    fn test_parse_blank_lines_separate_entries() {
        let src = "A  First\n\tcmd1\n\nB  Second\n\tcmd2\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_condition_f() {
        let src = "T  Test\n\tcargo test %f\n+ f \\.rs$\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].condition.is_some());
        assert!(entries[0].condition.as_ref().unwrap().is_match("main.rs"));
    }

    #[test]
    fn test_parse_condition_before_hotkey() {
        let src = "+ f \\.rs$\nT  Test\n\tcargo test %f\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].condition.is_some());
        assert!(entries[0].condition.as_ref().unwrap().is_match("foo.rs"));
    }

    #[test]
    fn test_parse_entry_no_body_skipped() {
        let src = "A  Oops\n\nB  Good\n\tcmd\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hotkey, 'B');
    }

    // --- filter_entries ---

    #[test]
    fn test_filter_no_condition_always_passes() {
        let entries = vec![MenuEntry {
            hotkey: 'A',
            title: "Anything".into(),
            command: "cmd".into(),
            condition: None,
        }];
        assert_eq!(filter_entries(&entries, "whatever.py").len(), 1);
    }

    #[test]
    fn test_filter_condition_match() {
        let entries = vec![MenuEntry {
            hotkey: 'T',
            title: "Test".into(),
            command: "cargo test".into(),
            condition: Some(Regex::new("\\.rs$").unwrap()),
        }];
        assert_eq!(filter_entries(&entries, "main.rs").len(), 1);
        assert_eq!(filter_entries(&entries, "main.py").len(), 0);
    }

    #[test]
    fn test_parse_invalid_regex_skips_entry() {
        let src = "+ f [invalid\nT  Test\n\tcmd\n";
        let entries = parse_menu(src);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_invalid_regex_reports_warning_and_keeps_valid_entries() {
        let src = "+ f [invalid\nT  Test\n\tcmd\n\nB  Build\n\tcargo build\n";
        let parsed = parse_menu_with_warnings(src);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].hotkey, 'B');
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.warnings[0].line, 1);
        assert!(
            parsed.warnings[0]
                .message
                .starts_with("Invalid filename regex `[invalid`:")
        );
    }
}
