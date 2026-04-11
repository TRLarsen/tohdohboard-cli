use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, clap::ValueEnum)]
pub enum TaskStatus {
    #[serde(rename = "To Do")]
    ToDo,
    Doing,
    #[serde(rename = "High Priority")]
    HighPriority,
    Done,
    Blocked,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::ToDo => write!(f, "To Do"),
            TaskStatus::Doing => write!(f, "Doing"),
            TaskStatus::HighPriority => write!(f, "High Priority"),
            TaskStatus::Done => write!(f, "Done"),
            TaskStatus::Blocked => write!(f, "Blocked"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub context_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub id: String,
    pub name: String,
    pub order: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct OmniBoardData {
    pub tasks: Vec<Task>,
    pub contexts: Vec<Context>,
}
