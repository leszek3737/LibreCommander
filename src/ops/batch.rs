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
