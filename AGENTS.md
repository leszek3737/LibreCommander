# Libre Commander (lc) — Agent Instructions

Rust TUI file manager inspired by Midnight Commander. Single Rust binary, with
optional external tools for enhanced features (for example `chafa` for image
previews). Stack: **Ratatui 0.30 + crossterm 0.29**, edition 2024, MSRV 1.95,
`unsafe_code = forbid`.

> If you have access to **Serena** or **GitNexus** MCP tools, jump to those
> sections — they are the preferred navigation path. Everything above them is
> for agents without those tools.

## Quick Orientation

- Binary crate name: `lc` (see `Cargo.toml`). Repo dir is `LibreCommander`.
- Entry: [src/main.rs](src/main.rs) — **~3000 lines**, holds the event loop,
  app state and most action handlers. Don't read it linearly; use
  `cargo doc --open` or grep for the symbol you need.
- Library facade: [src/lib.rs](src/lib.rs) re-exports `app`, `ops`, `ui`.
- Config file: `~/.config/lc/config.toml`
- User menu: `.mc.menu` (cwd) or `~/.config/lc/menu`

### Module map

```
src/
  main.rs              # event loop, App struct, high-level action handlers
  render.rs            # render orchestration and terminal draw entry points
  lib.rs               # public module exports
  menu.rs              # user menu loader (.mc.menu format)
  input/               # key/mouse handling and mode dispatch
    mod.rs             # input facade
    normal.rs          # normal-mode key handling
    mode_dispatch.rs   # routes input by current Mode
    dialogs.rs         # dialog input handling
    menu_actions.rs    # F9 menu actions
    command_line.rs    # shell command line input
    directory_tree.rs  # directory tree input
    pickers.rs         # list picker input
    mouse.rs           # mouse events
  app/
    config.rs          # config.toml parsing (serde + toml)
    types/             # core enums/structs (Pane, Mode, Selection, ...)
      app_state.rs, panel.rs, file_entry.rs, modes.rs, dialogs.rs, sorting.rs
    keymap.rs          # key bindings
    dir_tree.rs        # directory tree model
    user_menu.rs       # F2 menu state
    paths.rs           # XDG paths, tilde expansion
    shell.rs           # shell escape / exec
    job_runner.rs      # background work via rayon
    watcher_sync.rs    # debounced fs notifications
    file_type.rs, mime.rs, debug_log.rs
  fs/
    reader.rs          # async-style dir reads (rayon)
    watcher.rs         # `notify` crate wrapper
    path.rs            # path helpers
    cha.rs             # image terminal capability helpers
  ops/
    file_ops/          # copy / move / delete / mkdir / rename / chmod
      copy.rs, delete.rs, move_ops.rs, entry_ops.rs, common.rs, temp.rs
    chunk_copy.rs      # buffered copy with progress
    batch.rs           # multi-file pipelines
    search/            # name + content search (regex)
      model.rs, name.rs, content.rs, pattern.rs, walk.rs
    sorting.rs         # column sorters
    natsort.rs         # natural sort helpers
    compare.rs, helpers.rs
  ui/
    mod.rs             # top-level draw()
    panels/            # left/right file panels
    dialogs/           # modal dialogs (copy, delete, mkdir, ...)
    viewer/            # F3 internal viewer, hex/text/image rendering
      open.rs, loader.rs, search.rs, hex.rs, render.rs, scroll.rs, mime.rs
    dir_tree.rs, menu.rs, theme.rs
```

## Build, Run, Test

```bash
cargo run                          # dev build & run
cargo build --release              # binary at target/release/lc
cargo test                         # unit + integration tests
cargo test -- --nocapture          # with println! visible
cargo clippy --locked --all-targets -- -D warnings   # CI-equivalent lint
cargo fmt                          # format (run after every edit)
cargo fmt --check                  # CI check
```

For code changes, run the CI-equivalent local sequence before declaring the task
done:

