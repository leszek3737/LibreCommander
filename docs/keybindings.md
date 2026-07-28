# Keybindings

Libre Commander is modal. The active key set depends on what is on screen:
the normal panel view, the internal viewer, the command line, incremental
search, the menu bar, a dialog, or the directory tree.

> The authoritative, always-current list is built into the app: press **`F1`**
> for the help dialog. The tables below mirror that list.

## Normal mode (panels)

### Function keys

| Key | Action |
|-----|--------|
| `F1` | Show help dialog |
| `F2` | Open user menu |
| `F3` | View file in internal viewer |
| `F4` | Edit file in external editor (`$EDITOR`) |
| `F5` | Copy selected files |
| `F6` | Move / rename selected files |
| `F7` | Create directory, or extract archive under cursor |
| `F8` | Delete selected files |
| `F9` | Open the menu bar |
| `F10` | Quit the application |
| `F11` | Rename file or directory |
| `F12` | Archive operations menu |

### Navigation

| Key | Action |
|-----|--------|
| `Up` / `k` | Move cursor up |
| `Down` / `j` | Move cursor down |
| `Home` | Go to first entry |
| `End` | Go to last entry |
| `PageUp` | Page up |
| `PageDown` | Page down |
| `Enter` / `l` | Open directory (or file / archive under cursor) |
| `Backspace` / `h` | Go to parent directory |
| `Tab` | Switch active panel |
| `Ctrl+U` | Swap left and right panels |
| `Alt+Backspace` | Previous directory in history |
| `Alt+C` | Quick change directory (cd prompt) |
| `Alt+1` … `Alt+9` | Jump to a hotlist directory |

### Selection

| Key | Action |
|-----|--------|
| `Insert` | Toggle selection and move down |
| `Shift+Up` | Extend selection upward |
| `Shift+Down` | Extend selection downward |

> If no files are selected, operations like copy/move/delete act on the file
> under the cursor.

### View & panel options

| Key | Action |
|-----|--------|
| `Alt+Enter` | Show file properties |
| `Ctrl+H` | Toggle hidden files visibility |
| `Ctrl+R` | Refresh panel contents |
| `Ctrl+S` | Start incremental search / filter |
| `Ctrl+O` | Toggle external view (temporarily show normal terminal output) |
| `Alt+X` | Open the command line |

## Viewer mode

Open with `F3` on a file.

| Key | Action |
|-----|--------|
| `Up` / `k` | Scroll up one line |
| `Down` / `j` | Scroll down one line |
| `Left` | Scroll left |
| `Right` | Scroll right |
| `PageUp` | Page up |
| `PageDown` | Page down |
| `Home` | Go to beginning of file |
| `End` | Go to end of file |
| `l` | Toggle line numbers |
| `w` | Toggle line wrapping |
| `h` | Toggle hex mode |
| `/` | Open search dialog |
| `n` | Next search match |
| `N` | Previous search match |
| `Esc` / `F3` / `F10` / `q` | Close viewer |

Image preview mode (files detected as images, requires `chafa`) uses the same
scroll keys; text/hex toggles apply to text content.

## Command line mode

Open with `Alt+X`, or type a command in the bottom command line.

| Key | Action |
|-----|--------|
| `Enter` | Execute shell command |
| `Esc` | Cancel command line |
| `Backspace` | Delete character before cursor |
| `Up` / `Down` | Previous / next command in history |
| `Ctrl+A` | Move cursor to beginning of line |
| `Ctrl+E` | Move cursor to end of line |
| `Ctrl+U` | Clear line before cursor |
| `Ctrl+W` | Delete word before cursor |

## Incremental search / filter mode

Open with `Ctrl+S`.

| Key | Action |
|-----|--------|
| typing | Filter / jump to matching entries |
| `Enter` | Accept current search filter |
| `Esc` | Cancel search and restore |
| `Backspace` | Delete character before cursor |

## Menu bar mode

Open with `F9`.

| Key | Action |
|-----|--------|
| `Left` / `Right` | Move between top-level menus |
| `Up` / `Down` | Move within a menu |
| `Enter` | Activate item |
| `Esc` / `F9` / `F10` | Close menu |

## Dialogs

Confirmation dialogs:

| Key | Action |
|-----|--------|
| `y` / `Y` / `Enter` | Confirm (Yes) |
| `n` / `N` / `Esc` | Cancel (No) |

Text-input dialogs (rename, mkdir, quick cd, …):

| Key | Action |
|-----|--------|
| `Enter` | Accept |
| `Esc` | Cancel |
| `Backspace` | Delete character before cursor |
| `Left` / `Right` / `Home` / `End` | Move cursor |

List pickers (hotlist, history, compare mode, user menu, archive menu):

| Key | Action |
|-----|--------|
| `Up` / `Down` / `k` / `j` | Move |
| `Home` / `End` / `PageUp` / `PageDown` | Jump / page |
| `Enter` | Select |
| `Esc` | Cancel |

## Directory tree

| Key | Action |
|-----|--------|
| `Up` / `Down` | Move |
| `Left` / `Right` | Collapse / expand directory |
| `Enter` | Open selected directory in panel |
| `Esc` | Close tree |

## Mouse

Mouse input is supported: click to move the cursor and select entries, click
panel headers and the function bar, scroll with the wheel, and click dialog
buttons and menu items. All mouse actions map to the same operations as their
keyboard equivalents.
