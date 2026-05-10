# LibreCommander — Full Project Audit

> Generated: 2026-05-10 | Scope: `src/**/*.rs`
> Severity: **HIGH** = data loss / crash / wrong behavior | **MEDIUM** = incorrect / incomplete | **LOW** = tech debt / style

---

## Summary

| Severity | Count |
|----------|-------|
| HIGH     | 17    |
| MEDIUM   | 58    |
| LOW      | 30+   |

Top-risk areas: **file operations** (delete before validate, cancel race), **UI underflow panics** (narrow terminals), **main.rs monolith** (~3700 LOC), **dead/wrong keymap**.

---

## HIGH Severity

### 1. `src/ops/file_ops.rs` — Data loss: delete before validate (L68-72, L90-92)

`prepare_dest(overwrite=true)` deletes target **before** `reject_same_file` / descendant check.
`copy_file(src, src, true)` **deletes the source file**.

**Fix:** Validate `src == dest`, hardlink/symlink same-file, and descendant check **before** any `prepare_dest`.

### 2. `src/ops/file_ops.rs` — Data loss: move_entry same issue (L322-328, L392-398)

`move_entry*` also calls `prepare_dest` before `path_contains`. Moving `dir → dir/sub` with overwrite deletes target in subtree, then returns error.

**Fix:** Move descendant validation before `prepare_dest`.

### 3. `src/ops/chunk_copy.rs` — Cancel race (L24-35, L89-95)

Cancel checked during copy but **not** before `publish_temp()`. Cancelled operation still overwrites destination via rename.

**Fix:** Check `cancel` immediately before `publish_temp()` and at start of each publish branch.

### 4. `src/ops/batch.rs` — Parent+child dedup missing (L525-587)

`batch_delete` doesn't deduplicate parent+child paths. Selecting a directory and its child: parent removes child, then child gives false `NotFound`.

**Fix:** Canonicalize paths, remove descendants of already-selected directories before loop.

### 5. `src/app/debug_log.rs` — Flaky tests (L99-124)

Tests share global `LOG_FILE`. Parallel `cargo test` causes mutex contention; `log_returns_when_mutex_contended` can fail non-deterministically.

**Fix:** Serialize tests or use test-specific temp files.

### 6. `src/app/dir_tree.rs` — Symlink-to-dir broken (L96)

`symlink_metadata()` returns `is_dir=false` for symlinks to directories. Tree cannot expand or enter symlinked dirs.

**Fix:** Use `entry.file_type()` or `metadata()` to follow symlinks.

### 7. `src/app/job_runner.rs` — Abandoned threads on Drop (L23-26)

`Drop` sets cancel but doesn't join the thread. Abandoned `JoinHandle` lets FS ops continue without UI.

**Fix:** Add explicit shutdown path with `join()`, or block exit until job cancelled.

### 8. `src/fs/cha.rs` — Non-Unix file_mode wrong (L16-19)

`file_mode()` on non-Unix always returns `0o100644`. Directories, symlinks misclassified as regular files.

**Fix:** Build type bits from `meta.file_type()` (`is_dir`, `is_file`, `is_symlink`) in `#[cfg(not(unix))]` branch.

### 9. `src/ops/natsort.rs` — Key disagrees with sort (L123-125)

`natsort_key` gives `pic2 < pic02` but `natsort` expects `pic02 < pic02000 < pic2`. Leading zero semantics inconsistent.

**Fix:** Unify: `natsort_key` must encode same rules as `natsort`, including `compare_left` for leading zeros.

### 10. `src/ui/dialogs.rs` — Underflow panic (L224, 229, 291, 296)

`inner.width as usize - 2` underflows on narrow terminal. Debug panic, release gives huge width.

**Fix:** `inner.width.saturating_sub(2) as usize`.

### 11. `src/ui/panels.rs` — Underflow panic (L90-99)

`scroll_offset > entries.len()` causes `end_idx - start_idx` underflow. Debug panic / wrong `take()` in release.

**Fix:** Clamp: `start_idx = panel.scroll_offset.min(panel.entries.len())`, use `saturating_sub`.

### 12. `src/ui/viewer.rs` — Wrap scroll broken (L494-537)

`wrap_lines=true` renders only `visible_height` logical lines. Long line fills screen, scroll jumps past wrapped content.

**Fix:** Model visual line scrolling based on width, or disable wrap during navigation.

### 13. `src/fs/watcher.rs` — Debounce loses deletions (L207-223)

Debounce suppresses all event types. Fast `Created → Deleted` in 300ms loses deletion, leaves stale panel entry.

**Fix:** Debounce only `Modified`. Emit `Deleted`/`Renamed` immediately.

### 14. `src/menu.rs` — Wrong action mapping (L34-42)

Options menu labels (Configuration/Layout/Panel options/Appearance) map to wrong actions (SaveHotlist/ToggleListing/TogglePanelHidden/ResetPanelFilter).

**Fix:** Map to correct actions or hide unimplemented items.

### 15. `src/input/menu_actions.rs` — Duplicate action (L60-66 vs 74-80)

