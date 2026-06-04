use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::app::paths;

const MAX_MENU_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub enum CompiledCondition {
    Always,
    Match(Regex),
    Never,
}

/// A single entry parsed from a user menu file.
#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub hotkey: char,
    pub title: String,
    pub command: String,
    pub condition: CompiledCondition,
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
                (CompiledCondition::Always, CompiledCondition::Always) => true,
                (CompiledCondition::Never, CompiledCondition::Never) => true,
                (CompiledCondition::Match(a), CompiledCondition::Match(b)) => {
                    a.as_str() == b.as_str()
                }
                _ => false,
            }
    }
}

/// Shell-escape via single-quote wrapping.
///
/// Prevents shell metacharacter injection but does NOT protect
/// against option injection (filenames starting with `-`).
/// Use `safe_file_arg` which prepends `./` to `-`-prefixed names.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2 + 3 * s.chars().filter(|&c| c == '\'').count());
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

fn safe_file_arg(s: &str) -> String {
    if s.starts_with('-') {
        shell_quote(&format!("./{s}"))
    } else {
        shell_quote(s)
    }
}

/// Context supplied by the caller for `%`-substitutions.
#[derive(Debug)]
pub struct SubstContext<'a> {
    /// Name of the file under the cursor (not the full path).
    pub current_file: &'a Path,
    /// Active panel directory.
    pub active_dir: &'a Path,
    /// Opposite panel directory.
    pub other_dir: &'a Path,
    /// Tagged / selected files; if empty, falls back to `current_file`.
    pub tagged: &'a [PathBuf],
}

const NON_UTF8_ERR: &str = "non-UTF-8 path not supported in menu";

const INDENT_CHARS: &[char] = &['\t', ' ', '+'];

fn non_utf8_err() -> String {
    NON_UTF8_ERR.to_owned()
}

pub fn apply_substitutions(cmd: &str, ctx: &SubstContext<'_>) -> Result<String, String> {
    let mut out = String::with_capacity(cmd.len() * 2);
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            None => out.push('%'),
            Some('%') => out.push('%'),
            Some('f') => {
                let name = ctx
                    .current_file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(non_utf8_err)?;
                out.push_str(&safe_file_arg(name));
            }
            Some('d') => {
                out.push_str(&shell_quote(
                    ctx.active_dir.to_str().ok_or_else(non_utf8_err)?,
                ));
            }
            Some('D') => {
                out.push_str(&shell_quote(
                    ctx.other_dir.to_str().ok_or_else(non_utf8_err)?,
                ));
            }
            Some('t' | 's') => {
                if ctx.tagged.is_empty() {
                    let name = ctx
                        .current_file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .ok_or_else(non_utf8_err)?;
                    out.push_str(&safe_file_arg(name));
                } else {
                    let quoted: Result<Vec<String>, String> = ctx
                        .tagged
                        .iter()
                        .map(|p| tagged_name(p, ctx.active_dir).map(|n| safe_file_arg(&n)))
                        .collect();
                    out.push_str(&quoted?.join(" "));
                }
            }
            Some(other) => {
                out.push('%');
                out.push(other);
            }
        }
    }
    Ok(out)
}

fn tagged_name(path: &Path, active_dir: &Path) -> Result<String, String> {
    // Root path "/" has no parent; use the full path as-is since strip_prefix
    // cannot produce a relative name and file_name() returns None for "/".
    if path.is_absolute() && path.parent().is_none() {
        return path
            .to_str()
            .map(ToOwned::to_owned)
            .ok_or_else(non_utf8_err);
    }
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
        .ok_or_else(non_utf8_err)
}

/// Parse the menu file content and return all entries.
pub fn parse_menu(content: &str) -> Vec<MenuEntry> {
    parse_menu_with_warnings(content).entries
}

struct BodyCollectResult {
    body_lines: Vec<String>,
    condition: Option<ConditionParseResult>,
    condition_line: usize,
}

