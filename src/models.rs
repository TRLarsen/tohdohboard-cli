//! Data model for a TohdohBoard document.
//!
//! These types mirror the JSON "board document" shared with the backend and the
//! web app: `{ tasks, contexts, columns }`. Task, context and column fields are
//! serialized in `camelCase`, so the file on disk (or the body of
//! `GET`/`PUT /api/data`) is interchangeable with the web app's data.
//!
//! Only fields listed here are kept: unknown JSON fields are ignored on load
//! and therefore not written back on save.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A single task (a card on the board).
///
/// A task sits in one [`Context`] (swimlane, by id) and one [`Column`] (status,
/// by id). Neither reference is validated on load.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Unique identifier. Commands accept any leading prefix of its hyphenated
    /// string form (the `list` command shows the first 8 characters).
    pub id: Uuid,
    /// Short, one-line summary shown in the board and in `list`.
    pub title: String,
    /// Free-form details; empty string when none were given.
    pub description: String,
    /// Id of the [`Column`] the task is in (for example `todo`), not its display name.
    pub status: String,
    /// Id of the [`Context`] (swimlane) the task belongs to (for example `work`).
    ///
    /// Serialized as `contextId`.
    pub context_id: String,
    /// When the task was created (UTC, RFC 3339 in JSON). Serialized as `createdAt`.
    pub created_at: DateTime<Utc>,
    /// When the task was last changed (UTC, RFC 3339 in JSON). Serialized as `updatedAt`.
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
/// A context (swimlane): a horizontal grouping of tasks such as "Work".
pub struct Context {
    /// Stable identifier referenced by [`Task::context_id`]. Created contexts use
    /// the lowercased name with spaces replaced by `-`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Sort key: contexts are ordered by ascending position.
    pub position: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
/// A column (status) of the board, such as "To Do".
pub struct Column {
    /// Stable identifier referenced by [`Task::status`]. Created columns use the
    /// lowercased name with spaces replaced by `-`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Sort key: columns are ordered by ascending position.
    pub position: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
/// The whole board document: everything the CLI reads and writes in one go.
///
/// Unlike the other types it has no `camelCase` rename, as all its fields are
/// single words. Always loaded and saved as a unit.
pub struct TohdohBoardData {
    /// All tasks, in insertion order.
    pub tasks: Vec<Task>,
    /// All contexts (swimlanes).
    pub contexts: Vec<Context>,
    /// All columns (statuses).
    pub columns: Vec<Column>,
}
