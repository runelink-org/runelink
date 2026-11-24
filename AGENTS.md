# AGENTS.md

## Rust Monorepo Instructions

This repository is a Rust workspace.

### Do NOT run `cargo` directly.

Always use the Makefile targets instead. Cargo may fail in agent environments.

### Checks After Changes

- After making changes, run `make verify` from the repository root.

### Toolchain

The Makefile ensures a stable toolchain is used.  
Do not manually specify `rustup` or override toolchains unless explicitly instructed.

### Working Directory

Always run commands from the repository root.

### Using the CLI

When running `rune`, set `RUNE_NON_INTERACTIVE=1` so missing input fails instead of waiting on a prompt, unless you are testing the interactive prompts. Also set `RUNE_JSON=1` by default to get parseable output.
