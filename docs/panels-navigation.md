# Panels & Navigation

The main screen is two independent file panels. Each panel keeps its own
directory, sort order, filter, hidden-file setting, and navigation history.

## Panel layout

Each panel shows, top to bottom:

- A **header** with the current directory path.
- The **file list**: name, and in long mode also size, modification time, and
  (optionally) permissions.
- A **status line** with summary info (file count, selected bytes, free space).

Around the panels:

- **Menu bar** at the top (`F9`).
- **Command line** above the function bar.
- **Function-key bar** at the bottom (`F1`–`F12` hints).

## Basic navigation

| Action | Keys |
|--------|------|
| Move up / down | `Up`/`Down` or `k`/`j` |
| Enter directory / open file | `Enter` or `l` |
| Parent directory | `Backspace` or `h` |
| First / last entry | `Home` / `End` |
| Page up / down | `PageUp` / `PageDown` |
| Switch panel | `Tab` |
| Swap panels | `Ctrl+U` |

## Listing modes

Each panel can show a **Long** or **Brief** listing:

- **Long** (default): one file per row with size and modification time.
- **Brief**: compact multi-column names only.

Change the listing mode from the panel options in the menu bar
(`Left` / `Right` menus). The choice is persisted per panel in the config.

## Sorting

Sort the active panel from the menu bar or cycle sort modes. Available sort
fields:

- Name
- Natural name (numbers sorted numerically: `file2` before `file10`)
- Extension
- Size
- Modification time
- Birth time (creation time, where the filesystem provides it)

Each field can be ascending or descending. Directories are kept together
according to the sort. Sort settings are persisted per panel.

## Filtering

`Ctrl+S` starts incremental search/filter: as you type, the panel narrows to
matching entries. `Enter` keeps the filter applied; `Esc` restores the full
listing. The filter string is persisted per panel.

## Hidden files

`Ctrl+H` toggles visibility of dot-files in the active panel. The setting is
persisted per panel (`show_hidden`, default `true`).

## Permissions column

Toggle a permissions column per panel from the panel options
(`show_permissions`, default `false`).

## Quick cd

`Alt+C` opens a prompt to change the active panel's directory directly.
Supports `~` expansion and environment-variable expansion.

## History

Each panel remembers visited directories. `Alt+Backspace` goes back to the
previous directory. The history list is also available as a picker from the
menu.

## Hotlist

`Alt+1` … `Alt+9` jump to bookmarked directories (hotlist). Manage hotlist
entries from the menu bar. Hotlist entries are persisted in the config.

## Directory tree

Open a navigable directory tree from the menu bar. The tree shows the
filesystem hierarchy; expand/collapse with `Left`/`Right`, move with
`Up`/`Down`, and press `Enter` to open a directory in the active panel.
`Esc` closes the tree. The tree is built in the background and reports
unreadable directories as non-fatal diagnostics rather than failing.

## Refresh & live watching

- `Ctrl+R` forces a manual refresh of the active panel.
- A built-in filesystem **watcher** refreshes panels automatically when files
  change on disk (create, modify, delete, rename). It preserves your filter,
  sort, and selection, and is paused during background jobs to avoid churn.
