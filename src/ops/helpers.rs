use std::path::Path;

pub(crate) fn path_contains_canonical(parent: &Path, child: &Path) -> bool {
    child != parent && child.starts_with(parent)
}