```bash
cargo fmt
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

For docs-only changes, inspect `git diff`; no Rust build is required. Don't skip
clippy for code — the project pins `print_stdout`, `print_stderr`, `dbg_macro`
as **deny** and `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`,
`too_many_lines`, `cognitive_complexity`, `needless_pass_by_value`,
`redundant_clone`, and `inefficient_to_string` as **warn** (see `Cargo.toml`).

## Code Style & Conventions

- **No `unsafe`** — `unsafe_code = "forbid"` at crate level. Don't try.
- **No `println!` / `eprintln!` / `dbg!`** in committed code. For debug output
  use `app::debug_log` (writes to a file when enabled).
- **Avoid `.unwrap()` / `.expect()`** in non-test code unless the invariant is
  obvious and documented inline. Prefer `?`, `let ... else`, or
  `Result`-returning helpers.
- **No `#[allow(...)]` or `#[expect(...)]` to suppress lints** — fix the
  underlying issue instead. The only exceptions are:
  - `#[allow(clippy::unwrap_used / expect_used / panic)]` on `mod tests` blocks
    (test code idiomatically uses `.unwrap()` / `.expect()`).
  - `#[allow(clippy::print_stdout)]` when the TUI is explicitly suspended and
    stdout is the intended output channel (e.g., shell command prompt).
  - `#[allow(non_snake_case)]` when a name deliberately mirrors an external
    token (e.g., `%D` menu substitution).
  - If you believe a new exception is warranted, ask the user first and
    document the reason in a comment on the annotation.
- **Functions over 100 lines trigger `too_many_lines`** — split them. The same
  applies to deeply nested logic (`cognitive_complexity`).
- **Ratatui = immediate mode.** The render path must be a pure function of
  `App` state; do not mutate state from inside `ui::*` draw code. Keep state in
  `App` (in `main.rs`) and let event handlers be the only mutators.
- **Event loop** uses blocking `crossterm::event::read()` interleaved with
  `event::poll(timeout)` for periodic redraws/animations. Don't introduce
  tokio — this crate is intentionally sync; offload heavy work via `rayon` or
  `app::job_runner`.
- **Filesystem ops** must be cancellable and report progress through the
  existing job/watcher channels. Look at `ops::chunk_copy` as the reference
  pattern before writing new long-running operations.
- **Errors:** prefer `std::io::Result` and `anyhow`-free explicit error enums
  where they already exist. Don't add new dependencies casually.
- **Unicode:** filenames may contain anything. Use `unicode-width` for column
  math (already a dep); never assume `len() == display width`.
- **Cross-platform:** the `notify` backend differs on macOS vs others (see
  `Cargo.toml` `[target.'cfg(...)']`). Test path/permission logic mentally for
  both before committing.

## TUI Invariants

- **Render is pure.** `ui::*` and `render.rs` must draw from `App` state and not
  mutate application state. Event handlers own mutation.
- **Input stays responsive.** Do not run heavy filesystem work, recursive walks,
  external commands, or image generation directly in key/mouse handlers or render
  paths. Use existing background job patterns (`rayon`, `app::job_runner`,
  `ViewerLoader`, `ImagePreviewLoader`).
- **Terminal state must recover.** Any path that suspends the TUI, enters raw
  mode, or executes a shell/editor/viewer must restore terminal state on success
  and error.
- **Panel state is user state.** Refreshes should preserve selection, cursor,
  filters, sorting, hidden-file mode, and active pane unless the user action
  explicitly changes them.

## File Size Policy

Prefer small, focused files — but **not at the cost of idiomatic Rust**. The
800-line mark is a *checkpoint*, not a hard limit:

- If a file you are editing (or just created) exceeds **800 lines**, stop and
  evaluate whether it can be split along a natural seam: distinct concerns,
  separable sub-modules, independent state machines, UI vs. logic, etc.
- If a clean split exists, propose it to the user before doing it (a sketch of
  the new module layout is enough). Don't silently fan out into many tiny
  files mid-task.
- If the file is large but **cohesive** — one big `match` dispatcher, a single
  state machine, one struct's `impl` block, or generated code — leave it.
  Forcing a split there hurts readability. Say so explicitly.
- Never split by line count alone. Splits driven by "this file feels long" with
  no semantic boundary produce worse code than the original.
- `src/main.rs` is currently ~3000 lines and is a known target for gradual
  extraction. Opportunistic, well-scoped extractions are welcome there; whole-
  scale rewrites are not.

## Editing Workflow

1. Locate the symbol (Serena / GitNexus / `rg`) — don't read `main.rs` whole.
2. Read just that symbol + its direct callers.
3. Make the smallest possible change. No drive-by refactors.
4. If the file you touched crosses 800 lines, apply the **File Size Policy**
   above before finishing.
5. For code, run `cargo fmt && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked && cargo build --release --locked`.
6. If you touched a public API in `lib.rs` re-exports, grep for external uses.
7. Update `CHANGELOG.md` only if the user asks or you are preparing a release.

## Commits & PRs

- Recent style (see `git log`): Conventional Commits, e.g. `fix: ...`,
  `feat(viewer): ...`, `refactor: ...`. Match it.
