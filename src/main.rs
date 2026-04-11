mod models;
mod storage;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use models::{Task, TaskStatus};
use storage::Storage;
use uuid::Uuid;
use chrono::Utc;
use comfy_table::{Table, Color, Cell, Attribute};

#[derive(Parser)]
#[command(name = "omni")]
#[command(about = "A native Rust terminal task manager for OmniBoard", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a task
    Add {
        title: String,
        #[arg(long)]
        context: String,
        #[arg(long, default_value = "To Do")]
        status: TaskStatus,
        description: Option<String>,
    },
    /// List tasks
    List {
        #[arg(long)]
        context: Option<String>,
        #[arg(long)]
        status: Option<TaskStatus>,
    },
    /// Move a task
    Move {
        task_id: String,
        #[arg(long)]
        status: Option<TaskStatus>,
        #[arg(long)]
        context: Option<String>,
    },
    /// Delete a task
    Delete {
        task_id: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let storage = Storage::new()?;
    let mut data = storage.load()?;

    match cli.command {
        Commands::Add { title, context, status, description } => {
            // Verify context exists
            if !data.contexts.iter().any(|c| c.id == context) {
                bail!("Context '{}' does not exist.", context);
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

            let filtered_tasks = data.tasks.iter().filter(|t| {
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
                    task.status.to_string(),
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
                        task.status = s;
                    }
                    if let Some(c) = context.clone() {
                        // Verify context exists
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
    }

    Ok(())
}