`TogglePanelHidden` and `ToggleHiddenFiles` both toggle `show_hidden`. Only difference: second resets cursor/scroll.

**Fix:** Differentiate semantics or remove duplicate.

### 16. `src/app/keymap.rs` — Dead/wrong keymap

`KEYBINDINGS` uses string modes (`"Viewer"`) that don't match `AppMode` enum (`"Viewing"`). Missing `Dialog/Help/Error/Progress/OverwriteConfirm`. Constant is **dead code** in production — only used in tests and `build_help_message`. Double source of truth with `main.rs` match dispatch.

**Fix:** Type mode as `AppMode`, or remove table and generate help from dispatch.

### 17. `src/main.rs` — Multiple issues

| Issue | Lines | Description |
|-------|-------|-------------|
| Dead Enter pattern | L1298 | `KeyCode::Enter` in ALT branch unreachable |
| Async dialog stuck | L1625, L1671, L1990 | Async job confirmation leaves `Dialog(Confirm)` mode when `status_message` is `None` |
| Search wipe | L2283 | Esc with empty `unfiltered_entries` wipes file list |
| Resize ignored | L258 | `Event::Resize` sets no dirty flag |

---

## MEDIUM Severity (by file)

### `src/app/config.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L20-22, 60-64 | `PersistedPanel::default()` has `show_hidden=false`, `PanelState::new()` has `true` | Align defaults, add `#[serde(default = "default_true")]` |
| L100-102 | Empty saved `hotlist` not restored; default dir remains | Always assign `state.directory_hotlist = self.hotlist.clone()` |
| L119-122, 148-155 | `PathBuf` saved via `display().to_string()` loses non-UTF-8 paths | Validate with `to_str()`, skip non-encodable |
| L172-176 | `fs::write` is non-atomic; crash = empty config | Write to temp + `sync_all` + `rename` |

### `src/app/file_type.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L100-105 | `ends_with_ignore_ascii_case` unsafe for non-ASCII byte boundaries | Use `as_bytes().ends_with()` or `Path::extension` |
| L64-95 | Dotfiles like `.env.local`, `Dockerfile`, `Makefile` not recognized as config/source | Add exact-name and prefix lists |

### `src/app/mime.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L201, L116 | `rm`/`rmvb` → Archive not Video | Map MIME to `FileCategory::Video` |
| L289, L73 | `plist` → Other not Config | Add `application/x-plist => Config` |
| L179, L73 | `ai`/`eps` → Other not Image | Add `application/postscript => Image` or align with file_type.rs |
| L133-348 | Duplicate extension lists with file_type.rs, already diverged | Unify into single table |

### `src/app/shell.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L123 | No `TerminalRestoreGuard` before `suspend_terminal_stdout()` | Create guard before suspend |
| L90-92 | `Command::new("sh")` Unix-only | `cfg(unix)`/`cfg(windows)` with `SHELL`/`COMSPEC` |
| L127-142 | Exit message says `Ctrl+O`, code accepts Enter/Esc too | Unify text and behavior |

### `src/app/watcher_sync.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L42-53 | `last_synced` set even on watch failure; retry never happens | Set only on success |
| L120-160 | Events on panel directory itself ignored (requires parent match) | Handle `path == panel.path`: refresh or navigate to parent |

### `src/app/user_menu.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L142-145 | Unsupported condition silently passes (no restriction) | Return `Unsupported`, warn, skip entry |
| L230 | Empty regex after `+ f` matches everything | Reject empty pattern |

### `src/app/types.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L730-732 | `selected_entries()` misses unfiltered selection | Return from `unfiltered_entries` when available |
| L656-663 | Size sum can overflow | Use `saturating_add()` or `checked_add()` |

### `src/ops/batch.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L457-464 | Duplicate target: progress reports `completed++` without bytes | Add `bytes_done += sizes[idx]` or subtract from `bytes_total` |
| L527-598 | Directory delete shows no progress (size=0 from helpers) | Recursive size counting or callback progress |

### `src/ops/chunk_copy.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L78, L100-122 | Hardlink fallback loses permissions | Set permissions from source after fallback copy |
| L94-122 | Fallback copy has no progress reporting | Report bytes in fallback publish path |

### `src/ops/compare.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L40-73 | Works on filtered entries, not full panel | Compare `unfiltered_entries` or document limitation |
| L22-28 | `Thorough` compares `SystemTime` exactly; different FS resolutions | Add tolerance or document as strict |

### `src/ops/helpers.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L16-27 | `path_size()` returns 0 for directories | Recursive file size counting |
| L1-4 | `helpers` is `pub mod` exposing internal `action_label()` | `pub(crate) mod helpers` |

### `src/ops/search.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L205-455 | Content search is `#[cfg(test)]` only | Move to production or remove module description claim |
| L350-375 | Content search follows symlinks to files outside tree | Use `symlink_metadata` / `file_type.is_file()` |

