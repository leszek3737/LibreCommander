# Changelog

All notable changes to this project will be documented in this file.

## 0.0.9 — 2026-05-03

- Added asynchronous file operations with background job execution, live progress reporting, and cancellation support.
- Added filesystem watcher integration so panels refresh when files change outside Libre Commander.
- Added chunked file copying with no-clobber publishing, progress updates, metadata preservation, and safer fallback behavior on filesystems without hardlink support.
- Improved batch copy/move/delete progress accounting without pre-scanning large directory trees.
- Hardened recursive copy/move/delete: symlink-aware handling, cross-device move fallback, partial-copy cleanup, system directory protection, and safer cancellation boundaries.
- Expanded MIME and file-category detection for archives, documents, media, config files, source code, and viewer open-mode decisions.
- Added configurable keymap/help generation, user menu workflows, directory hotlist/history pickers, and external shell view handling.
- Added directory tree UI, menu rendering, theming, brief/long panel views, file icons, and richer panel status summaries.
- Improved built-in viewer with MIME-aware text/binary detection, hex mode, line numbers, word wrap, horizontal scrolling, and in-file search.
- Fixed UI regressions in search confirmation, info dialogs, watcher refresh, UTF-8 path truncation, and help/list picker rendering.

## 0.0.7 — 2026-05-02

- Quality gates enforced: fmt, clippy, test all pass before merge
- File operation safety: system directory protection, symlink-aware copy/move/delete
- Terminal recovery: panic-safe restore on exit via Drop guard
- Performance: lazy panel loading, fast path for same-device renames
- Typed actions: PendingAction enum drives copy/move/delete confirmation flow
- Search diagnostics: content search, incremental filter with glob patterns
- Viewer hardening: hex mode, word wrap, line numbers, in-file search, Unicode
- UI small terminal: graceful degradation under 80x24, minimum size guard
- Architecture extraction: ops module split into compare, batch, search, sorting, file_ops

## 0.0.6 — 2026-04-15

- Added crate package metadata for publishing and tooling.
- Added MIT license file matching the README license.
- Added this changelog.