- One logical change per commit. Don't bundle formatting churn with logic.
- Don't bump the version in `Cargo.toml` unless explicitly asked.
- Never commit `target/`, editor swap files, or worktree dirs.
- **Never amend existing commits** (`git commit --amend`) unless the user
  explicitly asks for it. Always create a new commit for each logical change.

## Safety Rails

- Filesystem actions on user data are destructive. Dry-run mentally before
  introducing new delete/move/overwrite paths; ensure confirmation dialogs are
  wired in `ui::dialogs`.
- Delete/move/overwrite flows must have an explicit confirmation path unless the
  user already confirmed the exact operation in the current UI flow.
- Symlinks are data. Do not follow them during chmod/copy/delete unless the
  operation explicitly says so and tests cover it.
- Cross-device moves need copy+delete fallback with cancellation, partial-copy
  cleanup, and no-clobber behavior preserved.
- Long-running file operations must report progress and support cancellation via
  the existing job/watcher channels.
- Don't add network calls — this is an offline tool by design.
- Don't add a runtime config migration without the user's say-so; users have
  hand-edited `config.toml` files.

## Serena — Semantic Code Navigation

This project is configured with the **Serena MCP** server for symbolic, LSP-backed
navigation of Rust code. It is the preferred way to read and analyse code here,
because `src/main.rs` is **~3000 lines** — reading it linearly is wasteful.

### Always Do

- **Use `mcp__serena__get_symbols_overview`** on a file before reading it whole.
- **Use `mcp__serena__find_symbol`** (with `relative_path`) to load a single symbol
  body instead of an entire file. Pass `include_body: true` only when you need the
  implementation; otherwise just inspect the signature.
- **Use `mcp__serena__find_referencing_symbols`** before renaming, removing or
  changing the signature of any public function, struct, enum or method — Rust's
  call graph is wide and silent breakage is easy.
- **Read memories on demand** via `mcp__serena__read_memory`. Available memories:
  `project_overview`, `architecture_modules`, `main_rs_navigation`, `key_symbols`,
  `code_style`, `testing_patterns`, `task_completion`, `suggested_commands`.
- **Use `mcp__serena__search_for_pattern`** for regex searches scoped to source —
  it respects `ignored_paths` (target/, Cargo.lock, .claude/worktrees/) and is
  faster than raw `grep` over the whole tree.

### Never Do

- NEVER read `src/main.rs` from line 1 without first calling `get_symbols_overview`
  on it (or `read_memory("main_rs_navigation")` for a section map).
- NEVER use `mcp__serena__execute_shell_command` or `mcp__serena__create_text_file`
  — they are excluded in `.serena/project.yml`. Use the active agent's normal
  shell and patch/file-edit tools instead.
- NEVER attempt edits via Serena's editing tools (`replace_symbol_body`,
  `insert_after_symbol`, `replace_content`, `rename_symbol`). The project is in
  `read_only: true` mode for Serena — apply edits with the active agent's
  patch/file-edit tools.

### When To Reach For Which Tool

| Goal                                          | Tool |
|-----------------------------------------------|------|
| Map a file's top-level items                  | `get_symbols_overview` |
| Read one function / struct / impl block       | `find_symbol` with `name_path` + `include_body` |
| "Who calls this?" / blast radius              | `find_referencing_symbols` |
| Locate concept across the codebase            | `search_for_pattern` (regex) |
| Locate file by name fragment                  | `find_file` |
| Recall conventions / commands / arch          | `read_memory` |

### Configuration Notes

- Project config: `.serena/project.yml` (versioned).
- Indexed paths exclude `target/**`, `Cargo.lock`, `.claude/worktrees/**`, `.github/**`.
- To (re)build the symbol index for faster lookups:
  `uvx --from git+https://github.com/oraios/serena serena project index`.
- If Serena returns stale symbol information after large refactors, re-run the
  index command above.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **LibreCommander** (3087 symbols, 9064 relationships, 271 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before risky symbol edits.** Before changing a non-test function/method/type signature, public API, action handler, file operation, render/input flow, or doing a rename/refactor, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user. Docs-only, formatting-only, test-only, and small private implementation fixes may skip impact analysis.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit risky non-test symbols without first running `gitnexus_impact` on them.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/LibreCommander/context` | Codebase overview, check index freshness |
| `gitnexus://repo/LibreCommander/clusters` | All functional areas |
| `gitnexus://repo/LibreCommander/processes` | All execution flows |
| `gitnexus://repo/LibreCommander/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
