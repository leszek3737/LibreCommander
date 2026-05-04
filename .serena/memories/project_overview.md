# Libre Commander (lc)

**Purpose:** TUI file manager inspired by Midnight Commander. Single binary, no runtime deps.

**Tech stack:** Rust (edition 2024, MSRV 1.85), Ratatui + Crossterm for TUI.

**Key deps:** chrono, rayon, crossterm, ratatui, regex, serde/toml, unicode-width, users, notify, filetime, infer, tempfile (dev).

**Config:** `~/.config/lc/config.toml` | User menu: `.mc.menu` or `~/.config/lc/menu`

## Architecture

```
src/
  main.rs        # entry point, app loop, event handling, tests
  lib.rs         # module exports (app, ops, ui, fs, menu)
  app/           # core logic: config, types, dir_tree, user_menu, keymap, shell, paths, watcher_sync, job_runner, file_type, mime, debug_log
  fs/            # filesystem: reader, watcher
  ops/           # operations: file_ops, search, sorting, batch, compare, helpers, chunk_copy
  ui/            # rendering: panels, dialogs, viewer, theme, dir_tree, menu
  input/         # input handling: mouse, menu_actions
  menu.rs        # menu types/logic
```

## Binary
`target/release/lc` after `cargo build --release`
