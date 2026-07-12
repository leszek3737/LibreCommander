use crate::debug_log;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

/// Stable per-file identity used for symlink-cycle detection.
///
/// Returns `None` when the platform cannot provide reliable identifiers, so
/// callers MUST then skip cycle detection rather than substitute a colliding
/// sentinel (a shared sentinel would make two distinct directories look like the
/// same node and trigger false-positive cycle suppression).
#[cfg(unix)]
fn file_key(metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_key(_metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
    // No STABLE identifiers off Unix: Windows' volume_serial_number()/
    // file_index() need the unstable `windows_by_handle` feature
    // (rust-lang/rust#63010), so cycle detection is skipped entirely.
    None
}

use crate::ops::sorting::cmp_ignore_case;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TreeDiagnosticKind {
    #[default]
    ReadDir,
    ReadDirEntry,
    ReadMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeDiagnostic {
    pub path: PathBuf,
    pub message: String,
    pub kind: TreeDiagnosticKind,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TreeBuildResult {
    pub entries: Vec<TreeEntry>,
    pub diagnostics: Vec<TreeDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub name: String,
    pub name_width: usize,
    pub read_error: bool,
}

/// Build a flat tree listing from `root`, expanding directories up to `initial_depth` levels.
/// Returns the list sorted: directories first (alphabetically), then files (alphabetically),
/// within each parent directory.
pub fn build_tree(root: &Path, initial_depth: usize, show_hidden: bool) -> Vec<TreeEntry> {
    build_tree_with_diagnostics(root, initial_depth, show_hidden).entries
}

pub fn build_tree_with_diagnostics(
    root: &Path,
    initial_depth: usize,
    show_hidden: bool,
) -> TreeBuildResult {
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    let mut visited = HashSet::new();
    insert_root_key(root, &mut visited);
    build_tree_recursive(
        root,
        0,
        initial_depth,
        show_hidden,
        &mut entries,
        &mut diagnostics,
        &mut visited,
    );
    TreeBuildResult {
        entries,
        diagnostics,
    }
}

fn insert_root_key(root: &Path, visited: &mut HashSet<(u64, u64)>) {
    if let Ok(meta) = root.metadata()
        && let Some(key) = file_key(&meta)
    {
        let _ = visited.insert(key);
    }
}

fn build_tree_recursive(
    dir: &Path,
    current_depth: usize,
    max_expand_depth: usize,
    show_hidden: bool,
    out: &mut Vec<TreeEntry>,
    diagnostics: &mut Vec<TreeDiagnostic>,
    visited: &mut HashSet<(u64, u64)>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(err) => {
            diagnostics.push(TreeDiagnostic {
                path: dir.to_path_buf(),
                message: format!("Failed to read directory: {err}"),
                kind: TreeDiagnosticKind::ReadDir,
            });
            return;
        }
    };

    // Collect the raw entries first so `children` can be sized from the actual
    // entry count instead of a fixed guess (the old `with_capacity(16)` caused
    // ~7 reallocations at ~1000 entries). The intermediate Vec holds cheap
    // `DirEntry` results; the costly `TreeEntry` Vec is then allocated once.
    let raw_entries: Vec<_> = read_dir.collect();
    let mut children: Vec<TreeEntry> = Vec::with_capacity(raw_entries.len());
    for entry in raw_entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                diagnostics.push(TreeDiagnostic {
                    path: dir.to_path_buf(),
                    message: format!("Failed to read directory entry: {err}"),
                    kind: TreeDiagnosticKind::ReadDirEntry,
                });
                continue;
            }
        };
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let Some(is_dir) = classify_is_dir(&entry, &path, diagnostics) else {
            continue;
        };
        let expanded = is_dir && current_depth < max_expand_depth;
        let name_width = UnicodeWidthStr::width(name.as_str());

        children.push(TreeEntry {
            path,
            depth: current_depth,
            is_dir,
            expanded,
            name,
            name_width,
            read_error: false,
        });
    }

    sort_entries(&mut children);
    recurse_children(
        children,
        current_depth,
        max_expand_depth,
        show_hidden,
        out,
        diagnostics,
        visited,
    );
}

