# TohdohBoard CLI

A native Rust terminal task manager for TohdohBoard.

## Overview
Manage your TohdohBoard tasks natively from the command line using this fast, compiled Rust binary. It uses the exact same data schema as the web app.

## Global Installation (Alias: tohdoh)

To use the `tohdoh` command from anywhere in your terminal:

```bash
cd tohdohboard-cli
cargo install --path .
```

*Make sure your cargo bin directory (usually `~/.cargo/bin`) is in your `$PATH`.*

## Remote Usage

By default, the CLI uses local files in `~/.tohdohboard/`. To use a server instead, set `TOHDOH_URL` to its `/api/data` URL (an empty `TOHDOH_URL` counts as unset):

```bash
export TOHDOH_URL="http://<your-server>:3001/api/data"
tohdoh list
```

Add the exports to your `.bashrc` or `.zshrc` to make them permanent.

`TOHDOH_URL` must end in `/data`: other endpoints (the archive) are derived from it by replacing that suffix. Requests time out after 10 s (connect) / 30 s (total).

How the server authenticates you decides what else to set:

* **Tailscale Serve** (private, tailnet only): Serve proxies to the backend on loopback and adds your identity header, so there is nothing to configure in the CLI. Just use the Serve URL:
  `export TOHDOH_URL="https://<machine>.<tailnet>.ts.net/api/data"`
* **Public / Tailscale Funnel** (reachable from the internet): the server runs in token mode and every request must carry a bearer token:
  `export TOHDOH_TOKEN="<your access token>"`
  The token is sent as `Authorization: Bearer <token>` and never printed. If the server answers `401` with a bearer challenge, the CLI tells you to set (or fix) `TOHDOH_TOKEN`. Prefer `https://` URLs whenever a token is used.
* **Proxy-mode server, tagged devices and CI**: a server in proxy mode also accepts bearer tokens when `[auth.tokens]` is configured. Use this where Tailscale Serve sends no identity header (for example tagged devices): set `TOHDOH_TOKEN` as above.

A `401` without a bearer challenge means the server expects its proxy to supply your identity: reach it through the proxy (for example `tailscale serve`) instead of directly. Redirects are never followed (they could silently drop a write); the error names the address to use. Trailing slashes in `TOHDOH_URL` are ignored; for archive commands it must not contain a query string.

Without `TOHDOH_URL` everything is local: the board is `~/.tohdohboard/data.json` and the archive is `~/.tohdohboard/archive.json`, the same format the server uses, so a server pointed at that directory sees the same archive.

Fields the CLI does not know about (added by other clients) are preserved when it saves.

## Building and Running Locally

```bash
cargo build --release
```

The binary will be located in `target/release/tohdoh`.

## Usage

Contexts and columns are referred to by their **id** (e.g. `work`, `todo`), not by display name (`To Do`). The default board has contexts `work`, `school`, `projects` and columns `todo`, `doing`, `done`.

* **Add a task**:
  `tohdoh add "Title" --context work --status todo "Description"`
  (`--status` defaults to `todo`; the description is optional)
* **List tasks**:
  `tohdoh list` (optionally `--context work` and/or `--status todo`)
* **Move a task**:
  `tohdoh move <TASK_ID_PREFIX> --status done --context projects`
* **Delete a task**:
  `tohdoh delete <TASK_ID_PREFIX>`

  An id prefix must match exactly one task: an empty or ambiguous prefix is an error that lists the candidates (`move`, `delete` and the `archive` commands all behave this way).
* **Archive**:
  `tohdoh archive done` archives every task in the `done` column (`--status <column>` and `--context <id>` narrow it),
  `tohdoh archive add <TASK_ID_PREFIX>...` archives specific tasks,
  `tohdoh archive list` shows the archive, `tohdoh archive restore <ID_PREFIX>...` brings tasks back.

  Archived tasks are removed from the board, not deleted. The CLI only archives on request; archiving old done tasks *automatically* is the server's job (see `[archive]` in its `config.toml`), so no age thresholds are configured here.
* **Add a context / column**:
  `tohdoh add-context "Side Projects"` / `tohdoh add-column "In Review"`
  (ids are derived from the name: `side-projects`, `in-review`)

## Documentation

The source is documented with Rustdoc, including private items. To build and open it:

```bash
cargo doc --no-deps --document-private-items --open
```
