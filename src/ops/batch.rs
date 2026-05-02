use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::app::types::PendingAction;
use crate::ops::file_ops;

pub struct BatchReport {
    pub errors: Vec<String>,
    pub success_count: usize,
}

pub fn execute_batch(action: PendingAction) -> BatchReport {
    match action {
        PendingAction::Copy { sources, dest } => batch_copy(&sources, &dest),
        PendingAction::Move { sources, dest } => batch_move(&sources, &dest),
        PendingAction::Delete { paths } => batch_delete(&paths),
    }
}

fn batch_copy(paths: &[PathBuf], dest_dir: &Path) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut used_dests: HashSet<PathBuf> = HashSet::new();
    let mut success_count: usize = 0;

    for src in paths {
        let file_name = src.file_name().unwrap_or_default();
        let dest = dest_dir.join(file_name);
        if !used_dests.insert(dest.clone()) {
            errors.push(format!(
                "{}: duplicate destination {}",
                src.display(),
                dest.display()
            ));
            continue;
        }
        let result = match src.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => file_ops::copy_symlink(src, &dest),
            Ok(meta) if meta.is_dir() => file_ops::copy_dir_recursive(src, &dest).map(|_| ()),
            Ok(_) => file_ops::copy_file(src, &dest).map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            errors.push(format!("{}: {}", src.display(), e));
        } else {
            success_count += 1;
        }
    }

    BatchReport {
        errors,
        success_count,
    }
}

fn batch_move(paths: &[PathBuf], dest_dir: &Path) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut used_dests: HashSet<PathBuf> = HashSet::new();
    let mut success_count: usize = 0;

    for src in paths {
        let file_name = src.file_name().unwrap_or_default();
        let dest = dest_dir.join(file_name);
        if !used_dests.insert(dest.clone()) {
            errors.push(format!(
                "{}: duplicate destination {}",
                src.display(),
                dest.display()
            ));
            continue;
        }
        if let Err(e) = file_ops::move_entry(src, &dest) {
            errors.push(format!("{}: {}", src.display(), e));
        } else {
            success_count += 1;
        }
    }

    BatchReport {
        errors,
        success_count,
    }
}

fn batch_delete(paths: &[PathBuf]) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut success_count: usize = 0;

    for path in paths {
        let result = match path.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => file_ops::delete_file(path),
            Ok(meta) if meta.is_dir() => file_ops::delete_dir_recursive(path),
            Ok(_) => file_ops::delete_file(path),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            errors.push(format!("{}: {}", path.display(), e));
        } else {
            success_count += 1;
        }
    }

    BatchReport {
        errors,
        success_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::PendingAction;
    use std::fs;

    fn make_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn batch_copy_files_to_dest() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = make_file(src_dir.path(), "a.txt", b"hello");
        let f2 = make_file(src_dir.path(), "b.txt", b"world");

        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(dest_dir.path().join("a.txt").exists());
        assert!(dest_dir.path().join("b.txt").exists());
    }

    #[test]
    fn batch_copy_duplicate_dest_reports_error() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = make_file(src_dir.path(), "same.txt", b"a");
        let f2 = make_file(src_dir.path(), "same.txt", b"b");

        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("duplicate destination"));
    }

    #[test]
    fn batch_move_files_to_dest() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();

        let f1 = make_file(src_dir.path(), "x.txt", b"data");
        let f2 = make_file(src_dir.path(), "y.txt", b"more");

        let action = PendingAction::Move {
            sources: vec![f1.clone(), f2.clone()],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!f1.exists());
        assert!(!f2.exists());
        assert!(dest_dir.path().join("x.txt").exists());
        assert!(dest_dir.path().join("y.txt").exists());
    }

    #[test]
    fn batch_delete_files() {
        let dir = tempfile::tempdir().unwrap();

        let f1 = make_file(dir.path(), "del1.txt", b"a");
        let f2 = make_file(dir.path(), "del2.txt", b"b");

        let action = PendingAction::Delete {
            paths: vec![f1.clone(), f2.clone()],
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 2);
        assert!(report.errors.is_empty());
        assert!(!f1.exists());
        assert!(!f2.exists());
    }

    #[test]
    fn batch_delete_nonexistent_reports_error() {
        let action = PendingAction::Delete {
            paths: vec![PathBuf::from("/tmp/lc_nonexistent_test_file_xyz")],
        };

        let report = execute_batch(action);

        assert_eq!(report.success_count, 0);
        assert_eq!(report.errors.len(), 1);
    }
}
