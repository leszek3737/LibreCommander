# User Menu

The user menu (`F2`) is an MC-compatible, user-defined command menu. It lets
you attach your own shell commands (with file substitutions) to menu entries,
optionally filtered by filename conditions.

## Menu file locations

Libre Commander looks for a menu file in this order:

1. A project-local `.mc.menu` in the active panel's current directory.
2. The global user menu at `~/.config/lc/menu`.

The first one found is used.

## File format

The format follows the Midnight Commander user-menu conventions:

- An entry starts with a **single letter or digit** followed by whitespace
  and the entry **label**.
- The lines after the label (indented) are the **shell command** to run.
- A **condition line** begins with `+ ` and restricts when the entry is shown.

Example:

```
g       Compress to tar.gz
        tar -czf %f.tar.gz %f

+ f \.txt$
v       View with less
        less %f
```

Entries whose condition does not match the current file are filtered out of
the picker.

## Conditions

Condition lines have the form:

```
+ f <regex>
```

- `f <regex>` — show the entry only when the current filename matches the
  regular expression. Multiple `+ f` conditions on one entry are combined
  (OR).
- Unrecognized condition types are treated as unsupported and the entry is
  hidden rather than shown incorrectly.

## Substitutions

Inside a command, `%` placeholders are expanded and **shell-quoted** for
safety:

| Placeholder | Expands to |
|-------------|------------|
| `%f` | Current file name (under cursor) |
| `%d` | Active panel's current directory |
| `%D` | Other panel's current directory |
| `%t` or `%s` | Tagged (selected) files, space-separated; falls back to the current file if none are tagged |
| `%%` | A literal `%` |

All expansions are quoted, so filenames with spaces or special characters are
safe. An unknown `%x` sequence is left as-is.

## Running an entry

1. Press `F2`.
2. Pick an entry from the list (filtered by conditions).
3. The command is expanded, then run through the same shell runner as the
   command line: the TUI is suspended, the command runs, and the panel is
   refreshed on return. See [Shell & Command Line](shell.md).
