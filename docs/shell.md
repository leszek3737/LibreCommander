# Shell & Command Line

Libre Commander integrates with your real shell without leaving the TUI
permanently. You can run one-off commands, keep a command history, and
temporarily drop to a normal terminal view.

## Opening the command line

- Press `Alt+X`, or
- Focus the command line at the bottom of the screen and start typing.

## Running a command

1. Type a shell command.
2. Press `Enter`.
3. The TUI is suspended (alternate screen left, raw mode disabled).
4. Your shell runs the command (`$SHELL -c …`, or a platform default).
5. After the command exits, press `Enter` to return to the TUI.
6. The active panel is refreshed so you see any files the command created or
   removed.

## Editing keys

| Key | Action |
|-----|--------|
| `Left` / `Right` / `Home` / `End` | Move cursor |
| `Backspace` | Delete character before cursor |
| `Ctrl+A` | Beginning of line |
| `Ctrl+E` | End of line |
| `Ctrl+U` | Clear from cursor to start of line |
| `Ctrl+W` | Delete word before cursor |
| `Up` / `Down` | History previous / next |
| `Esc` | Cancel without running |

## Command history

Commands you run are stored in a history list and are navigable with
`Up` / `Down` while editing. History is local to the session / stored with
app state (not shared with your interactive shell's `~/.bash_history`).

## External view (`Ctrl+O`)

`Ctrl+O` toggles **external view**: the TUI is temporarily suspended so you
can see normal terminal output (for example, the output of a previous
command, or a background process). Press `Ctrl+O` again (or the exit key
prompted) to restore the TUI. Both panels are refreshed on return.

## User-menu commands

Commands selected from the [User Menu](user-menu.md) (`F2`) also go through
the same shell runner: the TUI is suspended, the expanded command runs, and
the panel is refreshed afterwards.

## Environment

Commands inherit your environment, including `$EDITOR` (used by `F4`),
`$SHELL`, and `$PATH`. There is no network feature of Libre Commander itself
— the tool is offline by design — but shell commands you run are free to use
the network.
