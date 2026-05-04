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
  — they are excluded in `.serena/project.yml`. Use Claude's `Bash` and `Write`
  tools instead.
- NEVER attempt edits via Serena's editing tools (`replace_symbol_body`,
  `insert_after_symbol`, `replace_content`, `rename_symbol`). The project is in
  `read_only: true` mode for Serena — apply edits with Claude's `Edit` / `Write`.

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

This project is indexed by GitNexus as **LibreCommander** (1560 symbols, 4447 relationships, 138 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
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
