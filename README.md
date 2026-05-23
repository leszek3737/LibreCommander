# Libre Commander (lc)

A fast, Rust-based file manager inspired by Midnight Commander.

## Features

- **Dual-panel interface** - Navigate and manage files in two panels side-by-side
- **Async file operations** - Copy, move, delete, rename, chmod files and directories with background progress and cancellation
- **Safe recursive copy/move/delete** - Handles directories with symlink preservation, no-clobber copy, cross-device fallback, partial-copy cleanup, and cancellation safeguards
- **Advanced search** - Incremental panel filter, recursive file search (glob patterns), content search (grep-like)
- **File viewer** - Built-in text viewer with search, hex dump, line numbers, and word wrap
- **Directory tree** - Interactive expandable directory tree view
- **Directory compare** - Compare panels by name, size, or modification time (3 modes)
- **Directory hotlist** - Bookmark directories for quick access via Alt+1 through Alt+9
- **Directory history** - Navigate back with Alt+Backspace
- **User menu** - Extensible menu system via `.mc.menu` or `~/.config/lc/menu` (MC-compatible)
- **Sorting** - 12 sort modes: by name (standard & natural), size, modification time, creation time, or extension (ascending/descending)
- **File watcher** - Automatic panel refresh on external filesystem changes while preserving filters, sorting, and selection
- **Panel views** - Long (detailed) and Brief (compact) listing modes
- **File type icons** - Emoji icons and color coding for archives, images, source code, audio, video, config files
- **Mouse support** - Single click to select, double click to open/view, click to switch panels
- **Keyboard-driven** - 45+ keyboard shortcuts for power users
- **Configurable** - Customizable settings stored in `~/.config/lc/config.toml`
- **System protection** - Refuses to delete critical system directories (`/`, `/etc`, `/usr`, etc.)

## Build Instructions

### Prerequisites

- Rust 1.95+ (edition 2024 support required)
- Cargo

### Building from Source

```bash
cd ~/git/LibreCommander
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
| `serde` 1.0 | Config serialization |
| `toml` 1 | Config file parsing |
| `chrono` 0.4 | Date/time formatting |
| `regex` 1.0 | User menu condition matching |
| `unicode-width` 0.2 | Unicode character width for alignment |
| `users` 0.11 | File owner/group lookup |
| `notify` 8 | Filesystem watcher for auto-refresh (platform-conditional: macOS uses `macos_fsevent`) |
| `bitflags` 2 | Bitflag types |
| `dirs` 6 | XDG/user directories |
| `rayon` 1 | Parallel iteration / background jobs |
| `infer` 0.19 | MIME type detection |
| `filetime` 0.2 | File modification time handling |
| `ansi-to-tui` 8 | Parse ANSI sequences for image viewing |

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
| `F6` | Move file(s) |
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
| `Esc` / `F3` / `F10` / `q` | Exit viewer |
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
| `Ctrl+A` | Move to line start |
| `Ctrl+E` | Move to line end |
| `Ctrl+W` | Delete word |
| `Ctrl+U` | Delete to line start |
| `Ctrl+C` | Cancel command line |

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

Note: `a` (add) and `d` (delete) work only in the Hotlist picker.

### Mouse

| Action | Effect |
|--------|--------|
| Left click on file | Select entry |
| Left double-click | Open directory or view file |
| Left click on panel | Switch active panel |
| Left drag in panel | Select range of entries |
| Middle click | Copy (F5 equivalent) |
| Right click | Cancel / close (Esc equivalent) |
| Scroll | Scroll panel cursor |
| Click function bar (bottom) | F1-F10 actions |

## Configuration

Configuration file location: `~/.config/lc/config.toml`

### Config Schema

```toml
active_panel = "left"  # "left" or "right"
dir_first = true       # directories before files in sort
sort_sensitive = false # case-sensitive name sorting

[left]
path = "/home/user"
show_hidden = true
show_permissions = false
listing_mode = "long"  # "long" or "brief"
sort_mode = "name_asc" # see sort modes below
filter = ""            # glob pattern, empty = no filter

[right]
path = "/home/user/projects"
show_hidden = true
show_permissions = false
listing_mode = "long"
sort_mode = "name_asc"
filter = ""

