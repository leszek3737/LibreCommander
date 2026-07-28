# File Operations

All destructive operations (delete, overwrite) ask for explicit confirmation.
Long-running operations run in the background with a progress dialog and can be
cancelled — the UI stays responsive throughout.

## The active set: selection vs. cursor

Operations act on the **selection** if any files are selected; otherwise they
act on the single file **under the cursor**.

- `Insert` — toggle selection on the current row and move down.
- `Shift+Up` / `Shift+Down` — extend the selection.
- Switching panels or navigating does not clear the selection.

## Copy (`F5`)

Copies the active set to a destination directory.

- Default destination is the **other panel's** current directory; you can edit
  the target path in the dialog.
- Runs as a background job with a progress bar (bytes and files), and can be
  cancelled.
- On a name collision you get an **overwrite confirmation** with per-file
  choices (overwrite / skip / apply-to-all).
- Cross-device copies use a buffered copy with a temporary file that is
  published atomically, so an interrupted copy does not leave a half-written
  file at the destination.
- Symlinks are copied as symlinks (not followed).

## Move / Rename (`F6`)

- Move the active set to a destination directory (default: other panel).
- Moving a single file to a new name in the same directory is a **rename**.
- Same-directory moves are atomic renames. Cross-device moves fall back to
  copy + delete, preserving cancellation and no-clobber semantics.

## Rename (`F11`)

Renames the file or directory under the cursor in place (pre-filled input
dialog).

## Delete (`F8`)

Deletes the active set after confirmation.

- Runs as a cancellable background job.
- Symlinks are removed as links; their targets are not touched.
- Non-empty directories are removed recursively (confirmation covers the whole
  set).

## Create directory (`F7`)

Creates a new directory in the active panel's current directory. If the file
under the cursor is an **archive**, `F7` instead offers extraction (see
[Archives](archives.md)).

## Properties (`Alt+Enter`)

Shows a properties dialog for the entry under the cursor: full path, size,
modification time, type/category, and permission bits.

## Chmod (Unix)

On Unix platforms you can change permission bits through the properties /
chmod flow. Symlinks are not followed during chmod.

## Directory compare

Compare the two panel directories and mark differing entries (files present on
one side only, or different size/time). Available from the menu bar
(`Command` menu) and via the compare-mode picker.

## Safety guarantees

- **Confirmation** is required for delete, move-overwrite, and other
  destructive actions.
- **Cancellation** is available for every long-running job.
- **Atomicity**: copies publish via a temp file; same-device moves are atomic
  renames.
- **No-clobber**: overwrite is never silent — you confirm per collision or
  apply a choice to all.
- **Symlinks are data**: copy/delete/chmod do not follow symlinks unless the
  operation explicitly requires it.
