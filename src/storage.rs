//! Persistence for the board document and its archive.
//!
//! [`Storage`] hides where the data lives. The backend is chosen once, in
//! [`Storage::new`], from the environment:
//!
//! * **Remote mode**: if `TOHDOH_URL` is set (even to an empty string), the
//!   board is read with `GET` and written with `PUT` on that URL, which must be
//!   the backend's `/api/data` endpoint. The archive endpoints are derived from
//!   it by replacing the trailing `/data` (see [`sibling_url`]). Nothing is
//!   stored locally and no timeout is configured.
//! * **Local mode**: otherwise the board is the JSON file
//!   `~/.tohdohboard/data.json` and the archive is `archive.json` next to it
//!   (the same format the server uses, so the two stay interchangeable); the
//!   directory is created on demand. All writes are atomic (temp file + rename).
//!
//! Every command loads the whole document and, if it changed anything, saves
//! the whole document back; there is no locking or merging, so the last writer
//! wins.

use crate::models::{ArchiveData, ArchivedTask, Column, Context, TohdohBoardData, now};
use anyhow::{Context as AnyhowContext, Result, bail};
use chrono::DateTime;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Handle to wherever the board document is stored.
pub struct Storage {
    /// Where the data lives.
    backend: Backend,
}

/// The two places a board can live; chosen once by [`Storage::new`].
enum Backend {
    /// A directory holding `data.json` and `archive.json`.
    Local(PathBuf),
    /// Value of `TOHDOH_URL`: the server's `/api/data` URL.
    Remote(String),
}

/// Derives another API endpoint from the `/api/data` URL by replacing its
/// trailing `/data` with `/<name>`: `http://h:3001/api/data` + `archive`
/// gives `http://h:3001/api/archive`. A trailing slash is tolerated.
///
/// # Errors
///
/// Fails if the URL does not end in `/data`.
pub fn sibling_url(data_url: &str, name: &str) -> Result<String> {
    match data_url.trim_end_matches('/').strip_suffix("/data") {
        Some(base) => Ok(format!("{base}/{name}")),
        None => bail!("TOHDOH_URL must end in /data (got '{data_url}'), cannot derive the /{name} endpoint."),
    }
}

/// The board a first run starts with: contexts `work`, `school`, `projects`
/// and columns `todo`, `doing`, `done`.
fn default_board() -> TohdohBoardData {
    let ctx = |id: &str, name: &str, position: i32| Context {
        id: id.into(),
        name: name.into(),
        position: Some(position.into()),
        extra: Default::default(),
    };
    let col = |id: &str, name: &str, position: i32| Column {
        id: id.into(),
        name: name.into(),
        position: Some(position.into()),
        extra: Default::default(),
    };
    TohdohBoardData {
        tasks: vec![],
        contexts: vec![ctx("work", "Work", 0), ctx("school", "School", 1), ctx("projects", "Projects", 2)],
        columns: vec![col("todo", "To Do", 0), col("doing", "Doing", 1), col("done", "Done", 2)],
        extra: Default::default(),
    }
}

/// Writes `value` as pretty JSON to `path` atomically: the content goes to a
/// temporary file in the same directory (mode 0600 on Unix, like the server's
/// files, and synced to disk) which is then renamed over `path`, so readers
/// never see a half-written file.
fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = serde_json::to_string_pretty(value).context("Could not serialize data")?;
    let mut tmp_name = path.file_name().context("Data path has no file name")?.to_owned();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(&tmp)
        .and_then(|mut f| f.write_all(content.as_bytes()).and_then(|()| f.sync_all()))
        .context("Could not write data file")?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    }).context("Could not replace data file")?;
    Ok(())
}

/// Reads JSON from `path`, or `None` if the file does not exist.
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Could not read {}", path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("Could not parse {}", path.display()))?;
    Ok(Some(value))
}

impl Storage {
    /// Selects the storage backend from the environment.
    ///
    /// If `TOHDOH_URL` is set, remote mode is used and nothing else is touched.
    /// Otherwise local mode is used and `~/.tohdohboard` is created if missing.
    /// The data file itself is not created here (see [`Storage::load`]).
    ///
    /// # Errors
    ///
    /// Fails if the home directory cannot be determined or the data directory
    /// cannot be created.
    pub fn new() -> Result<Self> {
        if let Ok(url) = env::var("TOHDOH_URL") {
            return Ok(Self { backend: Backend::Remote(url) });
        }

        let home_dir = dirs::home_dir().context("Could not find home directory")?;
        Self::local(home_dir.join(".tohdohboard"))
    }

