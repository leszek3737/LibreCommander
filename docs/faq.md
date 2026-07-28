# FAQ & Troubleshooting

## General

**What is the binary called?**
The binary is `lc`. The crates.io package is `librecommander` (the name `lc`
was taken), so you install with `cargo install librecommander` and run `lc`.

**Which platforms are supported?**
Linux and macOS. Windows is not a target.

**Does it need internet?**
No. Libre Commander is offline by design and makes no network calls. Shell
commands you run are free to use the network, of course.

**Is it safe / does it use unsafe code?**
The crate is built with `forbid(unsafe_code)` — there is no `unsafe` anywhere.
Destructive operations require confirmation, and archive extraction is
hardened against path traversal and symlink abuse.

## Display problems

**Icons show as boxes / missing glyphs.**
Your terminal font lacks the glyphs. Set `icon_theme = "ascii"` in
`[theme]`, or install a [Nerd Font](https://www.nerdfonts.com/) and set
`icon_theme = "nerd_font"`. See [Configuration → Icon themes](configuration.md#icon-themes).

**Colors look wrong / no truecolor.**
Use the ANSI-safe classic palette: `[theme] preset = "classic"`. The default
`modern` preset assumes truecolor support.

**The screen looks garbled after a crash.**
The app installs an RAII terminal guard that restores the terminal on panic.
If your terminal is still garbled, run `reset`.

## Image preview

**Image view says `Failed to execute chafa (is it installed?)`.**
Image preview requires [`chafa`](https://hpjansson.org/chafa/), which is not
bundled. Install it (`brew install chafa`, `apt install chafa`, etc.). See
[Getting Started](getting-started.md#optional-image-preview).

## Editor

**`F4` opens the wrong editor / fails.**
`F4` launches `$EDITOR` (default `vi`). Export your preferred editor, e.g.
`export EDITOR=nano`, in your shell before starting `lc`.

## Archives

**I can't create a 7z archive.**
7z is read-only (list + extract). Creation is supported for ZIP, TAR, and the
compressed TAR variants (gz, bz2, xz, zst). See [Archives](archives.md).

**Extraction refused a file.**
Extraction rejects members whose paths would escape the destination
(zip-slip) and refuses to write through symlinks. This is intentional safety
behavior, not a bug.

## Config

**Where is my config?**
`~/.config/lc/config.toml` (or `$XDG_CONFIG_HOME/lc/config.toml`). See
[Configuration → Paths](configuration.md#paths).

**I edited config.toml and nothing changed / it reset.**
Invalid values fall back to defaults (and are logged to the debug log) rather
than aborting. Check your TOML syntax and value names against the
[reference](configuration.md#full-reference). Remember **Options → Save setup**
overwrites panel state from the running app.

**How do I reset to defaults?**
Delete (or move aside) `~/.config/lc/config.toml` and restart; a fresh default
config is created.

## Filesystem watching

**The panel doesn't refresh when I change files externally.**
Press `Ctrl+R` to force a refresh. Automatic watching uses the OS `notify`
backend, which differs between Linux and macOS; if a particular filesystem
does not emit events, manual refresh always works. The watcher is also paused
while a background job runs.

## Still stuck?

- Press `F1` for in-app help.
- Open an issue: https://github.com/leszek3737/LibreCommander/issues
