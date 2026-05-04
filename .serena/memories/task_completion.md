# Task Completion Checklist

After every code change:
1. `cargo fmt` — format
2. `cargo clippy` — lint (must pass, no warnings)
3. `cargo test` — all tests green
4. `cargo build --release` — release build OK

If clippy warns — fix before done.
