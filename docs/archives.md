# Archives

Libre Commander can browse, extract, and create archives without leaving the
TUI. Extraction is safety-hardened against path traversal (zip-slip),
symlink abuse, and runaway sizes.

## Supported formats

| Format | Browse | Extract | Create |
|--------|:------:|:-------:|:------:|
| ZIP | Yes | Yes | Yes |
| TAR | Yes | Yes | Yes |
| TAR.GZ | Yes | Yes | Yes |
| TAR.BZ2 | Yes | Yes | Yes |
| TAR.XZ | Yes | Yes | Yes |
| TAR.ZST | Yes | Yes | Yes |
| 7z | Yes | Yes | **No** |

Format is detected from the file extension and content.

## Browsing

Press `Enter` on an archive file to open it as a virtual directory listing.
You can navigate entries and preview members with `F3`. Leave the archive
with `Backspace` / `h` like a normal directory.

## Extracting

- Press `F7` with the cursor on an archive (or choose extract from the archive
  menu, `F12`).
- Choose the destination directory (default: other panel, or current panel).
- Extraction runs as a background job with progress and cancellation.

### Safety model

Extraction enforces several hard rules:

- **Path traversal** is rejected (zip-slip): member paths that would escape
  the destination directory are refused.
- **Symlinks** are handled carefully — writes through symlinks are refused
  where possible.
- **Size limits** bound the total extracted data so a malicious archive
  cannot fill the disk.
- On failure, partially extracted data is cleaned up.

## Creating

Open the archive operations menu with `F12` (or the menu bar) and choose
**Create**. Select the format and the members to include (the current
selection, or the file under the cursor). ZIP creation uses Deflate
compression (level 6 by default). 7z creation is not supported.

## Archive menu (`F12`)

A dedicated picker for archive actions (list / extract / create and format
selection) so you can operate on archives without relying on the `F7`
shortcut alone.
