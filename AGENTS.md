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

### Resource naming

Servers, channels, text channels, and messages are identified by their owning host together with an ID that is only unique on that host. Name fields, variables, parameters, and protocol fields by what they hold:

- `server`: the resource itself, such as a `Server`.
- `server_id`: a bare `ServerId`. Use it only where the host is implied, such as in requests sent to the owning host, or for a resource on the same host as the resource that holds it (`ServerChannel::server_id`).
- `server_ref`: a `ServerRef` (`ResourceRef<ServerId>`), the host and ID that name a resource on any host. Use it wherever the host isn't implied.

The same applies to each kind of resource (`channel`, `text_channel`, `message`). Users have no bare ID, so a `UserRef` is named `user_ref`, or for its role, such as `author` or `creator`.