/// Classify whether `entry` is a directory, recording a diagnostic and
/// returning `None` when the type cannot be determined.
///
/// `entry.file_type()` is the *un-followed* type (lstat-equivalent — the same
/// view as `symlink_metadata`), already cached by `read_dir` on most platforms,
/// so the common non-symlink case costs no extra syscall. For a symlink we
/// deliberately follow it via `path.metadata()` (stat-equivalent) so a
/// symlink-to-directory shows up as an expandable directory, while a broken
/// symlink — whose `metadata()` fails — stays classified as a plain file.
fn classify_is_dir(
    entry: &std::fs::DirEntry,
    path: &Path,
    diagnostics: &mut Vec<TreeDiagnostic>,
) -> Option<bool> {
    match entry.file_type() {
        Ok(ft) if !ft.is_symlink() => Some(ft.is_dir()),
        Ok(_) => Some(path.metadata().is_ok_and(|m| m.is_dir())),
        Err(err) => {
            diagnostics.push(TreeDiagnostic {
                path: path.to_path_buf(),
                message: format!("Failed to read metadata: {err}"),
                kind: TreeDiagnosticKind::ReadMetadata,
            });
            None
        }
    }
}

/// Outcome of weighing a directory's stable identity against the keys already
/// on the current descent path.
#[derive(Debug, PartialEq, Eq)]
enum DescentDecision {
    /// The key was already on the descent path: a symlink cycle. Keep the entry
    /// but do not descend.
    Cycle,
    /// Descent may proceed. Carries the key to release once the subtree has been
    /// walked, or `None` when the platform supplied no stable identifier and the
    /// descent therefore goes untracked.
    Descend(Option<(u64, u64)>),
}

/// Decide whether descending into a directory with the given `key` would form a
/// symlink cycle, recording newly seen keys in `visited`.
///
/// A `None` key means the platform cannot identify this directory (see
/// `file_key`); cycle detection is then SKIPPED and descent always proceeds.
/// Substituting a shared sentinel (e.g. the old Windows `(0, 0)` fallback) for
/// `None` would make distinct directories collide and be falsely suppressed as
/// cycles — the bug this seam exists to guard against. Factored out of
/// `recurse_children` so the `None`-skip branch is reachable from tests; it is
/// otherwise dead on Unix/Windows, where real metadata always yields a key.
fn classify_descent(key: Option<(u64, u64)>, visited: &mut HashSet<(u64, u64)>) -> DescentDecision {
    match key {
        Some(k) if !visited.insert(k) => DescentDecision::Cycle,
        Some(k) => DescentDecision::Descend(Some(k)),
        None => DescentDecision::Descend(None),
    }
}

