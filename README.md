# Libre Commander (lc)

A fast, Rust-based file manager inspired by Midnight Commander.

## Features

- **Dual-panel interface** - Navigate and manage files in two panels side-by-side
- **Fast file operations** - Copy, move, delete, rename, chmod files and directories
- **Recursive copy/move** - Handles directories with symlink preservation and cross-device fallback
- **Advanced search** - Incremental panel filter, recursive file search (glob patterns), content search (grep-like)
- **File viewer** - Built-in text viewer with search, hex dump, line numbers, and word wrap
- **Directory tree** - Interactive expandable directory tree view
- **Directory compare** - Compare panels by name, size, or modification time (3 modes)
- **Directory hotlist** - Bookmark directories for quick access via Alt+1 through Alt+9
- **Directory history** - Navigate back with Alt+Backspace
- **User menu** - Extensible menu system via `.mc.menu` or `~/.config/lc/menu` (MC-compatible)
- **Sorting** - 8 sort modes: by name, size, modification time, or extension (ascending/descending)
- **Panel views** - Long (detailed) and Brief (compact) listing modes
- **File type icons** - Emoji icons and color coding for archives, images, source code, audio, video, config files
- **Mouse support** - Single click to select, double click to open/view, click to switch panels
- **Keyboard-driven** - 70+ keyboard shortcuts for power users
- **Configurable** - Customizable settings stored in `~/.config/lc/config.toml`
- **System protection** - Refuses to delete critical system directories (`/`, `/etc`, `/usr`, etc.)

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

### Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` 0.30 | Terminal UI framework |
| `crossterm` 0.29 | Cross-platform terminal I/O |
| `tokio` 1.51 | Async runtime |
| `serde` 1.0 | Config serialization |
| `toml` 0.9 | Config file parsing |
| `chrono` 0.4 | Date/time formatting |
| `regex` 1.0 | User menu condition matching |
| `unicode-width` 0.2 | Unicode character width for alignment |
| `users` 0.11 | File owner/group lookup |

Dev dependency: `tempfile` 3 (for tests).

## Keyboard Shortcuts

### General

| Key | Action |
|-----|--------|
| `F1` | Help dialog |
| `F2` | User menu |
| `F9` | Menu bar |
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
| `Alt+C` | Quick cd dialog (enter path directly) |

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
| `Shift+↑` | Extend selection upward |
| `Shift+↓` | Extend selection downward |
| `Ctrl+R` | Refresh current panel |
| `Ctrl+O` | External viewer (temporarily exit to shell) |

### Search & Filter

| Key | Action |
|-----|--------|
| Type any key | Incremental search (filter files) |
| `Ctrl+S` | Enter search mode |
| `Esc` | Cancel search / clear filter |
| `Enter` | Confirm search |

### Panel & View

| Key | Action |
|-----|--------|
| `Ctrl+U` | Swap panels |
| `Ctrl+H` | Toggle hidden files |

### Bookmarks & History

| Key | Action |
|-----|--------|
| `Alt+1` through `Alt+9` | Jump to directory hotlist slot 1-9 |
| `Mouse Click` | Select file / Switch panel |
| `Mouse Double-Click` | Open directory / View file |

### File Viewer Mode

| Key | Action |
|-----|--------|
| `Esc` / `F3` / `q` | Exit viewer |
| `↑` / `k` | Scroll up |
| `↓` / `j` | Scroll down |
| `PageUp` / `PageDown` | Page up/down |
| `Home` / `End` | Go to top/bottom |
| `Left` / `Right` | Horizontal scroll |
| `l` | Toggle line numbers |
| `w` | Toggle word wrap |
| `h` | Toggle hex mode |
| `/` | Search in file |
| `n` / `N` | Next / previous search match |

### Directory Tree Mode

| Key | Action |
|-----|--------|
| `Esc` | Exit tree |
| `↑` / `↓` / `Home` / `End` / `PageUp` / `PageDown` | Navigate |
| `Enter` | Expand/collapse directory or view file |
| `c` | cd to selected directory |

### Command Line Mode

| Key | Action |
|-----|--------|
| `Esc` | Cancel command line |
| `Enter` | Execute shell command |
| `↑` / `↓` | Browse command history |
| `Backspace` | Delete character |

### Menu Bar (F9)

| Key | Action |
|-----|--------|
| `←` / `→` | Switch menu category |
| `↑` / `↓` | Navigate items |
| `Enter` | Execute action |
| `Esc` / `F9` | Close menu |

### List Picker (History, Hotlist, User Menu)

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate |
| `Enter` | Select / execute |
| `Esc` | Close |
| `a` | Add to hotlist (hotlist picker only) |
| `d` | Delete from hotlist (hotlist picker only) |

