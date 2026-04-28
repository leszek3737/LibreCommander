# Libre Commander (lc)

A fast, Rust-based file manager inspired by Midnight Commander.

## Features

- **Dual-panel interface** - Navigate and manage files in two panels side-by-side
- **Fast file operations** - Copy, move, delete, and rename files with ease
- **Advanced search** - Full file and content search capabilities
- **File viewer** - Built-in viewer for quick file inspection
- **Keyboard-driven** - Efficient keyboard shortcuts for power users
- **Configurable** - Customizable settings stored in `~/.config/lc/config.toml`
- **User menu** - Extensible menu system via `.mc.menu` or `~/.config/lc/menu`
- **Terminal-friendly** - Works in any terminal with full TUI support

## Build Instructions

### Prerequisites

- Rust 1.85+ (edition 2024 support required)
- Cargo

### Building from Source

```bash
cd ~/git/lc
cargo build --release
```

The binary will be located at `target/release/lc`.

### Running

```bash
./target/release/lc
```

Or install system-wide:

```bash
cargo install --path .
```

## Keyboard Shortcuts

### General

| Key | Action |
|-----|--------|
| `F1` | Help dialog |
| `F2` | User menu |
| `F10` / `q` | Quit |

### Navigation

| Key | Action |
|-----|--------|
| `Tab` | Switch between panels |
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `Enter` | Open directory / Execute file |
| `Backspace` | Go to parent directory |
| `Alt+Backspace` | Go to previous directory (history) |
| `Home` | Go to first entry |
| `End` | Go to last entry |
| `PageUp` | Page up |
| `PageDown` | Page down |

### File Operations

| Key | Action |
|-----|--------|
| `F3` | View file |
| `F4` | Edit file (opens in `$EDITOR`) |
| `F5` | Copy file(s) |
| `F6` | Move/Rename file(s) |
| `F7` | Create directory |
| `F8` | Delete file(s) |
| `Alt+Enter` | Show file properties |
| `Insert` | Toggle file selection |
| `Ctrl+R` | Refresh current panel |
| `Ctrl+O` | External viewer (temporarily exit to shell) |

### Search & Filter

| Key | Action |
|-----|--------|
| Type any key | Incremental search (filter files) |
| `Esc` | Cancel search |
| `Enter` | Confirm search |

### Bookmarks & History

| Key | Action |
|-----|--------|
| `Alt+1` through `Alt+9` | Jump to directory hotlist slot 1-9 |
| `Ctrl+U` | Swap panels |
| `Mouse Click` | Select file / Switch panel |
| `Mouse Double-Click` | Open directory / View file |

## Configuration

Configuration file location: `~/.config/lc/config.toml`

The config file stores:
- Panel paths and view modes
- Sort preferences
- Filter settings
- Hotlist entries

## User Menu

Create custom menu entries in:
- Local: `.mc.menu` in the current directory
- Global: `~/.config/lc/menu`

Menu format follows Midnight Commander conventions.

## License

MIT License

## Acknowledgments

Libre Commander is inspired by:
- [Midnight Commander](https://midnight-commander.org/) - The original dual-panel file manager
- [Rust](https://www.rust-lang.org/) - The programming language
- [Ratatui](https://ratatui.rs/) - Terminal UI library
