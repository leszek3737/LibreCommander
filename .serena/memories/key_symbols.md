# Key Symbols & Entry Points

## Application entry
- `fn main` — `src/main.rs:85` — sets up terminal, calls `run_app`, restores on exit
- `fn run_app` — `src/main.rs:104` — the main event loop; reads input, dispatches
  to handler functions, calls `render_ui` each frame
- `fn render_ui` — `src/main.rs:448` — top-level frame renderer

## Central state
- `struct AppState` — defined in `src/app/types.rs` — single source of truth for UI
- `struct PanelState` — `src/app/types.rs` — per-panel state (entries, cursor,
  scroll, selection, search filter)
- `enum ActivePanel` — left/right/tree
- `enum DialogKind` (domain) — `src/app/types.rs` — and `ui::dialogs::DialogKind`
  (presentation), bridged by `to_ui_dialog` at `src/main.rs:608`

## Event-handler family (all in `main.rs`)
- `handle_dialog` — `1670` (dispatcher)
  - `handle_confirm_dialog` — `1382`
  - `handle_input_dialog` — `1433`
  - `handle_error_dialog` — `1640`
  - `handle_progress_dialog` — `1647`
  - `handle_properties_dialog` — `1657`
  - `handle_copymove_dialog` — `1664`
- `handle_directory_tree` — `706`
- `handle_viewer_mode` — `1242`
- `handle_command_line` — `1286`
- `handle_list_picker` — `1776`
- `handle_search_mode` — `1936`
- `handle_menu_mode` — `1999`

## Operations (the "model" for file work)
- `src/ops/file_ops.rs` — copy/move/delete/mkdir/rename/chmod
- `src/ops/chunk_copy.rs` — chunked copy with progress
- `src/ops/batch.rs` — batched multi-file work
- `src/ops/search.rs` — find-in-files / find-by-name
- `src/ops/compare.rs` — directory compare/sync
- `src/ops/sorting.rs` — entry sorting

## Background / async work
- `src/app/job_runner.rs` — `RunningJob` struct, progress reporting back to UI
- `src/fs/watcher` (notify) → `src/app/watcher_sync.rs` → panel refresh

## Config & menu
- `src/app/config.rs` — load/save TOML at `~/.config/lc/config.toml`
- `src/menu.rs` + `src/app/user_menu.rs` — user menu (`.mc.menu` or `~/.config/lc/menu`)
- `src/app/keymap.rs` — keymap configuration

## Refactor caution: high-fanout symbols
Before changing the signature of any of these, run
`find_referencing_symbols` first:
- `AppState`, `PanelState`, `DialogKind` — touched everywhere
- `run_app`, `render_ui`, `handle_dialog` — central control flow
- public API of `ops/file_ops.rs`
