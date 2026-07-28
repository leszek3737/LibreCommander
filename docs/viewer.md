# Viewer

The internal viewer (`F3`) is a read-only preview pane with several display
modes. It loads content in the background, so opening large files does not
freeze the UI.

## Opening

Press `F3` on a file. The viewer decides how to display it:

- **Text** — for files detected as text.
- **Hex** — for binary files (or toggle manually).
- **Image** — for image files, rendered as character art via `chafa`.
- **Archive listing** — for archives, shows the contained entries.

## Text mode

| Key | Action |
|-----|--------|
| `Up` / `Down` (`k` / `j`) | Scroll one line |
| `PageUp` / `PageDown` | Scroll a page |
| `Home` / `End` | Jump to start / end |
| `Left` / `Right` | Horizontal scroll (when wrap is off) |
| `l` | Toggle line numbers |
| `w` | Toggle line wrapping |
| `h` | Switch to hex mode |
| `/` | Search |
| `n` / `N` | Next / previous match |

## Hex mode

Hex mode shows raw bytes with an offset column and an ASCII gutter. Use the
same scroll keys. Search (`/`) accepts a hex query to find byte patterns.
Toggle back to text with `h`.

## Image mode

Images are rendered in the terminal via [`chafa`](https://hpjansson.org/chafa/)
(must be installed separately — see
[Getting Started](getting-started.md#optional-image-preview)). Without `chafa`
the viewer reports `Failed to execute chafa (is it installed?)`.

## Archive listing

Opening an archive shows its entries (names, sizes). From here you can inspect
contents without extracting; extraction is done from the panels with `F7`
(see [Archives](archives.md)).

## Searching in the viewer

`/` opens a search prompt. In text mode it matches text (case-insensitive);
in hex mode it matches byte patterns. `n` / `N` cycle through matches, which
are highlighted.

## Closing

`Esc`, `F3`, `F10`, or `q` close the viewer and return to the panels.

## Notes

- The viewer is read-only — editing is done via `F4`, which launches your
  external `$EDITOR`.
- Very large files are loaded with bounded memory; the viewer streams and
  caches render state rather than holding the whole file.
