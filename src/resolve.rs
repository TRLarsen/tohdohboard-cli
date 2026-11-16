//! Resolving a user-typed task id prefix to exactly one task.
//!
//! Commands that change or remove tasks go through [`resolve_prefix`], so a
//! short or empty prefix can never silently act on the wrong (or on every) task.

use crate::models::Task;
use anyhow::{Result, bail};

/// Most candidates listed in an "ambiguous prefix" error.
const MAX_LISTED: usize = 10;

/// Finds the one task whose id starts with `prefix` (case-insensitive) and
/// returns its id as stored. An id equal to `prefix` wins over longer ids that
/// merely start with it.
///
/// # Errors
///
/// * `prefix` is empty (it would match everything);
/// * no task matches;
/// * several match: the error lists up to ten candidates (short id and title)
///   so the caller can retype a longer prefix.
pub fn resolve_prefix<'a>(
    tasks: impl IntoIterator<Item = &'a Task>,
    prefix: &str,
) -> Result<String> {
    if prefix.is_empty() {
        bail!("Task ID prefix must not be empty.");
    }
    let needle = prefix.to_ascii_lowercase();
    let matches: Vec<&Task> = tasks
        .into_iter()
        .filter(|t| t.id.to_ascii_lowercase().starts_with(&needle))
        .collect();
    if let [only] = matches
        .iter()
        .filter(|t| t.id.eq_ignore_ascii_case(prefix))
        .collect::<Vec<_>>()
        .as_slice()
    {
        return Ok(only.id.clone());
    }
    match matches.as_slice() {
        [] => bail!("Task with ID starting with '{prefix}' not found."),
        [only] => Ok(only.id.clone()),
        many => {
            let mut msg = format!(
                "ID prefix '{prefix}' is ambiguous, it matches {} tasks:",
                many.len()
            );
            for t in many.iter().take(MAX_LISTED) {
                let short: String = t.id.chars().take(8).collect();
                msg.push_str(&format!("\n  {short}  {}", t.title));
            }
            if many.len() > MAX_LISTED {
                msg.push_str(&format!("\n  ... and {} more", many.len() - MAX_LISTED));
            }
            bail!(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, title: &str) -> Task {
        Task {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            status: "todo".into(),
            context_id: "work".into(),
            created_at: String::new(),
            updated_at: String::new(),
            extra: Default::default(),
        }
    }

    fn board() -> Vec<Task> {
        vec![
            task("abc00000-0000-4000-8000-000000000001", "first"),
            task("abd00000-0000-4000-8000-000000000002", "second"),
            task("f0000000-0000-4000-8000-000000000003", "third"),
        ]
    }

    #[test]
    fn unique_prefix_resolves() {
        let tasks = board();
        assert_eq!(resolve_prefix(&tasks, "abc").unwrap(), tasks[0].id);
        assert_eq!(resolve_prefix(&tasks, "F0").unwrap(), tasks[2].id);
        assert_eq!(resolve_prefix(&tasks, &tasks[1].id).unwrap(), tasks[1].id);
    }

    #[test]
    fn odd_ids_match_case_insensitively_and_exact_wins() {
        let tasks = vec![task("Task-1", "one"), task("task-12", "twelve")];
        assert_eq!(resolve_prefix(&tasks, "task-1").unwrap(), "Task-1");
        assert_eq!(resolve_prefix(&tasks, "TASK-12").unwrap(), "task-12");
        assert!(resolve_prefix(&tasks, "task").is_err());
    }

    #[test]
    fn empty_prefix_is_rejected() {
        let err = resolve_prefix(&board(), "").unwrap_err().to_string();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn missing_prefix_is_rejected() {
        let err = resolve_prefix(&board(), "zz").unwrap_err().to_string();
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn ambiguous_prefix_lists_candidates() {
        let err = resolve_prefix(&board(), "ab").unwrap_err().to_string();
        assert!(err.contains("ambiguous"), "{err}");
        assert!(err.contains("abc00000  first") && err.contains("abd00000  second"), "{err}");
        assert!(!err.contains("third"), "{err}");
    }
}
