# AGENTS.md

## Rust Monorepo Instructions

This repository is a Rust workspace.

### Do NOT run `cargo` directly.

Always use the Makefile targets instead. Cargo may fail in agent environments.

### Checks After Changes

- Always run checks in this order after making changes:
  1. `make fmt` - Format code
  2. `make sqlx-prepare` - Update SQLx workspace metadata
  3. `make check` - Check the crates
  4. `make build` - Build the crates
  5. `make test` - Run tests

### Toolchain

The Makefile ensures a stable toolchain is used.  
Do not manually specify `rustup` or override toolchains unless explicitly instructed.

### Working Directory

Always run commands from the repository root.