    /// Local-mode storage in `dir`, which is created if missing.
    fn local(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).context("Could not create config directory")?;
        Ok(Self { backend: Backend::Local(dir) })
    }

    /// Reads the whole board document.
    ///
    /// In remote mode this is a blocking `GET` of `TOHDOH_URL`; a missing board is
    /// not seeded, whatever the server answers is used. In local mode, if the data
    /// file does not exist yet it is created with a default board (contexts `work`,
    /// `school`, `projects`; columns `todo`, `doing`, `done`; no tasks) and that
    /// default is returned.
    ///
    /// # Errors
    ///
    /// Fails on network errors, a non-success HTTP status, unreadable or
    /// unparseable JSON, or (local mode) I/O errors.
    pub fn load(&self) -> Result<TohdohBoardData> {
        match &self.backend {
            Backend::Remote(url) => get_json(url, "board"),
            Backend::Local(dir) => {
                let path = dir.join("data.json");
                match read_json(&path)? {
                    Some(data) => Ok(data),
                    None => {
                        let data = default_board();
                        write_json_atomic(&path, &data)?;
                        Ok(data)
                    }
                }
            }
        }
    }

    /// Writes the whole board document, replacing what was there.
    ///
    /// In remote mode this is a blocking `PUT` of the JSON document to
    /// `TOHDOH_URL`. In local mode the data file is replaced atomically with
    /// pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Fails on network errors, a non-success HTTP status, or (local mode)
    /// serialization and I/O errors.
    pub fn save(&self, data: &TohdohBoardData) -> Result<()> {
        match &self.backend {
            Backend::Remote(url) => {
                let resp = reqwest::blocking::Client::new()
                    .put(url)
                    .json(data)
                    .send()
                    .context("Failed to save data to remote server")?;
                check_status(&resp)
            }
            Backend::Local(dir) => write_json_atomic(&dir.join("data.json"), data),
        }
    }

    /// Lists archived tasks, newest archived first.
    ///
    /// Remote: `GET /api/archive`. Local: reads `archive.json` (empty if absent).
    ///
    /// # Errors
    ///
    /// Same failure modes as [`Storage::load`].
    pub fn archive_list(&self) -> Result<Vec<ArchivedTask>> {
        match &self.backend {
            Backend::Remote(url) => {
                let archive: ArchiveData = get_json(&sibling_url(url, "archive")?, "archive")?;
                Ok(archive.tasks)
            }
            Backend::Local(dir) => {
                let mut tasks = read_archive(dir)?.tasks;
                tasks.sort_by_key(|t| std::cmp::Reverse(DateTime::parse_from_rfc3339(&t.archived_at).ok()));
                Ok(tasks)
            }
        }
    }

    /// Moves the tasks with these ids from the board to the archive and
    /// returns how many were moved; unknown ids are ignored.
    ///
    /// Remote: `POST /api/archive`. Local: the archive file is written first and
    /// the board second, so an interrupted run duplicates a task rather than
    /// losing it.
    ///
    /// # Errors
    ///
    /// Network, HTTP-status, parse or I/O errors.
    pub fn archive_tasks(&self, ids: &[String]) -> Result<usize> {
        match &self.backend {
            Backend::Remote(url) => post_ids(&sibling_url(url, "archive")?, ids, "archived"),
            Backend::Local(dir) => {
                let wanted: HashSet<&String> = ids.iter().collect();
                let mut board = self.load()?;
                let mut archive = read_archive(dir)?;
                let now = now();
                let (moving, keep): (Vec<_>, Vec<_>) =
                    board.tasks.into_iter().partition(|t| wanted.contains(&t.id));
                board.tasks = keep;
                let n = moving.len();
                archive.tasks.retain(|a| !wanted.contains(&a.task.id));
                archive
                    .tasks
                    .extend(moving.into_iter().map(|task| ArchivedTask { task, archived_at: now.clone() }));
                write_json_atomic(&dir.join("archive.json"), &archive)?;
                write_json_atomic(&dir.join("data.json"), &board)?;
                Ok(n)
            }
        }
    }

    /// Moves the archived tasks with these ids back to the board (status and
    /// context preserved, `updatedAt` set to now) and returns how many were
    /// restored; unknown ids are ignored.
    ///
    /// Remote: `POST /api/archive/restore`. Local: the board is written first
    /// and the archive second (again duplicating rather than losing on a crash).
    ///
    /// # Errors
    ///
    /// Network, HTTP-status, parse or I/O errors.
    pub fn archive_restore(&self, ids: &[String]) -> Result<usize> {
        match &self.backend {
            Backend::Remote(url) => post_ids(&sibling_url(url, "archive/restore")?, ids, "restored"),
            Backend::Local(dir) => {
                let wanted: HashSet<&String> = ids.iter().collect();
                let mut board = self.load()?;
                let mut archive = read_archive(dir)?;
                let now = now();
                let (moving, keep): (Vec<_>, Vec<_>) =
                    archive.tasks.into_iter().partition(|a| wanted.contains(&a.task.id));
                archive.tasks = keep;
                let n = moving.len();
                for mut task in moving.into_iter().map(|a| a.task) {
                    task.updated_at = now.clone();
                    board.tasks.retain(|t| t.id != task.id);
                    board.tasks.push(task);
                }
                write_json_atomic(&dir.join("data.json"), &board)?;
                write_json_atomic(&dir.join("archive.json"), &archive)?;
                Ok(n)
            }
        }
    }
}

