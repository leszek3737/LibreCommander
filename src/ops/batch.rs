use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::types::PendingAction;
use crate::ops::file_ops;

pub struct BatchReport {
    pub errors: Vec<String>,
    pub success_count: usize,
    pub canceled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchProgress {
    pub completed: usize,
    pub total: usize,
    pub current: Option<PathBuf>,
}

impl BatchProgress {
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.completed as f32 / self.total as f32
        }
    }
}

pub fn execute_batch(action: PendingAction) -> BatchReport {
    execute_batch_with_progress(action, |_| {}, None)
}

pub fn execute_batch_with_progress(
    action: PendingAction,
    mut progress: impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    match action {
        PendingAction::Copy { sources, dest } => batch_copy(&sources, &dest, &mut progress, cancel),
        PendingAction::Move { sources, dest } => batch_move(&sources, &dest, &mut progress, cancel),
        PendingAction::Delete { paths } => batch_delete(&paths, &mut progress, cancel),
    }
}

fn is_canceled(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

fn report_progress(
    progress: &mut impl FnMut(BatchProgress),
    completed: usize,
    total: usize,
    current: Option<&Path>,
) {
    progress(BatchProgress {
        completed,
        total,
        current: current.map(Path::to_path_buf),
    });
}

fn batch_copy(
    paths: &[PathBuf],
    dest_dir: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut used_dests: HashSet<PathBuf> = HashSet::new();
    let mut success_count: usize = 0;
    let mut canceled = false;
    let total = paths.len();

    report_progress(progress, 0, total, paths.first().map(PathBuf::as_path));
    for (idx, src) in paths.iter().enumerate() {
        if is_canceled(&cancel) {
            canceled = true;
            break;
        }
        report_progress(progress, idx, total, Some(src));
        let file_name = src.file_name().unwrap_or_default();
        let dest = dest_dir.join(file_name);
        if !used_dests.insert(dest.clone()) {
            errors.push(format!(
                "{}: duplicate destination {}",
                src.display(),
                dest.display()
            ));
            report_progress(
                progress,
                idx + 1,
                total,
                paths.get(idx + 1).map(PathBuf::as_path),
            );
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
        report_progress(
            progress,
            idx + 1,
            total,
            paths.get(idx + 1).map(PathBuf::as_path),
        );
    }

    BatchReport {
        errors,
        success_count,
        canceled,
    }
}

fn batch_move(
    paths: &[PathBuf],
    dest_dir: &Path,
    progress: &mut impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut used_dests: HashSet<PathBuf> = HashSet::new();
    let mut success_count: usize = 0;
    let mut canceled = false;
    let total = paths.len();

    report_progress(progress, 0, total, paths.first().map(PathBuf::as_path));
    for (idx, src) in paths.iter().enumerate() {
        if is_canceled(&cancel) {
            canceled = true;
            break;
        }
        report_progress(progress, idx, total, Some(src));
        let file_name = src.file_name().unwrap_or_default();
        let dest = dest_dir.join(file_name);
        if !used_dests.insert(dest.clone()) {
            errors.push(format!(
                "{}: duplicate destination {}",
                src.display(),
                dest.display()
            ));
            report_progress(
                progress,
                idx + 1,
                total,
                paths.get(idx + 1).map(PathBuf::as_path),
            );
            continue;
        }
        if let Err(e) = file_ops::move_entry(src, &dest) {
            errors.push(format!("{}: {}", src.display(), e));
        } else {
            success_count += 1;
        }
        report_progress(
            progress,
            idx + 1,
            total,
            paths.get(idx + 1).map(PathBuf::as_path),
        );
    }

    BatchReport {
        errors,
        success_count,
        canceled,
    }
}

fn batch_delete(
    paths: &[PathBuf],
    progress: &mut impl FnMut(BatchProgress),
    cancel: Option<Arc<AtomicBool>>,
) -> BatchReport {
    let mut errors: Vec<String> = Vec::new();
    let mut success_count: usize = 0;
    let mut canceled = false;
    let total = paths.len();

    report_progress(progress, 0, total, paths.first().map(PathBuf::as_path));
    for (idx, path) in paths.iter().enumerate() {
        if is_canceled(&cancel) {
            canceled = true;
            break;
        }
        report_progress(progress, idx, total, Some(path));
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
        report_progress(
            progress,
            idx + 1,
            total,
            paths.get(idx + 1).map(PathBuf::as_path),
        );
    }

    BatchReport {
        errors,
        success_count,
        canceled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::PendingAction;
    use std::fs;
    use std::sync::atomic::Ordering;

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
        assert!(!report.canceled);
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
        assert!(!report.canceled);
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
        assert!(!report.canceled);
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
        assert!(!report.canceled);
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
        assert!(!report.canceled);
    }

    #[test]
    fn batch_delete_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = make_file(dir.path(), "one.txt", b"1");
        let f2 = make_file(dir.path(), "two.txt", b"2");
        let action = PendingAction::Delete {
            paths: vec![f1, f2],
        };
        let mut updates = Vec::new();

        let report = execute_batch_with_progress(action, |progress| updates.push(progress), None);

        assert_eq!(report.success_count, 2);
        assert!(!report.canceled);
        assert_eq!(
            updates.first().map(|p| (p.completed, p.total)),
            Some((0, 2))
        );
        assert_eq!(updates.last().map(|p| (p.completed, p.total)), Some((2, 2)));
        assert_eq!(updates.last().map(BatchProgress::percent), Some(1.0));
    }

    #[test]
    fn batch_copy_cancel_stops_between_items() {
        let src_dir = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let f1 = make_file(src_dir.path(), "first.txt", b"1");
        let f2 = make_file(src_dir.path(), "second.txt", b"2");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_progress = Arc::clone(&cancel);
        let action = PendingAction::Copy {
            sources: vec![f1, f2],
            dest: dest_dir.path().to_path_buf(),
        };

        let report = execute_batch_with_progress(
            action,
            |progress| {
                if progress.completed == 1 {
                    cancel_for_progress.store(true, Ordering::Relaxed);
                }
            },
            Some(cancel),
        );

        assert_eq!(report.success_count, 1);
        assert!(report.canceled);
        assert!(dest_dir.path().join("first.txt").exists());
        assert!(!dest_dir.path().join("second.txt").exists());
    }
}