/// Emit sorted `children` into `out`, descending into expandable directories
/// while guarding against symlink cycles. Every child is pushed exactly once;
/// directories are never silently dropped.
fn recurse_children(
    children: Vec<TreeEntry>,
    current_depth: usize,
    max_expand_depth: usize,
    show_hidden: bool,
    out: &mut Vec<TreeEntry>,
    diagnostics: &mut Vec<TreeDiagnostic>,
    visited: &mut HashSet<(u64, u64)>,
) {
    for mut child in children {
        if !(child.is_dir && child.expanded) {
            out.push(child);
            continue;
        }

        // We intend to descend, so read metadata to derive a cycle key.
        let meta = match child.path.metadata() {
            Ok(meta) => meta,
            Err(err) => {
                // Metadata read failed: keep the entry visible but flag it and
                // do not descend (its contents are unknown). Regression guard:
                // this branch previously `continue`d and dropped the entry.
                diagnostics.push(TreeDiagnostic {
                    path: child.path.clone(),
                    message: format!("Failed to read metadata: {err}"),
                    kind: TreeDiagnosticKind::ReadMetadata,
                });
                child.read_error = true;
                child.expanded = false;
                out.push(child);
                continue;
            }
        };

        // `None` => no stable identifier on this platform: skip cycle detection
        // and descend untracked. `Some(key)` already in `visited` => cycle on
        // the current descent path: keep the entry but do not recurse (and clear
        // `expanded`, since it has no listed children). Regression guard: a
        // detected cycle previously dropped the entry entirely. The three-way
        // decision lives in `classify_descent` so it can be unit-tested.
        let tracked_key = match classify_descent(file_key(&meta), visited) {
            DescentDecision::Cycle => {
                child.expanded = false;
                out.push(child);
                continue;
            }
            DescentDecision::Descend(key) => key,
        };

        // Clone the path before `out.push(child)` moves the entry out of scope;
        // the clone backs the recursive descent below.
        let child_path = child.path.clone();
        out.push(child);
        build_tree_recursive(
            &child_path,
            current_depth + 1,
            max_expand_depth,
            show_hidden,
            out,
            diagnostics,
            visited,
        );
        // Release the key after recursion so the same directory may reappear in
        // sibling branches — cycle protection is per-descent-path only.
        if let Some(key) = tracked_key {
            visited.remove(&key);
        }
    }
}

fn sort_entries(entries: &mut [TreeEntry]) {
    entries.sort_unstable_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| cmp_ignore_case(&a.name, &b.name))
    });
}

/// Toggle expansion of the directory at `index` in `entries`.
/// If expanding: reads children and inserts them after the entry.
/// If collapsing: removes all descendants (entries at greater depth until we return
/// to the same or lesser depth).
pub fn toggle_expand(entries: &mut Vec<TreeEntry>, index: usize, show_hidden: bool) {
    let diagnostics = toggle_expand_with_diagnostics(entries, index, show_hidden);
    if !diagnostics.is_empty() {
        debug_log!("Tree expand errors: {}", diagnostics.len());
        for diag in &diagnostics {
            debug_log!("  {}: {}", diag.path.display(), diag.message);
        }
    }
}

