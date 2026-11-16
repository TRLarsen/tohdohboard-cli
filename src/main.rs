//! `tohdoh`: a terminal task manager for TohdohBoard.
//!
//! TohdohBoard is a kanban-style board: **tasks** live in **contexts**
//! (swimlanes such as `work`) and **columns** (statuses such as `todo`,
//! `doing`, `done`). This crate is the command-line client; it reads and
//! writes the same JSON document as the web app and the backend.
//!
//! # Where the data lives
//!
//! * **Local file** (default): `~/.tohdohboard/data.json`. It is created with a
//!   default board (contexts `work`, `school`, `projects`; columns `todo`,
//!   `doing`, `done`) on first use.
//! * **Remote server**: set `TOHDOH_URL` to the backend's `/api/data` URL and
//!   every command reads and writes the board over HTTP instead; the local file
//!   is not used.
//!
//! See [`storage`] for details.
//!
//! # Usage
//!
//! Contexts and columns are always referred to by their **id** (`todo`), never
//! by display name (`To Do`).
//!
//! ```text
//! # Optional: use a remote board instead of ~/.tohdohboard/data.json
//! export TOHDOH_URL="http://localhost:3001/api/data"
//!
//! tohdoh add "Write report" --context work --status todo "First draft"
//! tohdoh list --context work
//! tohdoh move 1a2b3c4d --status doing
//! tohdoh delete 1a2b3c4d
//! tohdoh add-context "Side Projects"   # id: side-projects
//! tohdoh add-column "In Review"        # id: in-review
//! ```
//!
//! Run `tohdoh --help` or `tohdoh <command> --help` for the options.
//!
//! # Code layout
//!
//! * [`models`]: the serde types of the board document.
//! * [`storage`]: loading and saving it (local file or remote server).
//! * this module: the [`clap`] command-line definition and the command
//!   implementations, all in [`main`].

#![warn(missing_docs)]

mod models;
mod storage;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use models::Task;
use storage::Storage;
use uuid::Uuid;
use chrono::Utc;
use comfy_table::{Table, Color, Cell, Attribute};

/// Command-line interface of `tohdoh`.
///
/// The about text shown by `--help` is set explicitly on the `command`
/// attribute below.
#[derive(Parser)]
#[command(name = "tohdoh")]
#[command(about = "A native Rust terminal task manager for TohdohBoard", long_about = None)]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Commands,
}

/// The `tohdoh` subcommands.
///
/// Each variant doubles as the `--help` text of its subcommand: the first line
/// is the summary, the rest appears in `tohdoh <command> --help`.
#[derive(Subcommand)]
enum Commands {
    /// Add a task
    ///
    /// Create a task in the given context.
    ///
    /// The context and status must already exist (see `add-context` and
    /// `add-column`); the status defaults to `todo`. The task gets a fresh random
    /// id; use `list` to see its first 8 characters.
    Add {
        /// Task title
        title: String,
        /// Context id the task belongs to (e.g. `work`)
        #[arg(long)]
        context: String,
        /// Column id to put the task in (e.g. `todo`, `doing`, `done`)
        #[arg(long, default_value = "todo")]
        status: String,
        /// Longer description (optional; stored empty if omitted)
        description: Option<String>,
    },
    /// List tasks
    ///
    /// Prints a table of tasks (id prefix, title, context, status, last update),
    /// sorted by context position and then column position. Tasks whose context
    /// or column is unknown count as position 0. Both filters take ids and may be combined.
    List {
        /// Only show tasks in this context id
        #[arg(long)]
        context: Option<String>,
        /// Only show tasks in this column id
        #[arg(long)]
        status: Option<String>,
    },
    /// Move a task
    ///
    /// Change the status and/or context of a task.
    ///
    /// TASK_ID is matched as a prefix of the full UUID; only the first matching
    /// task in the board is changed, even if the prefix matches several. With
    /// neither `--status` nor `--context` the task is left as is, except that its
    /// updated time is refreshed.
    Move {
        /// Task id, or the beginning of it (the first 8 characters shown by `list` suffice)
        task_id: String,
        /// Column id to move the task to
        #[arg(long)]
        status: Option<String>,
        /// Context id to move the task to
        #[arg(long)]
        context: Option<String>,
    },
    /// Delete a task
    ///
    /// Delete task(s) whose id starts with TASK_ID.
    ///
    /// Every task whose UUID starts with TASK_ID is removed, not just the first;
    /// use a long enough prefix to be unambiguous. An empty TASK_ID matches (and
    /// deletes) every task.
    Delete {
        /// Task id, or the beginning of it; ALL tasks with this prefix are deleted
        task_id: String,
    },
    /// Add a context (swimlane)
    ///
    /// The id is the name lowercased, trimmed, with spaces replaced by `-`
    /// (`"Side Projects"` becomes `side-projects`); the name is stored as typed.
    /// Fails if a context with that id already exists. The new context is placed
    /// at position `count + 1`.
    AddContext {
        /// Display name; the id is derived from it
        name: String,
    },
    /// Add a column (status)
    ///
    /// The id is the name lowercased, trimmed, with spaces replaced by `-`
    /// (`"In Review"` becomes `in-review`); the name is stored as typed. Fails if
    /// a column with that id already exists. The new column is placed at position
    /// `count + 1`.
    AddColumn {
        /// Display name; the id is derived from it
        name: String,
    },
}

