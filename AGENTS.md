# Libre Commander (lc) — Agent Instructions

Rust TUI file manager (Ratatui + crossterm). Single binary, no runtime deps.

## Build & Run

```bash
cargo build --release        # binary: target/release/lc
cargo run                    # dev run
./target/release/lc          # direct run
```

## Test & Lint

```bash
cargo test                   # run tests (tests/ dir currently empty)
cargo clippy                 # lint
cargo fmt                    # format
```

## Architecture

```
src/
  main.rs        # entry point, app loop
  lib.rs         # module exports (app, ops, ui)
  app/           # core logic (config, types, dir_tree, user_menu)
  fs/            # filesystem operations
  ops/           # file ops, search, sorting
  ui/            # Ratatui rendering (panels, dialogs, viewer)
```

Config: `~/.config/lc/config.toml`  
User menu: `.mc.menu` or `~/.config/lc/menu`

## GitNexus (Code Intelligence)

Repo indexed as **lc**. Use GitNexus MCP tools for navigation and impact analysis.

> If GitNexus warns index is stale: run `npx gitnexus analyze` first.

### Always Do

- **MUST run `gitnexus_impact` before editing any symbol** — report blast radius to user
- **MUST run `gitnexus_detect_changes()` before commit** — verify affected scope
- **MUST warn user** if impact returns HIGH or CRITICAL risk
- Use `gitnexus_query({query: "concept"})` to find execution flows
- Use `gitnexus_context({name: "symbolName"})` for full symbol context

### Never Do

- NEVER edit without running `gitnexus_impact` first
- NEVER ignore HIGH/CRITICAL risk warnings
- NEVER rename with find-and-replace — use `gitnexus_rename`
- NEVER commit without `gitnexus_detect_changes()`

### Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/lc/context` | Codebase overview |
| `gitnexus://repo/lc/clusters` | Functional areas |
| `gitnexus://repo/lc/processes` | Execution flows |
| `gitnexus://repo/lc/process/{name}` | Step-by-step trace |

### Skill Files

| Task | Read |
|------|------|
| Architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Impact analysis / "What breaks?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Debugging / "Why failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Refactoring | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |