# Libre Commander (lc) — Agent Instructions

Rust TUI file manager (Midnight Commander clone). Single binary.
Stack: **Ratatui 0.30 + crossterm 0.29**, edition 2024, MSRV 1.95, `unsafe_code = forbid`.

> **MCP tools available:** Serena (LSP-backed symbol navigation) and GitNexus
> (knowledge graph, impact analysis). Prefer these over grep/find.
> See sections at bottom for usage.

## Hard Rules

- `unsafe_code = "forbid"` — don't attempt unsafe code
- NEVER `println!`/`eprintln!`/`dbg!` in committed code — corrupts TUI display, denied by clippy. Use `app::debug_log!` macro instead
- NEVER mutate state from `ui::*` draw code — rendering is a pure function of `AppState`; only `input::*` handlers mutate
- NEVER block the event thread — work > 50ms MUST go to `rayon` or `app::job_runner`
- NEVER introduce tokio — project is intentionally sync; use rayon for parallelism, `mpsc` channels for progress
- Delete/move/overwrite MUST have explicit user confirmation unless already confirmed in current flow
- Symlinks are data — don't follow during chmod/copy/delete unless the operation explicitly requires it
- Cross-device moves MUST use copy+delete fallback with cancellation and no-clobber preserved
- Archive extraction MUST validate paths (zip slip), set size limits, handle symlinks safely
- NEVER amend existing commits — always create new commits for each logical change
- NEVER commit `target/`, editor swap files, or worktree dirs
- Don't add network calls — this is an offline tool by design

## Build & Validate

| Action | Command |
|--------|---------|
| Dev run | `cargo run` |
| Release build | `cargo build --release` |
| Run all tests | `cargo test --locked` |
| Single test | `cargo test <name> -- --nocapture` |
| CI gate (run before declaring done) | `cargo fmt && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked && cargo build --release --locked` |

CI: GitHub Actions (`.github/workflows/rust.yml`), ubuntu + macos matrix. Must be green before merge.

## Repository Map

| Directory | Responsibility | Key files |
|-----------|---------------|-----------|
| `src/main.rs` (~570 lines) | Event loop, `run_app()`, dispatch, `TerminalGuard` | Entry point |
| `src/render.rs` | Render orchestration | `render_ui()` |
| `src/render_dialog_map.rs` | Dialog rendering dispatch | by `DialogKind` |
| `src/input/` | Key/mouse handling — **mutates state** | `normal.rs`, `dialogs.rs`, `mode_dispatch.rs` |
| `src/app/` | State types, config, keymaps, job runner, watcher sync | `types/app_state.rs` (~36 fields) |
| `src/ops/` | File operations — copy, move, delete, search, archive, sort | MUST be cancellable |
| `src/ui/` | Pure rendering — **never mutates state** | `panels/`, `dialogs/`, `viewer/` |
| `src/fs/` | Directory reads (rayon), `notify` watcher, path helpers, chafa CLI | |
| `src/tests/` | Integration tests: keybinds, search, dialogs, viewer, etc. (14 files) | |
| `src/menu.rs` | F9 menu bar definitions | |

Largest production files: `ops/file_ops/mod.rs` (~990), `ops/batch.rs` (~930), `input/dialogs.rs` (~830).

## Code Conventions (Non-Default Only)

- Functions > 100 lines trigger `too_many_lines` — split along natural seams; propose split to user first
- Prefer `?` and `let ... else` over `.unwrap()`; `#[allow]` only on `mod tests` blocks
- Use `unicode-width` for column math — `len() != display width` for CJK/emoji filenames
- Prefer `std::io::Result`; avoid `anyhow`
- No `#[allow(...)]` to suppress lints except: `unwrap_used`/`expect_used`/`panic` on tests, `print_stdout` when TUI suspended, `non_snake_case` for external tokens
- ANSI escape sequences in strings are NOT rendered — use `ansi-to-tui` crate
- `notify` backend differs on macOS (`cfg(target_os)`) — test path/permission logic for both platforms
- Conventional Commits: `fix:`, `feat(scope):`, `refactor:`. Don't bump `Cargo.toml` version unless asked

## Testing

- **Unit:** inline `#[cfg(test)] mod tests` in same file; directory modules have `tests.rs` siblings
- **Integration:** `src/tests/` — 14 files with `AppState` harness
- **File ops:** always use `tempfile::TempDir`; cover symlinks, cross-device, Unicode filenames
- **UI rendering:** `Terminal::new(TestBackend::new(w, h))` + assert on buffer; see `ui/viewer/tests.rs`
- **Async patterns:** `EventHarness` from `fs/watcher/tests.rs`

## Gotchas

- Event loop uses blocking `crossterm::event::read()` + `event::poll(33ms)` (`EVENT_POLL_TIMEOUT_MS` in main.rs). Don't change timeout without understanding spinner tick (200ms)
- `TerminalGuard` in main.rs provides RAII cleanup on panic — don't bypass it in error paths
- When spawning external process: MUST `LeaveAlternateScreen` + `disable_raw_mode` first, reclaim after. Vim queries terminal capabilities via ANSI that crossterm reads as keyboard events
- Adding a new dialog requires touching 4 places: `DialogKind` variant (modes.rs), detail struct (types/dialogs.rs), input handler (input/dialogs.rs), render in `render_dialog_map.rs` + `ui/dialogs/`
- Main loop calls `sync_watcher_job_state` before `sync_watcher_paths` and `pre_draw()` before `terminal.draw()` — check before modifying the loop
- `AppState` has ~36 fields — all UI-relevant data is here by design (enables pure rendering)
- Config migration requires user approval — users hand-edit `~/.config/lc/config.toml`

## File Size Policy

800 lines is a checkpoint, not a hard limit:
- Evaluate if split along natural seams exists; propose to user before splitting
- Cohesive files (single state machine, one impl block) — keep even if large
- Never split by line count alone

## Architecture (At a Glance)

- Sync event loop + rayon offloading (no async/tokio)
- Single `AppState` struct as source of truth → pure rendering
- Ratatui immediate-mode + crossterm backend + double-buffered diff

For full architecture detail, event loop diagram, and ADRs — read `src/main.rs` via Serena/GitNexus, not here.

## Serena — Semantic Code Navigation

Always Do:
- `get_symbols_overview` on a file before reading it whole
- `find_symbol` with `include_body: true` to load single symbol body
- `find_referencing_symbols` before renaming/removing any public symbol
- `read_memory` for conventions/architecture recall

Never Do:
- `execute_shell_command` or `create_text_file` — excluded in `.serena/project.yml`
- Edit via Serena tools — project is `read_only: true`; use normal edit/file tools

See `.serena/project.yml` for config. Re-index if stale: `uvx --from git+https://github.com/oraios/serena serena project index`

## GitNexus — Code Intelligence

Always Do:
- **Run `gitnexus_impact({target, direction: "upstream"})`** before editing any symbol — report blast radius to user
- **Run `gitnexus_detect_changes()`** before committing — verify only expected symbols affected
- **Warn user** if impact analysis returns HIGH or CRITICAL risk
- `gitnexus_query({query})` for finding execution flows (instead of grep)
- `gitnexus_context({name})` for full caller/callee/process context

Never Do:
- Edit any symbol without running `gitnexus_impact` first
- Ignore HIGH/CRITICAL risk warnings
- Rename with find-and-replace — use `gitnexus_rename`

Resources: `gitnexus://repo/{name}/context`, `/clusters`, `/processes`, `/process/{name}`
