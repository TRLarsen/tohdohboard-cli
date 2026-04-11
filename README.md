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

## Building and Running Locally

```bash
cargo build --release
```

The binary will be located in `target/release/tohdoh`.

## Usage

* **Add a task**:
  `tohdoh add "Title" --context work --status "To Do" "Description"`
* **List tasks**:
  `tohdoh list`
* **Move a task**:
  `tohdoh move <TASK_ID_PREFIX> --status Done --context home`
* **Delete a task**:
  `tohdoh delete <TASK_ID_PREFIX>`