### `src/ops/sorting.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L53-60 | Case-insensitive sort non-deterministic (no tiebreaker) | Add `(lowercase_name, original_name)` key |
| L120-131 | NaturalSort ASCII-only but Name sort uses Unicode | Document or unify to Unicode |

### `src/ops/file_ops.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L655-717 | Overwrite deletes dest before copy success; no rollback | Publish via rename of whole temp dir; use backup/staging |
| L158-439 | Ordinary and progress versions duplicate logic | Common helper with optional progress/cancel |

### `src/ops/mod.rs`

- All modules `pub mod` exposes internals. **Fix:** `pub(crate) mod` for helpers.

### `src/ops/natsort.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L136-157 | `parse_u64_digits` overflow → `u64::MAX` makes different large numbers equal | Store normalized digit string, compare len then lexicographic |

### `src/fs/path.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L36 | `expand_path()` trims legal leading/trailing spaces | Don't trim globally; handle empty in UI |
| L9-13 | `clean_path()` removes `..` lexically, ignores symlinks | Separate display cleanup from FS resolution |
| L45-53 | `~/$VAR` not expanded after tilde | Pass `rest` through `expand_env_vars()` |

### `src/fs/reader.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L107, L129 | `to_string_lossy()` loses non-UTF-8 filenames | Store `OsString`/`PathBuf`, separate display field |
| L78-82 | `..` navigation wrong for relative paths | Normalize to absolute or use `path.join("..")` + canonicalize |

### `src/fs/cha.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L205-217 | Symlink executable check uses link mode (0777), not target | Check target metadata for executability |
| L303-320 | Cache hit for symlink doesn't compare link metadata itself | Store and compare link mtime/ctime separately |

### `src/fs/watcher.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L179-181 | notify errors silently ignored | Log error, resync panel on critical failure |
| L124-138 | `unwatch` removes entry even on backend error | Remove only after success |
| L231-246 | Rename split mapped to Delete+Create, loses rename semantics | Pair `From`→`To` or remove `Renamed` from API |

### `src/ui/dir_tree.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L43-56 | `visible_height == 0` causes panic in slicing | Return early or render only help bar |
| L91-99 | Byte-based truncation of potentially Unicode help text | Use `unicode_width` / `char_indices` |

### `src/ui/menu.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L49-66 | No height clamping for dropdown | `max_visible = area.height.saturating_sub(dropdown_y + 1)` |
| L19-42 | `selected_item` not validated | `.min(items.len().saturating_sub(1))` |

### `src/ui/theme.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L14, L39, L88-92 | `Yellow on Cyan` poor contrast for search match | Use `fg=Black`/`bg=Yellow` or dark bg |
| L8-53 | Theme hardcoded, no config path | Load from config with `Default` fallback |

### `src/ui/viewer.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L157-196 | Hex mode searches text content, not bytes | Separate `search_hex` path or block search in Hex |
| L255-263 | Binary→Text toggle shows placeholder | Block toggle or decode lossy with warning |
| L167-184 | Unicode case expansion breaks highlight boundaries | Map boundaries to original char range, not byte-per-byte |

### `src/ui/panels.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L303-314 | Scrollbar thumb disappears when offset out of range | Clamp `scroll_offset` to `max_scroll` |

### `src/ui/mod.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L8-12, 24-28 | `LAYOUT_OVERHEAD_ROWS` docs contradict test | Unify: menu(1) + status(1) + command(1) + function(1) + borders(2) |

### `src/lib.rs`

- All modules exposed as public API. **Fix:** `pub(crate)` + selective `pub use`.

### `src/app/keymap.rs`

- `KEYBINDINGS` is dead production code; double source of truth with `main.rs` match.
- `find_duplicate_keys` test function is `pub(crate)` unnecessarily.
- Test `lines_are_not_empty` assertion is tautological (always true).

### `src/app/paths.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L68-71 | Temp file name collision risk across users/instances | Use UID/PID in name or `create_new` |
| L74-96 | XDG accepts relative paths (should be absolute only) | Check `path.is_absolute()` before accepting |

### `src/input/mouse.rs`

| Lines | Issue | Fix |
|-------|-------|-----|
| L263-291 | Click on panel border treated as entry click | Use inner area range only: `start+1..=end-1` |
| L332-334 | `ensure_cursor_visible` uses full panel height with borders | Use `panel_height.saturating_sub(2)` |
| L128-158 | Confirm dialog click accepts anywhere in row | Check `col >= dialog_left && col < dialog_left + width` |

---

## LOW Severity (cross-cutting patterns)

### Code Quality

- Many `pub` fields without invariant enforcement (`types.rs`, `cha.rs`, `config.rs`)
- Non-UTF-8 path handling gaps (`reader.rs`, `path.rs`, `paths.rs`)
- Test coverage gaps: natsort_key vs natsort agreement, Unicode edge cases, scroll edge cases, btime sort
- Emoji icon width terminal-dependent (`panels.rs`)
- Files approaching 800-line guideline: `panels.rs` (871), `main.rs` (~3700)
- `src/input/mouse.rs` L201-202: dropdown width via `s.len()` not `UnicodeWidthStr::width`