/// Reads `archive.json` from `dir`; a missing file is an empty archive.
fn read_archive(dir: &Path) -> Result<ArchiveData> {
    Ok(read_json(&dir.join("archive.json"))?.unwrap_or_default())
}

/// Fails with the HTTP status if `resp` is not a success.
fn check_status(resp: &reqwest::blocking::Response) -> Result<()> {
    if !resp.status().is_success() {
        bail!("Remote server returned error: {}", resp.status());
    }
    Ok(())
}

/// Blocking `GET` of `url`, parsed as JSON. `what` names the document in errors.
fn get_json<T: DeserializeOwned>(url: &str, what: &str) -> Result<T> {
    let resp = reqwest::blocking::get(url)
        .with_context(|| format!("Failed to fetch {what} from remote server"))?;
    check_status(&resp)?;
    resp.json()
        .with_context(|| format!("Failed to parse {what} JSON from remote server"))
}

/// `POST {"taskIds": ids}` to `url` and reads the count under `key` from the
/// JSON reply (`{"archived": n}` / `{"restored": n}`).
fn post_ids(url: &str, ids: &[String], key: &str) -> Result<usize> {
    let resp = reqwest::blocking::Client::new()
        .post(url)
        .json(&serde_json::json!({ "taskIds": ids }))
        .send()
        .context("Failed to reach remote server")?;
    check_status(&resp)?;
    let reply: serde_json::Value = resp.json().context("Failed to parse reply from remote server")?;
    reply[key]
        .as_u64()
        .map(|n| n as usize)
        .with_context(|| format!("Remote reply has no '{key}' count"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Task;
    use uuid::Uuid;

    /// A fresh empty directory under the system temp dir.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tohdoh-test-{tag}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn task(title: &str) -> Task {
        Task {
            id: Uuid::new_v4().to_string(),
            title: title.into(),
            description: String::new(),
            status: "done".into(),
            context_id: "work".into(),
            created_at: now(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            extra: Default::default(),
        }
    }

    #[test]
    fn sibling_url_replaces_trailing_data() {
        assert_eq!(sibling_url("http://h:3001/api/data", "archive").unwrap(), "http://h:3001/api/archive");
        assert_eq!(sibling_url("http://h/api/data/", "archive/restore").unwrap(), "http://h/api/archive/restore");
        assert!(sibling_url("http://h/board", "archive").is_err());
    }

    #[test]
    fn local_archive_and_restore_round_trip() {
        let dir = temp_dir("archive");
        let st = Storage::local(dir.clone()).unwrap();
        let mut board = st.load().unwrap();
        let (a, b) = (task("a"), task("b"));
        let a_updated = a.updated_at.clone();
        board.tasks = vec![a.clone(), b.clone()];
        st.save(&board).unwrap();

        assert_eq!(st.archive_tasks(&[a.id.clone(), Uuid::new_v4().to_string()]).unwrap(), 1);
        assert_eq!(st.load().unwrap().tasks.len(), 1);
        let listed = st.archive_list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].task.id, a.id);

        // On-disk format is the server's: { "tasks": [ task fields + archivedAt ] }.
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("archive.json")).unwrap()).unwrap();
        assert!(raw["tasks"][0]["archivedAt"].is_string());
        assert_eq!(raw["tasks"][0]["contextId"], "work");

        assert_eq!(st.archive_restore(std::slice::from_ref(&a.id)).unwrap(), 1);
        assert!(st.archive_list().unwrap().is_empty());
        let restored = st.load().unwrap();
        let back = restored.tasks.iter().find(|t| t.id == a.id).unwrap();
        assert_eq!((back.status.as_str(), back.context_id.as_str()), ("done", "work"));
        assert!(back.updated_at > a_updated);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_archive_preserves_unknown_fields() {
        let dir = temp_dir("extra");
        let st = Storage::local(dir.clone()).unwrap();
        let mut t = task("x");
        t.extra.insert("priority".into(), 7.into());
        let mut board = st.load().unwrap();
        board.tasks.push(t.clone());
        st.save(&board).unwrap();
        st.archive_tasks(&[t.id.clone()]).unwrap();
        assert_eq!(st.archive_list().unwrap()[0].task.extra["priority"], 7);
        st.archive_restore(&[t.id.clone()]).unwrap();
        assert_eq!(st.load().unwrap().tasks[0].extra["priority"], 7);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn written_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        let st = Storage::local(dir.clone()).unwrap();
        st.save(&default_board()).unwrap();
        let mode = fs::metadata(dir.join("data.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let st = Storage::local(dir.clone()).unwrap();
        st.save(&default_board()).unwrap();
        let names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("data.json")]);
        fs::remove_dir_all(dir).unwrap();
    }
}
