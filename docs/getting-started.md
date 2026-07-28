# Getting Started

## Requirements

- A terminal emulator with truecolor support is recommended (any modern
  terminal works).
- Rust **1.95+** only if you build from source.
- No runtime dependencies. `lc` is a single static-ish binary and works fully
  offline.

### Optional: image preview

Image preview in the viewer shells out to
[`chafa`](https://hpjansson.org/chafa/), which is **not bundled**. Without it,
image view shows `Failed to execute chafa (is it installed?)`.

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

## Installation

### From crates.io

The binary is named `lc` (the package name `librecommander` is used because
`lc` is taken on crates.io):

```bash
cargo install librecommander
lc
```

### Prebuilt binaries

Download release binaries for Linux (x86_64 / aarch64) and macOS
(arm64 / x86_64) from
[GitHub Releases](https://github.com/leszek3737/LibreCommander/releases).

### From source

```bash
git clone https://github.com/leszek3737/LibreCommander.git
cd LibreCommander
cargo build --release
./target/release/lc

# or install onto PATH
cargo install --path .
```

If your toolchain is too old:

```bash
rustup update stable
```

## First run

```bash
lc
```

No CLI flags are required. On first run a default configuration is created at
`~/.config/lc/config.toml` (see [Configuration](configuration.md) for the full
reference and XDG overrides).

You will see two file panels side by side, a menu bar at the top, a status
line per panel, a command line, and a function-key bar at the bottom.

## 30-second tour

1. **Move** with arrow keys or `h j k l` (vi-style).
2. **Switch panels** with `Tab`.
3. **Enter** a directory with `Enter`; go up with `Backspace` or `h`.
4. **Select** files with `Insert` (or `Shift+Up/Down` to extend selection).
5. **Copy** the selection to the other panel with `F5`, **move** with `F6`.
6. **View** a file with `F3`; inside the viewer toggle hex with `h`, line
   numbers with `l`, wrap with `w`, search with `/`.
7. **Delete** with `F8` (you will be asked to confirm).
8. **Create a directory** with `F7`.
9. Open an archive (`.zip`, `.tar.gz`, …) with `Enter` to browse it, or press
   `F7` on it to extract.
10. **Quit** with `F10` or `q`.

Press **`F1`** any time for the built-in help, and see
[Keybindings](keybindings.md) for the complete reference.

## Where things live

| What | Where |
|------|-------|
| Config | `~/.config/lc/config.toml` |
| User menu | `~/.config/lc/menu` (or a project-local `.mc.menu`) |
| Terminal state cache | `~/.cache/lc/terminal_state` |

These follow the XDG base-directory spec; see
[Configuration → Paths](configuration.md#paths).

## Next steps

- Learn the keys: [Keybindings](keybindings.md)
- Manage files: [File Operations](file-operations.md)
- Configure themes and panels: [Configuration](configuration.md)
