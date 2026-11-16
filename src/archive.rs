//! The `tohdoh archive` command group.
//!
//! Archiving moves tasks out of the board document into a separate archive
//! (`archive.json` locally, `/api/archive` on the server), so finished work
//! stops cluttering `list` and the web board. Archiving *by age* is not done
//! here: the server sweeps old done tasks itself, as configured in its
//! `config.toml`; the CLI only archives what you ask it to.

use crate::models::{TohdohBoardData, short_time};
use crate::resolve::resolve_prefix;
use crate::storage::Storage;
use anyhow::{Result, bail};
use clap::Subcommand;
use comfy_table::{Attribute, Cell, Color, Table};
use std::collections::HashSet;

/// Subcommands of `tohdoh archive`.
#[derive(Subcommand)]
pub enum ArchiveCommand {
    /// List archived tasks, newest archived first
    List,
    /// Archive tasks by id
    ///
    /// Each TASK_ID_PREFIX must identify exactly one task on the board.
    Add {
        /// Task ids, or the beginnings of them
        #[arg(required = true)]
        task_ids: Vec<String>,
    },
    /// Archive every task in a done column
    ///
    /// Archives all tasks whose status is `--status` (default `done`),
    /// optionally only those in `--context`.
    Done {
        /// Column id that counts as done
        #[arg(long, default_value = "done")]
        status: String,
        /// Only archive tasks in this context id
        #[arg(long)]
        context: Option<String>,
    },
    /// Restore archived tasks to the board
    ///
    /// Each ID_PREFIX must identify exactly one archived task. Status and
    /// context are kept; the updated time is set to now.
    Restore {
        /// Archived task ids, or the beginnings of them
        #[arg(required = true)]
        task_ids: Vec<String>,
    },
}

/// Runs one archive subcommand against `storage`; `board` is the board
/// document already loaded by the caller (used to resolve ids and select
/// done tasks).
///
/// # Errors
///
/// Fails on storage errors or if an id prefix is empty, unknown or ambiguous;
/// ids are all resolved before anything is changed.
pub fn run(storage: &Storage, board: &TohdohBoardData, command: ArchiveCommand) -> Result<()> {
    match command {
        ArchiveCommand::List => {
            let mut table = Table::new();
            table.set_header(vec![
                Cell::new("ID").add_attribute(Attribute::Bold).fg(Color::Cyan),
                Cell::new("Title").add_attribute(Attribute::Bold),
                Cell::new("Context").add_attribute(Attribute::Bold).fg(Color::Green),
                Cell::new("Status").add_attribute(Attribute::Bold).fg(Color::Yellow),
                Cell::new("Archived").add_attribute(Attribute::Bold).fg(Color::DarkGrey),
            ]);
            for a in storage.archive_list()? {
                table.add_row(vec![
                    a.task.id.chars().take(8).collect::<String>(),
                    a.task.title.clone(),
                    a.task.context_id.clone(),
                    a.task.status.clone(),
                    short_time(&a.archived_at),
                ]);
            }
            println!("{table}");
        }
        ArchiveCommand::Add { task_ids } => {
            let ids = resolve_all(board.tasks.iter(), &task_ids)?;
            let n = storage.archive_tasks(&ids)?;
            println!("Archived {n} task(s).");
        }
        ArchiveCommand::Done { status, context } => {
            if !board.columns.iter().any(|c| c.id == status) {
                bail!("Column '{status}' does not exist.");
            }
            let ids: Vec<String> = board
                .tasks
                .iter()
                .filter(|t| t.status == status && context.as_ref().is_none_or(|c| &t.context_id == c))
                .map(|t| t.id.clone())
                .collect();
            if ids.is_empty() {
                println!("Nothing to archive.");
            } else {
                let n = storage.archive_tasks(&ids)?;
                println!("Archived {n} task(s).");
            }
        }
        ArchiveCommand::Restore { task_ids } => {
            let archive = storage.archive_list()?;
            let ids = resolve_all(archive.iter().map(|a| &a.task), &task_ids)?;
            let n = storage.archive_restore(&ids)?;
            println!("Restored {n} task(s).");
        }
    }
    Ok(())
}

/// Resolves every prefix to one task id (see [`resolve_prefix`]), dropping
/// duplicates. Fails on the first bad prefix, before anything is changed.
fn resolve_all<'a>(
    tasks: impl Iterator<Item = &'a crate::models::Task> + Clone,
    prefixes: &[String],
) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for p in prefixes {
        let id = resolve_prefix(tasks.clone(), p)?;
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    Ok(ids)
}
