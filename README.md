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

## Tailnet / Remote Usage

By default, the CLI looks for data in `~/.tohdohboard/data.json`. To use it from any machine on your Tailnet, point it to your backend server's API:

```bash
export TOHDOH_URL="http://<your-server-ip>:3001/api/data"
tohdoh list
```

Add this export to your `.bashrc` or `.zshrc` to make it permanent.

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
  (changes only the first task matching the prefix)
* **Delete a task**:
  `tohdoh delete <TASK_ID_PREFIX>`
  (deletes *every* task whose id starts with the prefix)
* **Add a context / column**:
  `tohdoh add-context "Side Projects"` / `tohdoh add-column "In Review"`
  (ids are derived from the name: `side-projects`, `in-review`)

## Documentation

The source is documented with Rustdoc, including private items. To build and open it:

```bash
cargo doc --no-deps --document-private-items --open
```