fn collect_body_lines(
    lines: &mut std::iter::Peekable<std::iter::Enumerate<std::str::Lines<'_>>>,
    initial_condition: Option<ConditionParseResult>,
    initial_condition_line: usize,
) -> BodyCollectResult {
    let mut body_lines: Vec<String> = Vec::new();
    let mut condition = initial_condition;
    let mut condition_line = initial_condition_line;

    while let Some((_, next)) = lines.peek() {
        let trimmed = next.trim();
        if trimmed.is_empty() {
            let _ = lines.next();
            break;
        }
        if is_condition_line(next) {
            let Some((cond_line_idx, cond_line)) = lines.next() else {
                break;
            };
            let new_cond = parse_condition(cond_line.trim());
            condition = merge_conditions(condition.take(), new_cond);
            condition_line = cond_line_idx + 1;
            continue;
        }
        if let Some(rest) = next.strip_prefix(INDENT_CHARS) {
            body_lines.push(rest.to_string());
            let _ = lines.next();
        } else {
            break;
        }
    }

    BodyCollectResult {
        body_lines,
        condition,
        condition_line,
    }
}

/// Parse the menu file content and return entries plus non-fatal warnings.
pub fn parse_menu_with_warnings(content: &str) -> ParsedMenu {
    let mut entries: Vec<MenuEntry> = Vec::new();
    let mut warnings: Vec<MenuWarning> = Vec::new();
    let mut lines = content.lines().enumerate().peekable();
    let mut pending_condition: Option<ConditionParseResult> = None;
    let mut pending_condition_line: usize = 0;

    while let Some((line_idx, line)) = lines.next() {
        // Skip blank lines and comments.
        if line.trim().is_empty() || line.starts_with('#') {
            pending_condition = None;
            continue;
        }

        // Condition line (`+ f <regex>`): consumed before the hotkey line.
        // Only matches `+ ` or `+\t` prefix — bare `+text` falls through.
        if is_condition_line(line) {
            let new_cond = parse_condition(line.trim());
            pending_condition = merge_conditions(pending_condition.take(), new_cond);
            pending_condition_line = line_idx + 1;
            continue;
        }

        // Hotkey line: first char is the hotkey, rest (trimmed) is the title.
        let mut chars = line.chars();
        let hotkey = match chars.next() {
            Some(c) if !c.is_whitespace() => c,
            _ => {
                pending_condition = None;
                continue;
            }
        };
        let title = chars.as_str().trim().to_string();
        if title.is_empty() {
            pending_condition = None;
            continue;
        }

        let result =
            collect_body_lines(&mut lines, pending_condition.take(), pending_condition_line);
        let body_lines = result.body_lines;
        let condition = result.condition;
        let condition_line = result.condition_line;

        if body_lines.is_empty() {
            continue;
        }

        let compiled_condition = match condition {
            Some(ConditionParseResult::Pattern(s)) => match Regex::new(&s) {
                Ok(re) => CompiledCondition::Match(re),
                Err(err) => {
                    warnings.push(MenuWarning {
                        line: condition_line,
                        message: format!("Invalid filename regex `{s}`: {err}"),
                    });
                    CompiledCondition::Never
                }
            },
            Some(ConditionParseResult::Unsupported) => {
                warnings.push(MenuWarning {
                    line: condition_line,
                    message: "Unsupported condition type, entry will never match".into(),
                });
                CompiledCondition::Never
            }
            None => CompiledCondition::Always,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConditionParseResult {
    Pattern(String),
    Unsupported,
}

fn is_condition_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.first() == Some(&b'+') && bytes.get(1).is_some_and(|&b| b == b' ' || b == b'\t')
}

fn merge_conditions(
    existing: Option<ConditionParseResult>,
    new: Option<ConditionParseResult>,
) -> Option<ConditionParseResult> {
    match (existing, new) {
        (Some(ConditionParseResult::Pattern(a)), Some(ConditionParseResult::Pattern(b))) => {
            Some(ConditionParseResult::Pattern(format!("(?:{a})|(?:{b})")))
        }
        (old, None) => old,
        (_, new) => new,
    }
}

/// Parse a condition line of the form `+ f <regex>`.
/// Returns `Some(Pattern(regex_string))` for filename-regex conditions,
/// `Some(Unsupported)` for unrecognized condition types,
/// `None` if the line is empty or has no condition type.
fn parse_condition(line: &str) -> Option<ConditionParseResult> {
    // Strip leading `+` and whitespace.
    // Defensive fallback: callers already verify the leading '+' via
    // is_condition_line, but parse_condition may also be invoked directly.
    let rest = line.strip_prefix('+').unwrap_or(line).trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    match parts.next() {
        Some("f") => parts
            .next()
            .map(|r| r.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(ConditionParseResult::Pattern),
        Some(_) => Some(ConditionParseResult::Unsupported),
        None => None,
    }
}

/// Filter entries whose condition passes for `filename`.
/// Entries without a condition always pass.
pub fn filter_entries<'a>(entries: &'a [MenuEntry], filename: &str) -> Vec<&'a MenuEntry> {
    entries
        .iter()
        .filter(|e| match &e.condition {
            CompiledCondition::Always => true,
            CompiledCondition::Match(re) => re.is_match(filename),
            CompiledCondition::Never => false,
        })
        .collect()
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| !m.is_symlink() && m.is_file())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuSource {
    Local,
    Global,
}

pub fn locate_menu_file(panel_dir: &Path) -> Option<(PathBuf, MenuSource)> {
    locate_menu_file_with_global(panel_dir, paths::user_menu_path().as_deref())
}

fn locate_menu_file_with_global(
    panel_dir: &Path,
    global_path: Option<&Path>,
) -> Option<(PathBuf, MenuSource)> {
    let local = panel_dir.join(".mc.menu");
    if is_regular_file(&local) {
        return Some((local, MenuSource::Local));
    }
    if let Some(cfg) = global_path
        && is_regular_file(cfg)
    {
        return Some((cfg.to_path_buf(), MenuSource::Global));
    }
    None
}

#[derive(Debug, Clone)]
pub struct LoadedMenu {
    pub entries: Vec<MenuEntry>,
    pub warnings: Vec<MenuWarning>,
    pub source: MenuSource,
}

pub fn load_menu_with_warnings(panel_dir: &Path, filename: &str) -> Result<LoadedMenu, String> {
    let (path, source) = locate_menu_file(panel_dir).ok_or_else(|| {
        format!(
            "No user menu file found (searched: {}/.mc.menu, ~/.config/lc/menu)",
            panel_dir.display()
        )
    })?;
    let mut content = String::new();
    File::open(&path)
        .map_err(|e| format!("Failed to open menu file {}: {e}", path.display()))?
        .take(MAX_MENU_FILE_BYTES)
        .read_to_string(&mut content)
        .map_err(|e| format!("Failed to read menu file {}: {e}", path.display()))?;
    let parsed = parse_menu_with_warnings(&content);
    let entries = filter_entries(&parsed.entries, filename)
        .into_iter()
        .cloned()
        .collect();
    Ok(LoadedMenu {
        entries,
        warnings: parsed.warnings,
        source,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn ctx<'a>(
        file: &'a str,
        active: &'a Path,
        other: &'a Path,
        tagged: &'a [PathBuf],
    ) -> SubstContext<'a> {
        SubstContext {
            current_file: Path::new(file),
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
        assert_eq!(
            apply_substitutions("echo %f", &c).unwrap(),
            "echo 'file.txt'"
        );
    }

    #[test]
    fn test_subst_percent_f_spaces() {
        let active = PathBuf::from("/home/user");
        let other = PathBuf::from("/tmp");
        let c = ctx("my document.pdf", &active, &other, &[]);
        assert_eq!(
            apply_substitutions("xdg-open %f", &c).unwrap(),
            "xdg-open 'my document.pdf'"
        );
    }

    #[test]
    fn test_subst_percent_d() {
        let active = PathBuf::from("/home/user/docs");
        let other = PathBuf::from("/tmp");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(
            apply_substitutions("ls %d", &c).unwrap(),
            "ls '/home/user/docs'"
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_subst_percent_D() {
        let active = PathBuf::from("/home/user");
        let other = PathBuf::from("/mnt/backup");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(
            apply_substitutions("cp %f %D", &c).unwrap(),
            "cp 'f' '/mnt/backup'"
        );
    }

    #[test]
    fn test_subst_percent_t_no_tagged_falls_back_to_f() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let c = ctx("file.rs", &active, &other, &[]);
        assert_eq!(
            apply_substitutions("tar czf a.tgz %t", &c).unwrap(),
            "tar czf a.tgz 'file.rs'"
        );
    }

    #[test]
    fn test_subst_percent_s_is_alias_for_t() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let tagged = vec![PathBuf::from("/a/x.txt"), PathBuf::from("/a/y.txt")];
        let c = ctx("x.txt", &active, &other, &tagged);
        let result = apply_substitutions("tar czf a.tgz %s", &c).unwrap();
        assert!(result.contains("'x.txt'"));
        assert!(result.contains("'y.txt'"));
    }

    #[test]
    fn test_subst_percent_t_multiple_files() {
        let active = PathBuf::from("/src");
        let other = PathBuf::from("/dst");
        let tagged = vec![PathBuf::from("/src/a b.txt"), PathBuf::from("/src/c.txt")];
        let c = ctx("a b.txt", &active, &other, &tagged);
        let result = apply_substitutions("cp %t /dst/", &c).unwrap();
        assert_eq!(result, "cp 'a b.txt' 'c.txt' /dst/");
    }

    #[test]
    fn test_subst_percent_t_keeps_relative_paths_under_active_dir() {
        let active = PathBuf::from("/src");
        let other = PathBuf::from("/dst");
        let tagged = vec![PathBuf::from("/src/dir/a.txt")];
        let c = ctx("dir/a.txt", &active, &other, &tagged);
        let result = apply_substitutions("cp %t /dst/", &c).unwrap();
        assert_eq!(result, "cp 'dir/a.txt' /dst/");
    }

    #[test]
    fn test_subst_double_percent_literal() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(apply_substitutions("echo 100%%", &c).unwrap(), "echo 100%");
    }

    #[test]
    fn test_subst_unknown_token_passthrough() {
        let active = PathBuf::from("/a");
        let other = PathBuf::from("/b");
        let c = ctx("f", &active, &other, &[]);
        assert_eq!(apply_substitutions("echo %z", &c).unwrap(), "echo %z");
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
        assert!(matches!(entries[0].condition, CompiledCondition::Always));
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
        assert!(matches!(entries[0].condition, CompiledCondition::Match(_)));
        assert!(!filter_entries(&entries, "main.rs").is_empty());
    }

    #[test]
    fn test_parse_condition_before_hotkey() {
        let src = "+ f \\.rs$\nT  Test\n\tcargo test %f\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].condition, CompiledCondition::Match(_)));
        assert!(!filter_entries(&entries, "foo.rs").is_empty());
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
            condition: CompiledCondition::Always,
        }];
        assert_eq!(filter_entries(&entries, "whatever.py").len(), 1);
    }

    #[test]
    fn test_filter_condition_match() {
        let entries = vec![MenuEntry {
            hotkey: 'T',
            title: "Test".into(),
            command: "cargo test".into(),
            condition: CompiledCondition::Match(Regex::new("\\.rs$").unwrap()),
        }];
        assert_eq!(filter_entries(&entries, "main.rs").len(), 1);
        assert_eq!(filter_entries(&entries, "main.py").len(), 0);
    }

    #[test]
    fn test_parse_condition_f_empty_pattern_is_none() {
        assert_eq!(parse_condition("+ f"), None);
        assert_eq!(parse_condition("+ f "), None);
        assert_eq!(parse_condition("+ f  "), None);
    }

    #[test]
    fn test_parse_condition_f_empty_pattern_entry_has_no_condition() {
        let src = "T  Test\n\tcmd\n+ f \n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].condition, CompiledCondition::Always));
    }

    #[test]
    fn test_parse_invalid_regex_keeps_entry_visible() {
        let src = "+ f [invalid\nT  Test\n\tcmd\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hotkey, 'T');
        assert!(matches!(entries[0].condition, CompiledCondition::Never));
        assert_eq!(filter_entries(&entries, "anything.txt").len(), 0);
    }

    #[test]
    fn test_parse_invalid_regex_reports_warning() {
        let src = "+ f [invalid\nT  Test\n\tcmd\n\nB  Build\n\tcargo build\n";
        let parsed = parse_menu_with_warnings(src);
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].hotkey, 'T');
        assert!(matches!(
            parsed.entries[0].condition,
            CompiledCondition::Never
        ));
        assert_eq!(filter_entries(&parsed.entries, "anything.txt").len(), 1);
        assert_eq!(parsed.entries[1].hotkey, 'B');
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.warnings[0].line, 1);
        assert!(
            parsed.warnings[0]
                .message
                .starts_with("Invalid filename regex `[invalid`:")
        );
    }

    #[test]
    fn test_parse_unsupported_condition_keeps_entry_visible() {
        let src = "+ d /tmp\nT  Test\n\tcmd\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hotkey, 'T');
        assert!(matches!(entries[0].condition, CompiledCondition::Never));
        assert_eq!(filter_entries(&entries, "anything.txt").len(), 0);
    }

    #[test]
    fn test_parse_unsupported_condition_reports_warning() {
        let src = "+ d /tmp\nT  Test\n\tcmd\n\nB  Build\n\tcargo build\n";
        let parsed = parse_menu_with_warnings(src);
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].hotkey, 'T');
        assert!(matches!(
            parsed.entries[0].condition,
            CompiledCondition::Never
        ));
        assert_eq!(filter_entries(&parsed.entries, "anything.txt").len(), 1);
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(
            parsed.warnings[0].message,
            "Unsupported condition type, entry will never match"
        );
    }

    #[test]
    fn test_locate_menu_file_prefers_local_over_global() {
        let temp = tempfile::tempdir().unwrap();
        let panel_dir = temp.path().join("panel");
        let config_dir = temp.path().join("config");
        fs::create_dir(&panel_dir).unwrap();
        fs::create_dir(&config_dir).unwrap();

        let local = panel_dir.join(".mc.menu");
        let global = config_dir.join("menu");
        fs::write(&local, "L  Local\n\tcmd\n").unwrap();
        fs::write(&global, "G  Global\n\tcmd\n").unwrap();

        let located = locate_menu_file_with_global(&panel_dir, Some(&global)).unwrap();
        assert_eq!(located, (local, MenuSource::Local));
    }

    #[test]
    fn test_consecutive_conditions_merged() {
        let src = "+ f \\.rs$\n+ f \\.toml$\nT  Test\n\tcmd %f\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].condition, CompiledCondition::Match(_)));
        assert!(!filter_entries(&entries, "foo.rs").is_empty());
        assert!(!filter_entries(&entries, "cargo.toml").is_empty());
        assert!(filter_entries(&entries, "foo.py").is_empty());
    }

    #[test]
    fn test_unindented_plus_without_whitespace_treated_as_body() {
        let src = "A  Add\n\tcmd\n+extra_arg\n";
        let entries = parse_menu(src);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].command.contains("extra_arg"));
    }
}
