//! Data model for a TohdohBoard document.
//!
//! These types mirror the JSON "board document" shared with the backend and the
//! web app: `{ tasks, contexts, columns }`. Task, context and column fields are
//! serialized in `camelCase`, so the file on disk (or the body of
//! `GET`/`PUT /api/data`) is interchangeable with the web app's data.
//!
//! Unknown JSON fields are preserved: every type carries an `extra` map
//! (`#[serde(flatten)]`) that captures fields this version does not know about
//! and writes them back unchanged, so the CLI never strips data that another
//! client (or a newer version of the backend) added.
//!
//! Loading is deliberately lenient and saving faithful: ids and timestamps are
//! kept as the strings they were stored as (never normalised), a missing
//! `description` or `position` is tolerated, and positions may be any JSON
//! number. One odd value therefore cannot make the board unloadable, and the
//! CLI only rewrites what a command actually changes.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

/// The current time as the RFC 3339 UTC string the CLI writes into timestamps.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Short form of a stored timestamp for tables (`2026-01-02 03:04`, UTC), or
/// the stored text unchanged if it is not valid RFC 3339.
pub fn short_time(stored: &str) -> String {
    DateTime::parse_from_rfc3339(stored)
        .map(|t| t.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| stored.to_string())
}

/// Numeric value of an optional `position`; a missing one counts as 0.
pub fn position_of(position: &Option<Number>) -> f64 {
    position.as_ref().and_then(Number::as_f64).unwrap_or(0.0)
}

/// A single task (a card on the board).
///
/// A task sits in one [`Context`] (swimlane, by id) and one [`Column`] (status,
/// by id). Neither reference is validated on load.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Unique identifier, kept exactly as stored (new tasks get a UUID v4).
    /// Commands accept any leading prefix, ignoring case; `list` shows the
    /// first 8 characters.
    pub id: String,
    /// Short, one-line summary shown in the board and in `list`.
    pub title: String,
    /// Free-form details; empty string when none were given or the field is missing.
    #[serde(default)]
    pub description: String,
    /// Id of the [`Column`] the task is in (for example `todo`), not its display name.
    pub status: String,
    /// Id of the [`Context`] (swimlane) the task belongs to (for example `work`).
    ///
    /// Serialized as `contextId`.
    pub context_id: String,
    /// When the task was created, as stored; the CLI writes RFC 3339 UTC.
    /// Serialized as `createdAt`.
    pub created_at: String,
    /// When the task was last changed, as stored; the CLI writes RFC 3339 UTC.
    /// Serialized as `updatedAt`.
    pub updated_at: String,
    /// Optional due date, passed through exactly as stored (the CLI writes
    /// `YYYY-MM-DD`). Serialized as `dueDate`; omitted when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// Fields this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A task that has been moved to the archive: a [`Task`] plus when that happened.
///
/// This is the element type of the server's `GET /api/archive` and of the
/// local `archive.json`; the task fields sit at the top level next to
/// `archivedAt`.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedTask {
    /// The task as it was when archived.
    #[serde(flatten)]
    pub task: Task,
    /// When the task was archived, as stored (RFC 3339 UTC when written by the server or CLI).
    pub archived_at: String,
}

/// The archive document: `{ "tasks": [ArchivedTask] }`, as in `archive.json`.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ArchiveData {
    /// Archived tasks in the order stored; no particular order is guaranteed
    /// (`archive list` sorts them for display).
    pub tasks: Vec<ArchivedTask>,
    /// Fields this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Sort key: contexts are ordered by ascending position (see [`position_of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Number>,
    /// Fields this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Sort key: columns are ordered by ascending position (see [`position_of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Number>,
    /// Fields this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Top-level fields this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "tasks": [{
                "id": "1a2b3c4d-0000-4000-8000-000000000001",
                "title": "t", "description": "", "status": "todo", "contextId": "work",
                "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
                "priority": 3, "labels": ["a", "b"]
            }],
            "contexts": [{ "id": "work", "name": "Work", "position": 0, "color": "#fff" }],
            "columns": [{ "id": "todo", "name": "To Do", "position": 0, "wip": 5 }],
            "version": 2
        })
    }

    #[test]
    fn unknown_fields_round_trip() {
        let original = sample();
        let board: TohdohBoardData = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(board.extra["version"], 2);
        assert_eq!(board.tasks[0].extra["priority"], 3);
        assert_eq!(board.contexts[0].extra["color"], "#fff");
        assert_eq!(board.columns[0].extra["wip"], 5);
        assert_eq!(serde_json::to_value(&board).unwrap(), original);
    }

    #[test]
    fn due_date_is_passed_through_and_omitted_when_unset() {
        let mut doc = sample();
        doc["tasks"][0]["dueDate"] = json!("next tuesday");
        let board: TohdohBoardData = serde_json::from_value(doc.clone()).unwrap();
        assert_eq!(board.tasks[0].due_date.as_deref(), Some("next tuesday"));
        assert_eq!(serde_json::to_value(&board).unwrap(), doc);

        let board: TohdohBoardData = serde_json::from_value(sample()).unwrap();
        assert!(board.tasks[0].due_date.is_none());
        assert!(!serde_json::to_value(&board).unwrap()["tasks"][0].as_object().unwrap().contains_key("dueDate"));
    }

    #[test]
    fn archived_task_round_trips_with_unknown_fields() {
        let mut task = sample()["tasks"][0].clone();
        task["archivedAt"] = json!("2026-02-01T00:00:00Z");
        let doc = json!({ "tasks": [task], "note": "x" });
        let archive: ArchiveData = serde_json::from_value(doc.clone()).unwrap();
        assert_eq!(archive.tasks[0].task.extra["priority"], 3);
        assert!(!archive.tasks[0].task.extra.contains_key("archivedAt"));
        assert_eq!(serde_json::to_value(&archive).unwrap(), doc);
    }

    #[test]
    fn odd_values_load_and_are_written_back_unchanged() {
        let doc = json!({
            "tasks": [
                { "id": "task-123", "title": "a", "status": "todo", "contextId": "work",
                  "createdAt": "yesterday", "updatedAt": "2026-01-02 03:04" },
                { "id": "1A2B3C4D-0000-4000-8000-00000000000A", "title": "b", "description": "",
                  "status": "todo", "contextId": "work",
                  "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z" }
            ],
            "contexts": [{ "id": "work", "name": "Work", "position": 1.5 }],
            "columns": [{ "id": "todo", "name": "To Do" }]
        });
        let board: TohdohBoardData = serde_json::from_value(doc.clone()).unwrap();
        assert_eq!(board.tasks[0].description, "");
        assert_eq!(position_of(&board.contexts[0].position), 1.5);
        assert_eq!(position_of(&board.columns[0].position), 0.0);
        let out = serde_json::to_value(&board).unwrap();
        // Only the missing description is materialised; everything else is untouched.
        assert_eq!(out["tasks"][0]["id"], "task-123");
        assert_eq!(out["tasks"][0]["createdAt"], "yesterday");
        assert_eq!(out["tasks"][0]["updatedAt"], "2026-01-02 03:04");
        assert_eq!(out["tasks"][1]["id"], "1A2B3C4D-0000-4000-8000-00000000000A");
        assert_eq!(out["contexts"], doc["contexts"]);
        assert_eq!(out["columns"], doc["columns"]);
    }

    #[test]
    fn short_time_formats_or_falls_back() {
        assert_eq!(short_time("2026-01-02T03:04:05+02:00"), "2026-01-02 01:04");
        assert_eq!(short_time("yesterday"), "yesterday");
        assert!(DateTime::parse_from_rfc3339(&now()).is_ok());
    }
}
