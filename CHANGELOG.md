# Changelog

All notable changes to this project will be documented in this file.

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

## 0.0.7 — 2026-04-15

- Added crate package metadata for publishing and tooling.
- Added MIT license file matching the README license.
- Added this changelog.
