use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub name: String,
}

/// Build a flat tree listing from `root`, expanding directories up to `initial_depth` levels.
/// Returns the list sorted: directories first (alphabetically), then files (alphabetically),
/// within each parent directory.
pub fn build_tree(root: &Path, initial_depth: usize, show_hidden: bool) -> Vec<TreeEntry> {
    let mut entries = Vec::new();
    build_tree_recursive(root, 0, initial_depth, show_hidden, &mut entries);
    entries
}

fn build_tree_recursive(
    dir: &Path,
    current_depth: usize,
    max_expand_depth: usize,
    show_hidden: bool,
    out: &mut Vec<TreeEntry>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    let mut children: Vec<TreeEntry> = Vec::new();
    for entry in read_dir {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let is_dir = path.symlink_metadata().map(|m| m.is_dir()).unwrap_or(false);
        let expanded = is_dir && current_depth < max_expand_depth;

        children.push(TreeEntry {
            path,
            depth: current_depth,
            is_dir,
            expanded,
            name,
        });
    }

    // Sort: directories first (alphabetically), then files (alphabetically)
    children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    for child in children {
        let should_recurse = child.is_dir && child.expanded;
        out.push(child);
        let inserted_idx = out.len() - 1;

        if should_recurse {
            let child_path = out[inserted_idx].path.clone();
            build_tree_recursive(
                &child_path,
                current_depth + 1,
                max_expand_depth,
                show_hidden,
                out,
            );
        }
    }
}

/// Toggle expansion of the directory at `index` in `entries`.
/// If expanding: reads children and inserts them after the entry.
/// If collapsing: removes all descendants (entries at greater depth until we return
/// to the same or lesser depth).
pub fn toggle_expand(entries: &mut Vec<TreeEntry>, index: usize, root: &Path, show_hidden: bool) {
    let entry = match entries.get(index) {
        Some(e) => e.clone(),
        None => return,
    };

    if !entry.is_dir {
        return;
    }

    if entry.expanded {
        // Collapse: remove all descendants
        let depth = entry.depth;
        let mut end = index + 1;
        while end < entries.len() && entries[end].depth > depth {
            end += 1;
        }
        entries.drain(index + 1..end);
        entries[index].expanded = false;
    } else {
        // Expand: read children and insert
        let mut children = Vec::new();
        build_tree_recursive(
            &entry.path,
            entry.depth + 1,
            entry.depth + 1,
            show_hidden,
            &mut children,
        );
        // build_tree_recursive sets expanded=false since depth+1 > max_expand_depth (depth+1).
        // We want children dirs to show as collapsed (expanded=false), which is correct.
        // But build_tree_recursive uses max_expand_depth = depth+1, meaning dirs at depth+1
        // will have expanded = is_dir && (depth+1 < depth+1) = false. Correct.
        entries[index].expanded = true;
        entries.splice(index + 1..index + 1, children);
    }
    let _ = root; // root not needed for toggle, kept for API consistency
}

#[cfg(test)]
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

        toggle_expand(&mut entries, sub1_idx, dir.path(), false);

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

        toggle_expand(&mut entries, sub1_idx, dir.path(), false);

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
        toggle_expand(&mut entries, a_idx, dir.path(), false);
        assert_eq!(entries.len(), len_before);
    }

    #[test]
    fn toggle_expand_out_of_bounds_is_noop() {
        let dir = setup_test_dir();
        let mut entries = build_tree(dir.path(), 0, false);
        let len_before = entries.len();
        toggle_expand(&mut entries, 9999, dir.path(), false);
        assert_eq!(entries.len(), len_before);
    }
}
