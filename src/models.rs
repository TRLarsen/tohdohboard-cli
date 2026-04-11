use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub status: String,
    pub context_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub id: String,
    pub name: String,
    pub position: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub id: String,
    pub name: String,
    pub position: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TohdohBoardData {
    pub tasks: Vec<Task>,
    pub contexts: Vec<Context>,
    pub columns: Vec<Column>,
}
