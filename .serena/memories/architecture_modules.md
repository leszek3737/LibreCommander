# Module Architecture

Top-level layout under `src/`:

```
main.rs    # binary entry, run_app loop, key/event dispatch, render_ui, tests
lib.rs     # crate root, re-exports modules
menu.rs    # menu types & loading logic (.mc.menu, ~/.config/lc/menu)

app/       # core domain state (no I/O of files-on-disk except config)
  config.rs        # TOML config load/save (~/.config/lc/config.toml)
  types.rs         # AppState, PanelState, ActivePanel, DialogKind, etc.
  dir_tree.rs      # directory tree model for the side tree panel
  user_menu.rs     # user-menu data model
  keymap.rs        # keymap config + lookup
  shell.rs         # shell-out helpers (suspend/resume terminal, run cmd)
  paths.rs         # path helpers (home, expand, normalize)
  watcher_sync.rs  # bridge fs::watcher events → panel state refresh
  job_runner.rs    # background long-running jobs (copy/move/delete progress)
  file_type.rs     # classify entries (dir, file, symlink, exec, ...)
  mime.rs          # MIME detection (uses `infer` crate)
  debug_log.rs     # opt-in debug log file

fs/        # filesystem I/O
  reader  -- directory listing
  watcher -- notify-based fs watcher (macos_fsevent / inotify)

ops/       # operations performed on files
  file_ops.rs   # copy/move/delete/mkdir/rename/chmod
  search.rs     # find-in-files / find-by-name
  sorting.rs    # entry sorting (by name/size/mtime/...)
  batch.rs      # batched multi-file ops with progress
  compare.rs    # directory compare / sync mode
  helpers.rs    # shared op utilities
  chunk_copy.rs # chunked copy with progress callback

ui/        # Ratatui rendering only — no business logic
  panels.rs   # twin file panels
  dialogs.rs  # confirm / input / error / progress / properties / copymove / list-picker
  viewer.rs   # built-in file viewer
  theme.rs    # colors / styles
  dir_tree.rs # tree panel rendering
  menu.rs     # menu rendering

input/     # keyboard / mouse handling
  mouse.rs
  menu_actions.rs
```

## Boundaries / dependency direction

- `ui/` depends on `app/types` (reads state) but never mutates app state directly.
  Mutations happen in `main.rs` handlers in response to events.
- `ops/` is pure file-system work; it returns results / errors and does not know
  about UI. Long-running ops report progress via `job_runner`.
- `fs/watcher` produces events; `app/watcher_sync` translates them into refreshes.
- `app/types::AppState` is the single source of truth for UI rendering.

## Tests

Tests live in `src/main.rs` under `#[cfg(test)] mod tests` (around line 2074+).
There is a top-level `tests/` directory but it is currently empty.
