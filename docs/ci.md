# CI Tools

CI tools and execution methods used in the jetdb project.

## Tool List

### 1. cargo test — Run Tests

Run all unit tests and integration tests across the project.

```bash
cargo test
```

Per-crate tests:

```bash
cargo test -p jetdb          # Library only
cargo test -p jetdb-cli      # CLI only
```

### 2. cargo clippy — Linting

Rust static analysis tool. Treats warnings as errors to maintain code quality.

```bash
cargo clippy -- -D warnings
```

Installation (if not already included with rustup):

```bash
rustup component add clippy
```

### 3. cargo audit — Vulnerability Check

Check dependency crates for known security vulnerabilities.

```bash
cargo audit
```

Installation:

```bash
cargo install cargo-audit
```

### 4. cargo doc — Documentation Build

Generate API documentation for the entire workspace. Detects broken links and doc comment syntax errors.

```bash
cargo doc --workspace
```

Generated documentation is output to `target/doc/jetdb/index.html`.

### 5. rust-code-analysis-cli — Complexity Metrics

Measure cyclomatic complexity, cognitive complexity, and other source code metrics.

```bash
rust-code-analysis-cli -m -p crates/ -O json
```

Installation:

```bash
cargo install rust-code-analysis-cli --locked
```

> **Note**: The `--locked` flag is required. Without it, compilation fails due to a tree-sitter version mismatch ([GitHub Issue #1140](https://github.com/nickel-org/rust-code-analysis/issues/1140)).
>
> This project's last release was January 2023 and maintenance has stalled.

## Recommended Execution Order

1. `cargo test` — Verify existing tests pass first
2. `cargo clippy -- -D warnings` — Check code quality
3. `cargo audit` — Check for security issues
4. `cargo doc --workspace` — Verify documentation builds correctly

Running other checks is pointless if tests don't pass, so `cargo test` comes first. `cargo clippy` detects code issues and should run early. `cargo audit` and `cargo doc` are independent and can run in any order.
