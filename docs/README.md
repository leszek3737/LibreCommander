# Libre Commander Documentation

User documentation for **Libre Commander** (`lc`) — a keyboard-first,
dual-panel terminal file manager written in Rust. A modern, offline,
single-binary take on the Norton Commander / Midnight Commander workflow.

## Read this first

| Document | What you get |
|----------|--------------|
| [Getting Started](getting-started.md) | Install, first run, optional dependencies, 30-second tour |
| [Keybindings](keybindings.md) | Every key, grouped by mode (panels, viewer, dialogs, command line, menu, tree) |
| [File Operations](file-operations.md) | Copy, move, delete, rename, mkdir, chmod, properties, selection |
| [Panels & Navigation](panels-navigation.md) | Panel layout, listing modes, sorting, filtering, hidden files, hotlist, history, directory tree |
| [Viewer](viewer.md) | Internal viewer: text, hex, image preview, archive listing, search |
| [Archives](archives.md) | Supported formats, browsing, extraction, creation, safety model |
| [Search](search.md) | File-name search, content search, patterns, limits |
| [Shell & Command Line](shell.md) | Built-in command line, external view, command history |
| [User Menu](user-menu.md) | `.mc.menu`-compatible user menu, substitutions, conditions |
| [Configuration](configuration.md) | `config.toml` reference, themes, paths, XDG locations |
| [FAQ & Troubleshooting](faq.md) | Common problems and answers |

## At a glance

- Two synchronized file panels with independent sort, filter, and history.
- Classic function-key workflow (`F3` view, `F4` edit, `F5` copy, `F6` move,
  `F7` mkdir, `F8` delete, `F10` quit) plus vi-style navigation.
- Background file operations with progress and cancellation — the UI never freezes.
- Live filesystem watching: panels refresh when files change on disk.
- Built-in viewer with text, hex, image (via `chafa`), and archive-listing modes.
- Archive support: ZIP, TAR, TAR.GZ/BZ2/XZ/ZST (read/write) and 7z (read).
- File-name and file-content search with wildcards and safety limits.
- Shell command line and MC-compatible user menu (`.mc.menu`).
- Single offline binary. No async runtime. `forbid(unsafe_code)`.

## Project facts

| | |
|---|---|
| Binary name | `lc` |
| crates.io package | `librecommander` |
| Platforms | Linux, macOS |
| MSRV | Rust 1.95 (edition 2024) |
| License | MIT |
| Repository | https://github.com/leszek3737/LibreCommander |

> In-app help is always available: press **`F1`** at any time.