pub fn toggle_expand_with_diagnostics(
    entries: &mut Vec<TreeEntry>,
    index: usize,
    show_hidden: bool,
) -> Vec<TreeDiagnostic> {
    let (is_dir, expanded, depth, path) = match entries.get(index) {
        Some(e) => (e.is_dir, e.expanded, e.depth, e.path.clone()),
        None => return Vec::new(),
    };

    if !is_dir {
        return Vec::new();
    }

    if expanded {
        let mut end = index + 1;
        while end < entries.len() && entries[end].depth > depth {
            end += 1;
        }
        entries.drain(index + 1..end);
        entries[index].expanded = false;
        Vec::new()
    } else {
        let mut children = Vec::new();
        let mut diagnostics = Vec::new();
        let mut visited = HashSet::new();
        insert_root_key(&path, &mut visited);
        build_tree_recursive(
            &path,
            depth + 1,
            depth + 1,
            show_hidden,
            &mut children,
            &mut diagnostics,
            &mut visited,
        );

        // Single source of truth, decided once after the recursion has fully
        // populated `diagnostics`: the directory itself was unreadable iff a
        // `ReadDir` error was recorded for `path`. Child reads recurse under
        // their own paths, so only the top-level read of `path` can match here.
        let root_read_failed = diagnostics
            .iter()
            .any(|diag| diag.kind == TreeDiagnosticKind::ReadDir && diag.path == path);

        let insert_pos = index + 1;
        entries.splice(insert_pos..insert_pos, children);
        entries[index].expanded = !root_read_failed;
        entries[index].read_error = root_read_failed;
        diagnostics
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create structure:
        // root/
        //   a.txt
        //   b.txt
        //   sub1/
        //     c.txt
        //     deep/
        //       d.txt
        //   sub2/
        //     e.txt
        //   .hidden_dir/
        //     f.txt
        //   .hidden_file
        fs::create_dir(root.join("sub1")).unwrap();
        fs::create_dir(root.join("sub1").join("deep")).unwrap();
        fs::create_dir(root.join("sub2")).unwrap();
        fs::create_dir(root.join(".hidden_dir")).unwrap();

        fs::File::create(root.join("a.txt")).unwrap();
        fs::File::create(root.join("b.txt")).unwrap();
        fs::File::create(root.join("sub1").join("c.txt")).unwrap();
        fs::File::create(root.join("sub1").join("deep").join("d.txt")).unwrap();
        fs::File::create(root.join("sub2").join("e.txt")).unwrap();
        fs::File::create(root.join(".hidden_dir").join("f.txt")).unwrap();
        fs::File::create(root.join(".hidden_file")).unwrap();

        dir
    }

    #[test]
    fn build_tree_flat_structure() {
        let dir = setup_test_dir();
        let entries = build_tree(dir.path(), 0, false);

        // With initial_depth=0 (expand 0 levels = nothing expanded), all dirs are collapsed.
        // Dirs come first: sub1, sub2; then files: a.txt, b.txt
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["sub1", "sub2", "a.txt", "b.txt"]);
        for entry in &entries {
            assert!(!entry.expanded);
        }
    }

    #[test]
    fn build_tree_respects_max_depth() {
        let dir = setup_test_dir();
        // initial_depth=1 means expand first 1 levels (depth 0 is expanded).
        let entries = build_tree(dir.path(), 1, false);

        // sub1 is expanded, so its children appear: deep (dir, collapsed), c.txt (file)
        // sub2 is expanded: e.txt
        // Hidden entries skipped. Dirs sorted before files within each directory.
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["sub1", "deep", "c.txt", "sub2", "e.txt", "a.txt", "b.txt"]
        );

        // sub1 and sub2 should be expanded, deep should not
        assert!(entries.iter().find(|e| e.name == "sub1").unwrap().expanded);
        assert!(entries.iter().find(|e| e.name == "sub2").unwrap().expanded);
        assert!(!entries.iter().find(|e| e.name == "deep").unwrap().expanded);
    }

    #[test]
    fn toggle_expand_adds_children() {
        let dir = setup_test_dir();
        let mut entries = build_tree(dir.path(), 0, false);

        // Initially sub1 is collapsed
        let sub1_idx = entries.iter().position(|e| e.name == "sub1").unwrap();
        assert!(!entries[sub1_idx].expanded);

        toggle_expand(&mut entries, sub1_idx, false);

        // sub1 should now be expanded with its children
        assert!(entries[sub1_idx].expanded);
        let child_names: Vec<&str> = entries[sub1_idx + 1..]
            .iter()
            .take_while(|e| e.depth > entries[sub1_idx].depth)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(child_names, vec!["deep", "c.txt"]);
    }

    #[test]
    fn toggle_collapse_removes_children() {
        let dir = setup_test_dir();
        let mut entries = build_tree(dir.path(), 1, false);

        // sub1 is expanded with children c.txt and deep
        let sub1_idx = entries.iter().position(|e| e.name == "sub1").unwrap();
        assert!(entries[sub1_idx].expanded);
        let count_before = entries.len();

        toggle_expand(&mut entries, sub1_idx, false);

        assert!(!entries[sub1_idx].expanded);
        assert!(entries.len() < count_before);
        // After sub1, the next entry should be sub2 (not c.txt)
        let next_name = entries.get(sub1_idx + 1).map(|e| e.name.as_str());
        assert_eq!(next_name, Some("sub2"));
    }

    #[test]
    fn hidden_dirs_skipped_when_show_hidden_false() {
        let dir = setup_test_dir();

        let entries = build_tree(dir.path(), 2, false);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert!(!names.contains(&".hidden_dir"));
        assert!(!names.contains(&".hidden_file"));
    }

    #[test]
    fn hidden_dirs_shown_when_show_hidden_true() {
        let dir = setup_test_dir();

        let entries = build_tree(dir.path(), 2, true);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert!(names.contains(&".hidden_dir"));
        assert!(names.contains(&".hidden_file"));
    }

    #[test]
    fn toggle_expand_file_is_noop() {
        let dir = setup_test_dir();
        let mut entries = build_tree(dir.path(), 0, false);

        let a_idx = entries.iter().position(|e| e.name == "a.txt").unwrap();
        let len_before = entries.len();
        toggle_expand(&mut entries, a_idx, false);
        assert_eq!(entries.len(), len_before);
    }

    #[test]
    fn toggle_expand_out_of_bounds_is_noop() {
        let dir = setup_test_dir();
        let mut entries = build_tree(dir.path(), 0, false);
        let len_before = entries.len();
        toggle_expand(&mut entries, 9999, false);
        assert_eq!(entries.len(), len_before);
    }

    #[test]
    fn build_tree_reports_unreadable_root() {
        let missing = std::env::temp_dir().join(format!(
            "lc_dir_tree_missing_{}_{}",
            std::process::id(),
            "root"
        ));

        let result = build_tree_with_diagnostics(&missing, 0, false);

        assert!(result.entries.is_empty());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].path, missing);
        assert!(
            result.diagnostics[0]
                .message
                .starts_with("Failed to read directory:")
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_tree_keeps_broken_symlink_as_symlink_not_file() {
        let dir = tempfile::tempdir().unwrap();
        let broken_link = dir.path().join("broken-link");
        std::os::unix::fs::symlink(dir.path().join("missing-target"), &broken_link).unwrap();

        let result = build_tree_with_diagnostics(dir.path(), 0, false);

        let entry = result
            .entries
            .iter()
            .find(|entry| entry.path == broken_link)
            .unwrap();
        assert!(!entry.is_dir);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn toggle_expand_reports_read_errors_and_stays_collapsed() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let mut entries = vec![TreeEntry {
            path: missing.clone(),
            depth: 0,
            is_dir: true,
            expanded: false,
            name: "missing".to_string(),
            name_width: 7,
            read_error: false,
        }];

        let diagnostics = toggle_expand_with_diagnostics(&mut entries, 0, false);

        assert_eq!(entries.len(), 1);
        assert!(!entries[0].expanded);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, missing);
        assert!(
            diagnostics[0]
                .message
                .starts_with("Failed to read directory:")
        );
    }

    #[test]
    fn recurse_children_keeps_entry_with_read_error_on_metadata_failure() {
        // Regression (#68): a directory slated for descent whose metadata read
        // fails used to be dropped entirely. It must instead stay in the output,
        // flagged `read_error == true` and collapsed, with a diagnostic.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("vanished-dir");
        let child = TreeEntry {
            path: missing.clone(),
            depth: 0,
            is_dir: true,
            expanded: true,
            name: "vanished-dir".to_string(),
            name_width: 12,
            read_error: false,
        };

        let mut out = Vec::new();
        let mut diagnostics = Vec::new();
        let mut visited = HashSet::new();
        recurse_children(
            vec![child],
            0,
            1,
            false,
            &mut out,
            &mut diagnostics,
            &mut visited,
        );

        assert_eq!(out.len(), 1, "metadata failure must not drop the entry");
        assert_eq!(out[0].path, missing);
        assert!(out[0].read_error, "metadata failure must set read_error");
        assert!(!out[0].expanded, "unreadable dir must not stay expanded");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, TreeDiagnosticKind::ReadMetadata);
    }

    #[cfg(unix)]
    #[test]
    fn build_tree_keeps_entry_on_symlink_cycle() {
        // Regression (#68): a child whose descent forms a cycle used to vanish
        // because the entry was only pushed inside the `visited.insert` branch.
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        // Symlink pointing back at its own parent directory -> descent cycle.
        std::os::unix::fs::symlink(&sub, sub.join("self-link")).unwrap();

        let result = build_tree_with_diagnostics(dir.path(), 5, false);
        let names: Vec<&str> = result.entries.iter().map(|e| e.name.as_str()).collect();

        assert!(
            names.contains(&"self-link"),
            "cyclic entry was dropped: {names:?}"
        );
        let link = result
            .entries
            .iter()
            .find(|e| e.name == "self-link")
            .unwrap();
        assert!(!link.expanded, "cyclic dir must not be marked expanded");
        // Cycle detection must stop the walk: the entry appears exactly once.
        assert_eq!(
            names.iter().filter(|n| **n == "self-link").count(),
            1,
            "cycle was not contained: {names:?}"
        );
    }

    #[test]
    fn build_tree_expands_distinct_sibling_dirs_independently() {
        // Intent documentation, NOT a regression guard on this platform: the
        // collision class it describes (the old Windows `(0, 0)` `file_key`
        // fallback that made two distinct directories share a key) cannot occur
        // on Unix, where every inode is distinct, so this passes against the old
        // code too. The portable, non-tautological guards for that fix live in
        // `classify_descent_keeps_distinct_keys_independent` and
        // `classify_descent_skips_cycle_check_when_key_unavailable`; this test
        // only asserts the end-to-end happy path of distinct siblings expanding.
        let dir = tempfile::tempdir().unwrap();
        let d1 = dir.path().join("d1");
        let d2 = dir.path().join("d2");
        fs::create_dir(&d1).unwrap();
        fs::create_dir(&d2).unwrap();
        fs::File::create(d1.join("f1.txt")).unwrap();
        fs::File::create(d2.join("f2.txt")).unwrap();

        let entries = build_tree(dir.path(), 1, false);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert!(names.contains(&"f1.txt"), "d1 was suppressed: {names:?}");
        assert!(names.contains(&"f2.txt"), "d2 was suppressed: {names:?}");
    }

    #[test]
    fn classify_descent_skips_cycle_check_when_key_unavailable() {
        // Real regression guard for the cycle-key collision class: a `None` key
        // means identifiers are unavailable (the case the old Windows `(0, 0)`
        // sentinel mishandled), so cycle detection must be SKIPPED — descent
        // always proceeds and is never suppressed, even across repeated calls
        // that a shared sentinel would have made collide.
        let mut visited = HashSet::new();
        assert_eq!(
            classify_descent(None, &mut visited),
            DescentDecision::Descend(None)
        );
        assert_eq!(
            classify_descent(None, &mut visited),
            DescentDecision::Descend(None),
            "missing identifiers must never register as a cycle"
        );
        assert!(
            visited.is_empty(),
            "a None key must not be recorded in the visited set"
        );
    }

    #[test]
    fn classify_descent_flags_repeated_key_as_cycle() {
        // A stable key seen twice on the same descent path IS a cycle.
        let mut visited = HashSet::new();
        let key = (1, 42);
        assert_eq!(
            classify_descent(Some(key), &mut visited),
            DescentDecision::Descend(Some(key)),
            "first sighting of a key must descend and track it"
        );
        assert_eq!(
            classify_descent(Some(key), &mut visited),
            DescentDecision::Cycle,
            "second sighting of the same key on the path is a cycle"
        );
    }

    #[test]
    fn classify_descent_keeps_distinct_keys_independent() {
        // The collision the fix targets: distinct directories must keep distinct
        // keys and never suppress one another as a false cycle.
        let mut visited = HashSet::new();
        assert_eq!(
            classify_descent(Some((1, 1)), &mut visited),
            DescentDecision::Descend(Some((1, 1)))
        );
        assert_eq!(
            classify_descent(Some((1, 2)), &mut visited),
            DescentDecision::Descend(Some((1, 2))),
            "a distinct key must not be mistaken for a cycle"
        );
    }
}