hotlist = ["/home/user", "/home/user/projects"]
```

### Sort Modes

`name_asc`, `name_desc`, `natural_name_asc`, `natural_name_desc`, `size_asc`, `size_desc`, `mod_time_asc`, `mod_time_desc`, `btime_asc`, `btime_desc`, `extension_asc`, `extension_desc`

An optional `[theme]` section is supported for color customization; all fields have defaults.

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `EDITOR` | External editor for F4 | `vi` |
| `HOME` | Config/menu file location | (required) |
| `XDG_CONFIG_HOME` | Config/menu file base directory | `$HOME/.config` |

## User Menu

Create custom menu entries in:
- Local: `.mc.menu` in the active panel's directory
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
- **Condition**: `+ f <regex>` — only show entry when filename matches regex; multiple condition lines are OR'd together. Conditions can appear before or after the hotkey line.

### Substitution Tokens

| Token | Expands to |
|-------|------------|
| `%f` | Current filename (shell-quoted) |
| `%d` | Active panel directory (shell-quoted) |
| `%D` | Other panel directory (shell-quoted) |
| `%t` / `%s` | Tagged/selected files (space-separated, shell-quoted); `%s` is an alias for `%t` |
| `%%` | Literal `%` |

Commands are executed via `sh -c` with the active panel's directory as working directory.

Menu files are limited to 1 MiB.

## File Operations

Long-running copy, move, and delete operations run as background jobs with live item and byte progress. Operations can be canceled between safe boundaries; move operations finish cleanup after a successful cross-device copy so source and destination do not diverge unexpectedly.

Safety guarantees:

- Existing destinations are not overwritten by chunked copies.
- Recursive directory copies publish through a temporary sibling and clean up partial output on failure or cancellation.
- Symlinks are copied or deleted as symlinks rather than following their targets.
- Cross-device moves fall back to copy-then-delete only after the copy succeeds.
- Critical system directories are protected from deletion.

## File Viewer

The built-in viewer (F3) supports:

- **Text mode** with word wrap (toggle with `w`)
- **Line numbers** (toggle with `l`)
- **Hex dump** (toggle with `h`) — standard hex+offset format, 16 bytes per line
- **Image preview** — automatic image preview rendering in character art using `chafa` (toggle with `h` to hex mode)
- **In-file search** (`/` to search, `n`/`N` to navigate matches)
- **Horizontal scrolling** for wide lines
- **Unicode support** — lossy UTF-8 display for binary files
- **Size limit** — files up to 100 MiB (larger files are truncated)
- **Content detection** — auto-detection of text vs binary content (MIME-based with null-byte fallback)

## Image Preview

lc renders images as character art (ANSI TrueColor) using **chafa**. Open any
image file with `F3` — the viewer auto-detects image MIME types and switches to
Image mode.

### Requirements

```bash
# macOS
brew install chafa

# Debian / Ubuntu
sudo apt install chafa

# Fedora
sudo dnf install chafa

# Arch
sudo pacman -S chafa
```

chafa is not bundled with lc. If missing, the viewer shows
"Failed to execute chafa (is it installed?)".

### Controls in Image mode

| Key | Action |
|-----|--------|
| `h` | Toggle between image preview and hex dump |
| `Up` / `Down` / `k` / `j` | No-op (image fills available area) |
| `Esc` / `F3` / `F10` / `q` | Close viewer |

### How it works

- On first view or terminal resize, lc spawns `chafa --size WxH <file>` and
  parses its ANSI output into terminal characters via `ansi-to-tui`.
- The result is cached — subsequent frames only clone the cached `Text`,
  keeping **60 FPS** rendering.
- Preview size adapts to the terminal area, leaving one line for the status bar.

## Search

### Incremental Search (Panel Filter)

Type any character in normal mode to start filtering. The panel updates in real-time. Supports glob patterns (`*`, `?`). Case-insensitive.

### File Search (Find File)

Menu: Command > Find file. Recursive glob-pattern search from the active panel's directory. First match is navigated to automatically.

### Content Search

Available programmatically via `FileSearch::search_content()`. Searches file contents line-by-line. Case-insensitive. Content search limits: files over 10 MiB skipped, lines over 64 KiB skipped, max 1000 results, max depth 20, max 10000 items scanned. Not yet wired to a UI action.

## Sorting

Twelve sort modes, cycled via menu (Left/Right > Sort order):

| Mode | Key | Order |
|------|-----|-------|
| Name ↑ | name_asc | A-Z |
| Name ↓ | name_desc | Z-A |
| Nat ↑ | natural_name_asc | A-Z (digit-aware) |
| Nat ↓ | natural_name_desc | Z-A (digit-aware) |
| Size ↑ | size_asc | Smallest first |
| Size ↓ | size_desc | Largest first |
| Time ↑ | mod_time_asc | Oldest first |
| Time ↓ | mod_time_desc | Newest first |
| Created ↑ | btime_asc | Oldest first |
| Created ↓ | btime_desc | Newest first |
| Ext ↑ | extension_asc | A-Z |
| Ext ↓ | extension_desc | Z-A |

Rules: `..` always first, directories before files, case-insensitive. These defaults are configurable via `dir_first` and `sort_sensitive` in `config.toml`. Natural sort compares multi-digit runs numerically (e.g. `file9` < `file10`).

## Directory Compare

Command menu > Compare dirs. Three modes:

| Mode | Matching criteria |
|------|-------------------|
| Quick | Filename + entry type |
| Size | Filename + size (dirs: name + type only) |
| Thorough | Filename + size + modification time (dirs: name + type only) |

Differing and unique entries are auto-selected in both panels.

## Testing

Run the test suite:

```bash
cargo test
```

The test suite covers:
- File operations (copy, move, delete, rename, chmod)
- Search (incremental, glob, content, symlink safety)
- Sorting (all 12 modes, edge cases)
- UI rendering (colors, icons, formatting, truncation)
- Config persistence (roundtrip serialization)
- User menu parsing and substitution
- Directory tree building and toggling
- Viewer (scroll, search, hex mode, Unicode)
- Batch operations (copy/move/delete with progress and cancellation)
- File watcher events and debouncing

## Quality Gates

Run these checks before submitting changes:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

File operations include safety guards: system directories are protected from deletion, symlinks are handled correctly during copy/move/delete, and terminal state is always restored (even on panic).

## License

MIT License

## Acknowledgments

Libre Commander is inspired by:
- [Midnight Commander](https://midnight-commander.org/) - The original dual-panel file manager
- [Yazi](https://github.com/sxyazi/yazi) - Some code components were adapted from this project by [Sxyazi](https://github.com/sxyazi) (MIT License)
- [Rust](https://www.rust-lang.org/) - The programming language
- [Ratatui](https://ratatui.rs/) - Terminal UI library
