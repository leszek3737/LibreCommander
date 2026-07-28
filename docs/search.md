# Search

Libre Commander has two search modes: **file-name search** and **file-content
search**. Both walk the filesystem from a chosen root with shared safety
limits (depth, item count, cancellation, cycle detection via visited inodes).

## Starting a search

Search is available from the menu bar (`Command` menu) and related dialogs.
You choose:

- The **root directory** (default: active panel's current directory).
- The **pattern**.
- Whether to match by **name** or by **content**.

Results appear in a results list; you can jump to a match in a panel.

## File-name search

Matches file and directory names against a pattern.

Patterns support:

- **Substring** matching.
- **Wildcards** (`*`, `?`) compiled into an internal pattern matcher.

Matching is case-insensitive where appropriate for the platform.

## File-content search

Scans file contents line-by-line for a pattern. Binary files are skipped by
heuristics. Matches report the file path and the matching line.

## Limits and cancellation

To keep the UI responsive and avoid runaway walks:

- Maximum **depth** and **item count** caps stop unbounded trees.
- Directory **cycles** (via symlinks) are detected through visited-inode
  tracking and skipped.
- Search is a background job and can be **cancelled** at any time.
- When a limit is hit, results are returned with a truncation reason rather
  than failing silently.

## Incremental filter vs. search

Do not confuse search with the panel's **incremental filter** (`Ctrl+S`):

| | Incremental filter (`Ctrl+S`) | Search |
|---|---|---|
| Scope | Current panel listing only | Recursive walk from a root |
| Purpose | Jump / narrow the visible list | Find files by name or content |
| Result | Filtered panel view | Results list with jump-to |

## Sorting

Independent of search, every panel listing can be sorted by name, natural
name, extension, size, modification time, or birth time — ascending or
descending. See [Panels & Navigation → Sorting](panels-navigation.md#sorting).