## Configuration

Configuration file location: `~/.config/lc/config.toml`

### Config Schema

```toml
version = 1
active_panel = "left"  # "left" or "right"

[left]
path = "/home/user"
show_hidden = true
listing_mode = "long"  # "long" or "brief"
sort_mode = "name_asc" # see sort modes below
filter = ""            # glob pattern, empty = no filter

[right]
path = "/home/user/projects"
show_hidden = true
listing_mode = "long"
sort_mode = "name_asc"
filter = ""

hotlist = ["/home/user", "/home/user/projects"]
```

### Sort Modes

`name_asc`, `name_desc`, `size_asc`, `size_desc`, `mod_time_asc`, `mod_time_desc`, `extension_asc`, `extension_desc`

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `EDITOR` | External editor for F4 | `vi` |
| `HOME` | Config/menu file location | (required) |

## User Menu

Create custom menu entries in:
- Local: `.mc.menu` in the current directory
- Global: `~/.config/lc/menu`

### Menu Format

```
# Comment line

+ f \.rs$
T  Run Rust tests
	cargo test %f

+ f \.py$
R  Run Python script
	python3 %f

A  Archive selected files
	tar czf archive.tgz %t

D  Diff panels
	diff -rq %d %D
```

### Entry Structure

- **Hotkey**: First character of the line (single char)
- **Title**: Rest of the hotkey line (display label)
- **Body**: Indented lines (tab or space) as shell commands
- **Condition**: `+ f <regex>` — only show entry when filename matches regex

### Substitution Tokens

| Token | Expands to |
|-------|------------|
| `%f` | Current filename (shell-quoted) |
| `%d` | Active panel directory (shell-quoted) |
| `%D` | Other panel directory (shell-quoted) |
| `%t` / `%s` | Tagged/selected files (space-separated, shell-quoted) |
| `%%` | Literal `%` |

Commands are executed via `sh -c` with the active panel's directory as working directory.

## File Viewer

The built-in viewer (F3) supports:

- **Text mode** with word wrap (toggle with `w`)
- **Line numbers** (toggle with `l`)
- **Hex dump** (toggle with `h`) — standard hex+offset format, 16 bytes per line
- **In-file search** (`/` to search, `n`/`N` to navigate matches)
- **Horizontal scrolling** for wide lines
- **Unicode support** — lossy UTF-8 display for binary files

## Search

### Incremental Search (Panel Filter)

Type any character in normal mode to start filtering. The panel updates in real-time. Supports glob patterns (`*`, `?`). Case-insensitive.

### File Search (Find File)

Menu: Command > Find file. Recursive glob-pattern search from the active panel's directory. First match is navigated to automatically.

### Content Search

Available programmatically via `FileSearch::search_content()`. Searches file contents line-by-line. Case-insensitive. Not yet wired to a UI action.

## Sorting

Eight sort modes, cycled via menu (Left/Right > Sort order):

| Mode | Key | Order |
|------|-----|-------|
| Name ↑ | name_asc | A-Z |
| Name ↓ | name_desc | Z-A |
| Size ↑ | size_asc | Smallest first |
| Size ↓ | size_desc | Largest first |
| Time ↑ | mod_time_asc | Oldest first |
| Time ↓ | mod_time_desc | Newest first |
| Ext ↑ | extension_asc | A-Z |
| Ext ↓ | extension_desc | Z-A |

Rules: `..` always first, directories before files, case-insensitive.

## Directory Compare

Command menu > Compare dirs. Three modes:

| Mode | Matching criteria |
|------|-------------------|
| Quick | Filename only |
| Size | Filename + size |
| Thorough | Filename + size + modification time |

Differing and unique entries are auto-selected in both panels.

## Testing

Run the test suite:

```bash
cargo test
```

The test suite covers:
- File operations (copy, move, delete, rename, chmod)
- Search (incremental, glob, content, symlink safety)
- Sorting (all 8 modes, edge cases)
- UI rendering (colors, icons, formatting, truncation)
- Config persistence (roundtrip serialization)
- User menu parsing and substitution
- Directory tree building and toggling
- Viewer (scroll, search, hex mode, Unicode)

## Quality Gates

Run these checks before submitting changes:

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
```

File operations include safety guards: system directories are protected from deletion, symlinks are handled correctly during copy/move/delete, and terminal state is always restored (even on panic).

## License

MIT License

## Acknowledgments

Libre Commander is inspired by:
- [Midnight Commander](https://midnight-commander.org/) - The original dual-panel file manager
- [Rust](https://www.rust-lang.org/) - The programming language
- [Ratatui](https://ratatui.rs/) - Terminal UI library
