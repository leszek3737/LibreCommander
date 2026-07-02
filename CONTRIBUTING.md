# Contributing to Libre Commander

Thanks for your interest in improving `lc`! This guide covers everything you need
to build, test, and submit changes.

> **Working with an AI assistant?** [`AGENTS.md`](AGENTS.md) contains the
> machine-oriented instructions (Serena/GitNexus usage, hard rules, gotchas).
> Read that first — humans and agents follow the same rules.

---

## Quick Start

```bash
git clone https://github.com/leszek3737/LibreCommander.git
cd LibreCommander
cargo run                  # dev build, optimized for fast incremental rebuilds
cargo test --locked        # full test suite
```

Requirements: **Rust 1.95+** (edition 2024), `cargo`.

Before proposing a feature, please [open an issue](https://github.com/leszek3737/LibreCommander/issues)
to discuss scope — especially for anything beyond a bug fix.

---

## The Quality Gate

CI runs these on every push and PR (`.github/workflows/rust.yml`, Linux + macOS
matrix). They must be green before merge — run them locally first:

```bash
cargo fmt
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

| Command | Purpose |
|---------|---------|
| `cargo fmt` | Format the code |
| `cargo clippy --locked --all-targets -- -D warnings` | Zero lint warnings |
| `cargo test --locked` | All tests pass |
| `cargo build --release --locked` | Release build succeeds |

> `--locked` is intentional: contributions must build against the committed
> `Cargo.lock`, not a freshly resolved dependency set.

---

## Hard Rules

These are non-negotiable — they protect the TUI and your data:

- **`unsafe_code = "forbid"`** — no `unsafe`, anywhere. No exceptions.
- **No `println!` / `eprintln!` / `dbg!`** in committed code — they corrupt the
  TUI display and are denied by clippy. Use the `app::debug_log!` macro instead.
- **Never mutate state from `ui::*` draw code** — rendering must be a pure
  function of `AppState`. Only `input::*` handlers mutate state.
- **Never block the event thread** — any work over ~50 ms goes to `rayon` or the
  `app::job_runner`, never inline.
- **Never introduce `tokio`/async** — the project is intentionally synchronous.
  Use `rayon` for parallelism, `mpsc` channels for progress.
- **Destructive ops need explicit confirmation** — delete/move/overwrite must
  confirm unless already confirmed in the current flow.
- **Symlinks are data** — don't follow them during chmod/copy/delete unless the
  operation explicitly requires it.
- **Cross-device moves** use copy+delete fallback with cancellation and no-clobber.
- **Archive extraction** validates paths (zip-slip), sets size limits, handles
  symlinks safely.
- **No network calls** — `lc` is offline by design.

---

## Architecture (at a glance)

```
                 ┌─────────────┐
   crossterm ──► │  event loop │ (main.rs)   blocking read + poll(33ms)
                 └──────┬──────┘
                        │ events
                        ▼
                 ┌─────────────┐         ┌──────────────┐
                 │  input/*    │ ──mut──► │  AppState    │  single source of truth (~36 fields)
                 │  handlers   │         └──────┬───────┘
                 └─────────────┘                │ pure read
                        ▲                       ▼
                 ┌─────────────┐         ┌──────────────┐
                 │  ops/*      │ ◄─jobs─ │  ui/*        │  pure render, never mutates
                 │ (rayon)     │         │ ratatui      │
                 └─────────────┘         └──────────────┘
```

- **Sync event loop + rayon offloading** — no async runtime.
- **One `AppState` struct** holds all UI-relevant data → enables pure rendering.
- **Ratatui** immediate-mode + crossterm backend + double-buffered diff.

### Repository map

| Directory | Responsibility | Notes |
|-----------|---------------|-------|
| `src/main.rs` | Event loop, dispatch, `TerminalGuard` | RAII terminal cleanup on panic |
| `src/render.rs` | Render orchestration | |
| `src/render_dialog_map.rs` | Dialog render dispatch | by `DialogKind` |
| `src/input/` | Key/mouse handling — **mutates state** | `normal.rs`, `dialogs.rs`, `mode_dispatch.rs` |
| `src/app/` | State types, config, keymaps, job runner, watcher sync | `types/app_state.rs` |
| `src/ops/` | File operations — copy, move, delete, search, archive, sort | MUST be cancellable |
| `src/ui/` | Pure rendering — **never mutates state** | `panels/`, `dialogs/`, `viewer/` |
| `src/fs/` | Directory reads (rayon), `notify` watcher, path helpers | |
| `src/tests/` | Integration tests | `AppState` harness |
| `src/menu.rs` | `F9` menu bar definitions | |

For deeper detail (event-loop internals, ADRs), read `src/main.rs` and
`AGENTS.md`.

---

## Code Conventions

We follow standard Rust style; only the **non-default** rules are listed:

- **Functions over ~100 lines** trip `too_many_lines`. Split along natural seams;
  propose the split in your PR first.
- Prefer `?` and `let ... else` over `.unwrap()`/`.expect()`.
  `#[allow(...)]` only on `mod tests` blocks.
- Use `unicode-width` for column math — `len() != display width` for CJK/emoji
  filenames.
- Prefer `std::io::Result`; avoid `anyhow`.
- `#[allow(...)]` to suppress lints is allowed **only** for:
  `unwrap_used`/`expect_used`/`panic` on tests, `print_stdout` when the TUI is
  suspended, `non_snake_case` for external tokens.
- ANSI escape sequences in strings are **not** rendered — use the `ansi-to-tui` crate.
- The `notify` watcher backend differs on macOS (`cfg(target_os)`) — test
  path/permission logic for both platforms.

### File-size policy

800 lines is a **checkpoint**, not a hard limit. Evaluate whether a split along
natural seams exists; propose it before splitting. Cohesive files (single state
machine, one impl block) stay even if large. Never split by line count alone.

---

## Testing

- **Unit tests:** inline `#[cfg(test)] mod tests` in the same file; directory
  modules have `tests.rs` siblings.
- **Integration tests:** `src/tests/` — use the `AppState` harness.
- **File ops:** always use `tempfile::TempDir`; cover symlinks, cross-device,
  and Unicode filenames.
- **UI rendering:** `Terminal::new(TestBackend::new(w, h))` + assert on the
  buffer. See `ui/viewer/tests.rs`.
- **Async/watcher patterns:** `EventHarness` from `fs/watcher/tests.rs`.

```bash
cargo test --locked                     # everything
cargo test <name> -- --nocapture        # single test with output
```

---

## Gotchas

- The event loop uses blocking `crossterm::event::read()` + `event::poll(33ms)`
  (`EVENT_POLL_TIMEOUT_MS` in `main.rs`). Don't change the timeout without
  understanding the spinner tick (200 ms).
- `TerminalGuard` provides RAII cleanup on panic — don't bypass it in error paths.
- When spawning an external process: you **must** `LeaveAlternateScreen` +
  `disable_raw_mode` first, and reclaim after. (Vim queries terminal
  capabilities via ANSI that crossterm reads as keyboard events.)
- Adding a new dialog touches **4 places**: `DialogKind` variant (`modes.rs`),
  detail struct (`types/dialogs.rs`), input handler (`input/dialogs.rs`), and
  render (`render_dialog_map.rs` + `ui/dialogs/`).
- The main loop calls `sync_watcher_job_state` before `sync_watcher_paths`, and
  `pre_draw()` before `terminal.draw()` — check this before modifying the loop.
- Config migration requires user approval — users hand-edit `config.toml`.

---

## Commits

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(archive): add zstd write support
fix(viewer): correct horizontal scroll on wide lines
refactor(input): split mode dispatch
docs(readme): add screenshot
```

- Don't bump the `Cargo.toml` version unless asked.
- Keep commit messages clean and self-contained — no co-author trailers,
  attribution lines, or "generated with …" notes.
- Each logical change is its own commit; never amend a commit that has been
  pushed.

---

## Filing issues & pull requests

- **Bugs:** use the bug-report template; include OS, terminal, Rust version, and
  minimal reproduction steps.
- **Features:** open an issue to discuss before implementing large changes.
- **Pull requests:** use the PR template, ensure the quality gate is green, and
  link the issue you're closing (`Closes #123`).

Happy hacking! 🦀
