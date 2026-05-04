# Code Style & Conventions

## Linting (from Cargo.toml lints)
- `unsafe_code = "forbid"` — no unsafe
- `print_stdout = "deny"`, `print_stderr = "deny"` — no println/print
- `dbg_macro = "deny"` — no dbg!
- `unwrap_used = "warn"`, `expect_used = "warn"` — prefer ? operator / proper error handling
- `panic = "warn"`, `todo = "warn"`, `unimplemented = "warn"`
- `too_many_lines = "warn"`, `cognitive_complexity = "warn"`, `module_inception = "warn"`
- `needless_pass_by_value = "warn"`, `redundant_clone = "warn"`, `inefficient_to_string = "warn"`

## Naming
- Standard Rust naming: snake_case functions/vars, PascalCase types/structs/enums
- Short module names (fs, ops, ui, app)

## Patterns
- No unsafe code (forbidden)
- Error propagation via `?` operator
- Struct-based state (App struct in main.rs)
- Module per concern under src/{app,fs,ops,ui,input}/

## Formatting
- `cargo fmt` — standard rustfmt