/// Entry point: parses the command line, loads the board, runs one command and
/// saves the board if the command changed it.
///
/// The board is loaded from [`Storage`] before any command runs (so a first run
/// in local mode creates the default board file even for `list`). Commands
/// save the whole document only after they succeed; `list` never saves.
///
/// # Errors
///
/// Returns an error (printed by the runtime, exit code 1) if storage fails or
/// a command is invalid: unknown context or column id, no task matching the
/// id prefix, or a duplicate context or column id.
fn main() -> Result<()> {
    let cli = Cli::parse();
    let storage = Storage::new()?;
    let mut data = storage.load()?;

    match cli.command {
        Commands::Add { title, context, status, description } => {
            if !data.contexts.iter().any(|c| c.id == context) {
                bail!("Context '{}' does not exist.", context);
            }
            if !data.columns.iter().any(|c| c.id == status) {
                bail!("Column status '{}' does not exist.", status);
            }

            let task = Task {
                id: Uuid::new_v4(),
                title,
                description: description.unwrap_or_default(),
                status,
                context_id: context.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            
            println!("Adding task: {}", task.title);
            data.tasks.push(task);
            storage.save(&data)?;
            println!("Task added successfully.");
        }
        Commands::List { context, status } => {
            let mut table = Table::new();
            table.set_header(vec![
                Cell::new("ID").add_attribute(Attribute::Bold).fg(Color::Cyan),
                Cell::new("Title").add_attribute(Attribute::Bold),
                Cell::new("Context").add_attribute(Attribute::Bold).fg(Color::Green),
                Cell::new("Status").add_attribute(Attribute::Bold).fg(Color::Yellow),
                Cell::new("Updated").add_attribute(Attribute::Bold).fg(Color::DarkGrey),
            ]);

            // Sort contexts and columns by position for lookup
            let mut sorted_tasks = data.tasks.clone();
            
            sorted_tasks.sort_by(|a, b| {
                let ctx_a = data.contexts.iter().find(|c| c.id == a.context_id).map(|c| c.position).unwrap_or(0);
                let ctx_b = data.contexts.iter().find(|c| c.id == b.context_id).map(|c| c.position).unwrap_or(0);
                
                if ctx_a != ctx_b {
                    return ctx_a.cmp(&ctx_b);
                }
                
                let col_a = data.columns.iter().find(|c| c.id == a.status).map(|c| c.position).unwrap_or(0);
                let col_b = data.columns.iter().find(|c| c.id == b.status).map(|c| c.position).unwrap_or(0);
                
                col_a.cmp(&col_b)
            });

            let filtered_tasks = sorted_tasks.iter().filter(|t| {
                let context_match = context.as_ref().map_or(true, |c| &t.context_id == c);
                let status_match = status.as_ref().map_or(true, |s| &t.status == s);
                context_match && status_match
            });

            for task in filtered_tasks {
                let short_id = task.id.to_string().chars().take(8).collect::<String>();
                table.add_row(vec![
                    short_id,
                    task.title.clone(),
                    task.context_id.clone(),
                    task.status.clone(),
                    task.updated_at.format("%Y-%m-%d %H:%M").to_string(),
                ]);
            }

            println!("{}", table);
        }
        Commands::Move { task_id, status, context } => {
            let mut found = false;
            for task in data.tasks.iter_mut() {
                if task.id.to_string().starts_with(&task_id) {
                    if let Some(s) = status.clone() {
                        if !data.columns.iter().any(|cx| cx.id == s) {
                            bail!("Column '{}' does not exist.", s);
                        }
                        task.status = s;
                    }
                    if let Some(c) = context.clone() {
                        if !data.contexts.iter().any(|cx| cx.id == c) {
                            bail!("Context '{}' does not exist.", c);
                        }
                        task.context_id = c;
                    }
                    task.updated_at = Utc::now();
                    found = true;
                    println!("Moved task '{}'", task.title);
                    break;
                }
            }

            if !found {
                bail!("Task with ID starting with '{}' not found.", task_id);
            }

            storage.save(&data)?;
        }
        Commands::Delete { task_id } => {
            let initial_len = data.tasks.len();
            data.tasks.retain(|t| !t.id.to_string().starts_with(&task_id));
            
            if data.tasks.len() == initial_len {
                bail!("Task with ID starting with '{}' not found.", task_id);
            }

            storage.save(&data)?;
            println!("Task deleted successfully.");
        }
        Commands::AddContext { name } => {
            let id = name.to_lowercase().trim().replace(' ', "-");
            if data.contexts.iter().any(|c| c.id == id) {
                bail!("Context with ID '{}' already exists.", id);
            }

            let context = models::Context {
                id: id.clone(),
                name,
                position: data.contexts.len() as i32 + 1,
            };

            println!("Adding context: {}", context.name);
            data.contexts.push(context);
            storage.save(&data)?;
            println!("Context '{}' added successfully.", id);
        }
        Commands::AddColumn { name } => {
            let id = name.to_lowercase().trim().replace(' ', "-");
            if data.columns.iter().any(|c| c.id == id) {
                bail!("Column with ID '{}' already exists.", id);
            }

            let column = models::Column {
                id: id.clone(),
                name,
                position: data.columns.len() as i32 + 1,
            };

            println!("Adding column: {}", column.name);
            data.columns.push(column);
            storage.save(&data)?;
            println!("Column '{}' added successfully.", id);
        }
    }

    Ok(())
}
