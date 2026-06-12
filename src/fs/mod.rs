//! Filesystem operations for Libre Commander.
//!
//! This crate module provides all low-level filesystem access needed by the
//! TUI panels and background jobs:
//!
//! - **Directory reading** ([`reader`]) — parallel directory listings via
//!   **rayon** with uid/gid name resolution and sorted [`FileEntry`] results.
//! - **Filesystem watching** ([`watcher`]) — real-time change notifications
//!   powered by the **notify** crate (debounced, cross-platform, with macOS
//!   `FSEvents` / Linux `inotify` backends).
//! - **Path utilities** ([`path`]) — tilde and environment-variable expansion,
//!   `.`/`..` normalization, and path-component helpers.
//! - **Cha metadata** ([`cha`]) — compact file attribute struct (permissions,
//!   size, timestamps, symlink target) abstracting Unix vs non-Unix `Metadata`.

pub mod cha;
pub mod path;
pub mod reader;
pub mod watcher;
