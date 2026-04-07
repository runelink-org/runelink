# RuneLink

RuneLink is an **experimental** federated messaging network with a Slack/Discord-style model: **servers** contain **channels**, and channels contain **messages**.

This repo contains:
- A Rust server implementation (`runelink-server`)
- A Rust CLI client (`runelink-cli`, binary name: `rune`)
- A Rust client library (`runelink-client`) and shared API types (`runelink-types`) for building additional clients

> Status: **WIP**. APIs and data model may change. Not security-audited.

Public instance and web client: [**runelink.chat**](https://runelink.chat).

## What is RuneLink?

- **Federated**: many independent hosts can interoperate.
- **Server structure**: like Discord/Slack; join servers, talk in channels.
- **Open client ecosystem**: today, we have a CLI and web client, but the intent is that the community can build their own clients against the HTTP/Websocket API or the various SDKs provided.

## Concepts

- **Host**: a RuneLink deployment reachable at a host (on port `7000` by default).
- **User**: an account on exactly one host (your **home host**).
- **Server**: a workspace/community that "lives on" some host.
- **Channel**: a room inside a server.

## Repository layout

This is a Rust workspace (see `Cargo.toml`) with these crates:

- `runelink-server`: Axum HTTP/Websocket server + Postgres persistence + federation management.
- `runelink-cli`: the `rune` CLI client (a TUI is planned, but not the primary interface yet).
- `runelink-client`: reusable Rust client library for talking to RuneLink servers.
- `runelink-types`: shared request/response and host types.

## Federation (high level)

RuneLink separates **user authentication** from **server-to-server federation**:

- **Clients authenticate only with their home host** (user sessions are local).
- When a home host needs to interact with a remote host, it uses **server-to-server requests** authenticated with **short-lived JWTs**.
- Remote hosts validate those JWTs by discovering public keys via **JWKS** published at `/.well-known/jwks.json`.

More detailed federation/authentication documentation is planned.

## Authentication (high level)

Authentication is local to your home host. The current server exposes OIDC-style discovery endpoints and a token endpoint supporting:

- `password` grant (username/password) to get an access token + refresh token
- `refresh_token` grant to mint new access tokens

This is intentionally *not* federated: your end-user credentials are never shared with remote hosts.

## Getting started (local dev)

### Prerequisites

- Rust toolchain (this workspace uses **edition 2024**). If you don't have Rust installed yet, see [rustup](https://rustup.rs/).
- Postgres

### Build and use the CLI client (`rune`)

Install `rune` from source (recommended for now):

```bash
cargo install --path runelink-cli
```

Verify it's in your PATH:

```bash
rune --help
```

Typical flow:

```bash
# 1) Create an account on your home host (signup policy may change)
rune account create

# 2) Log in (stores tokens locally)
rune account login

# 3) Create a server (workspace/community)
rune server create

# 4) Create a channel
rune channel create

# 5) Send a message
rune message send
```

Most commands will prompt you for any missing values (host, IDs, message body, etc.) so you can get started quickly. For scripting and non-interactive use, most prompts also have `--...` flags (run `rune --help` and `rune <command> --help`).

### Run the server

If you want to host your own RuneLink server, run `runelink-server` (Axum + Postgres).

`runelink-server` reads configuration from TOML.

Create your local config from the example:

```bash
cp runelink-server/config.example.toml runelink-server/config.toml
```

Single-server config syntax:

```toml
[[servers]]
local_host = "localhost"
public_port = 7000
secure = false
database_url = "postgres://postgres:postgres@localhost/runelink"
key_dir = "/home/your-user/.local/share/runelink/keys/localhost/7000"
```

If you are running behind a TLS reverse proxy, keep the public host/port in
`local_host` + `public_port`, and bind the backend locally:

```toml
[[servers]]
local_host = "example.com"
public_port = 7000
secure = true
bind_host = "127.0.0.1"
bind_port = 17000
database_url = "postgres://postgres:postgres@localhost/runelink"
key_dir = "/home/your-user/.local/share/runelink/keys/7000"
```

A sample Caddy config for this setup lives at `deploy/Caddyfile.example`.

Then update `database_url` and any other values for your environment, install `sqlx-cli`, and run migrations:

`runelink-server` runtime config comes from TOML, but `sqlx` tooling (CLI/query checking) reads `DATABASE_URL` from env/`.env`. Copy `runelink-server/.env.example` to `runelink-server/.env` for local tooling, or pass `--database-url` to `sqlx` commands.

If you edit SQL queries checked by `sqlx`, refresh the offline metadata afterward with `cargo sqlx prepare --workspace`.

```bash
cargo install sqlx-cli --no-default-features --features postgres,rustls
sqlx migrate run
```

Start the server:

```bash
cargo run
```

If your config contains multiple `[[servers]]` entries, `runelink-server` will infer cluster mode and start multiple server instances in one process.

You can verify it's up with:

```bash
curl "http://localhost:7000/ping"
```

## Roadmap (high level)

- Direct messages: user-to-user direct messages and group chats.
- Presence updates: typing indicators, user statuses, and message notifications.
- Calls: group audio + video calls with WebRTC.
- For more detailed and up-to-date plans, see the [project board](https://github.com/orgs/runelink-org/projects/1).

## Contributing

For useful workspace commands, refer to the [makefile](./Makefile).

## License

GPL-3.0. See [LICENSE](./LICENSE).
