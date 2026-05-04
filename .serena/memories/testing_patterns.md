# Testing Patterns

## Where tests live

- **Unit tests inline in `src/main.rs`** under `#[cfg(test)] mod tests`,
  starting around line 2074. Use `find_symbol({name_path: "tests", relative_path: "src/main.rs"})`
  to load only the test module.
- The top-level `tests/` directory **does not currently exist** as a populated
  integration-test tree. New integration tests should be added there as
  `tests/<feature>.rs` files; each becomes its own crate.
- Some `ops/*.rs` files contain their own `#[cfg(test)] mod tests` blocks —
  check with `get_symbols_overview` per file.

## Patterns in use

- **Filesystem isolation:** `tempfile` (dev-dependency) is used to create
  scratch directories. Always use `TempDir` rather than `/tmp` paths so tests
  are parallel-safe and self-cleaning.
- **No mocking of std::fs:** ops are tested against a real temp directory.
- **No async runtime:** the project is synchronous (rayon for parallelism,
  notify for fs events). Tests are plain `#[test]` functions.
- **No external processes** are spawned in tests — keep it that way; the
  binary itself shells out (`app/shell.rs`) but those paths are not exercised
  in unit tests.

## Running tests

- `cargo test` — full suite
- `cargo test <pattern>` — filter by name
- `cargo test -- --nocapture` — see stdout (note: `print_*` is denied in
  production code; tests may print via `eprintln!`-equivalents only when
  necessary, prefer assertions)

## When adding tests

1. If exercising a single function in `ops/` or `app/`, add a
   `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of that file.
2. If exercising end-to-end behaviour involving multiple modules,
   create `tests/<name>.rs` (this also forces using only the public API).
3. Use `tempfile::tempdir()` for any test that touches the filesystem.
4. Run `cargo fmt && cargo clippy && cargo test` before considering it done
   (see `task_completion` memory).
