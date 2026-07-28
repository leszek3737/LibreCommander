# Configuration

Configuration is a TOML file at `~/.config/lc/config.toml`. It is created
with defaults on first run and is updated when you save setup from the menu
bar (**Options → Save setup**). You can also hand-edit it — invalid values
fall back to defaults rather than crashing.

## Paths

Locations follow the XDG base-directory specification:

| What | Path |
|------|------|
| Config | `$XDG_CONFIG_HOME/lc/config.toml` (default `~/.config/lc/config.toml`) |
| User menu | `$XDG_CONFIG_HOME/lc/menu` (default `~/.config/lc/menu`) |
| Terminal state cache | `$XDG_CACHE_HOME/lc/terminal_state` (default `~/.cache/lc/terminal_state`) |

`XDG_CONFIG_HOME` / `XDG_CACHE_HOME` are honored only when set to an absolute
path; otherwise the `~/.config` / `~/.cache` defaults are used.

## Environment variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `EDITOR` | External editor launched by `F4` | `vi` |
| `SHELL` | Shell used to run commands | platform default |
| `HOME` | Base for config / menu location | required |
| `XDG_CONFIG_HOME` | Config base directory | `$HOME/.config` |
| `XDG_CACHE_HOME` | Cache base directory | `$HOME/.cache` |

## Full reference

```toml
# Which panel is active on startup: "left" or "right".
active_panel = "left"

# Keep directories before files in sorted listings.
dir_first = true

# Case-sensitive sorting (false = case-insensitive).
sensitive = false

# Bookmarked directories for Alt+1..Alt+9.
hotlist = ["/home/user/projects", "/tmp"]

[left]
path = "/home/user"          # starting directory (optional)
listing_mode = "long"        # "long" or "brief"
sort_mode = "name_asc"       # see sort modes below
filter = ""                  # persisted panel filter
show_hidden = true           # show dot-files
show_permissions = false     # permissions column

[right]
path = "/tmp"
listing_mode = "long"
sort_mode = "name_asc"
filter = ""
show_hidden = true
show_permissions = false

[theme]
preset = "modern"            # "modern" (default) or "classic"
icon_theme = "emoji"         # "emoji", "ascii", or "nerd_font"
# Individual color overrides (optional), e.g.:
# panel_bg = "#16161e"
# highlight_bg = "cyan"
```

### Sort modes

Valid values for `sort_mode`:

```
name_asc        name_desc
natural_name_asc  natural_name_desc
extension_asc   extension_desc
size_asc        size_desc
mod_time_asc    mod_time_desc
btime_asc       btime_desc
```

Natural sort treats digit runs numerically (`file9` sorts before `file10`).
`..` always sorts first; directories sort before files when `dir_first` is
true. Sorting is case-insensitive unless `sensitive = true`.

### Listing modes

- `long` — one file per row with size and modification time (default).
- `brief` — compact multi-column names.

## Theming

Two built-in presets:

- **`modern`** (default) — dark truecolor palette.
- **`classic`** — the original navy Norton/MC palette, ANSI-safe for
  terminals without truecolor.

Borders are rounded in both. Individual colors override the chosen preset.
Color values accept named colors (`red`, `navy`), hex (`#RRGGBB` or `#RGB`),
or ANSI indices (`0`–`255`).

Available color keys:

```toml
[theme]
preset       = "classic"
icon_theme   = "emoji"
panel_bg     = "navy"
panel_fg     = "white"
status_bar_bg = "navy"
status_bar_fg = "white"
menu_bar_bg  = "navy"
menu_bar_fg  = "white"
dialog_bg    = "black"
dialog_fg    = "white"
highlight_bg = "cyan"
highlight_fg = "black"
border_active = "yellow"
border_inactive = "dark_gray"
title        = "light_cyan"
error        = "red"
warning      = "yellow"
info         = "cyan"
selected_file_fg = "light_yellow"
scrollbar_active = "yellow"
scrollbar_inactive = "dark_gray"
function_bar_fg    = "light_blue"   # F-key number
function_bar_label = "light_blue"   # F-key label
function_bar_bg    = "dark_gray"
search_match_fg = "black"
search_match_bg = "light_green"
directory    = "white"
executable   = "green"
symlink      = "cyan"
archive      = "red"
image        = "magenta"
video        = "light_magenta"
audio        = "light_green"
source_code  = "yellow"
config       = "light_blue"
regular_file = "white"
```

### Icon themes

| Value | Notes |
|-------|-------|
| `emoji` | Default. Works in most modern terminals. |
| `ascii` | Plain ASCII glyphs — use if your font lacks emoji/nerd glyphs. |
| `nerd_font` | Requires a [Nerd Font](https://www.nerdfonts.com/) in your terminal. |

If icons render as missing-glyph boxes, switch to `ascii` or install a Nerd
Font and set `nerd_font`.

## Saving setup

**Options → Save setup** in the menu bar writes the current panel state,
sorting, filters, hidden-file toggles, and hotlist back to `config.toml`.
Theme colors are read from the `[theme]` table at startup.

## Robustness

- Missing fields use defaults.
- Invalid enum values (sort mode, listing mode, icon theme) fall back to
  their default and are logged to the debug log rather than aborting.
- Paths in the config are normalized on load.
