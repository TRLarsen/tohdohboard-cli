//! Persistence for the board document and its archive.
//!
//! [`Storage`] hides where the data lives. The backend is chosen once, in
//! [`Storage::new`], from the environment:
//!
//! * **Remote mode**: if `TOHDOH_URL` is set and non-empty, the board is read
//!   with `GET` and written with `PUT` on that URL, which must be the backend's
//!   `/api/data` endpoint. The archive endpoints are derived from it by
//!   replacing the trailing `/data` (see [`sibling_url`]); trailing slashes on
//!   the URL are ignored. Nothing is stored locally. Redirects are never
//!   followed (a redirected `PUT` could be silently turned into a `GET`), they
//!   are reported as errors. URL credentials are never echoed. If `TOHDOH_TOKEN` is set it is sent as `Authorization: Bearer` on
//!   every request (and never printed); requests time out after 10 s to
//!   connect and 30 s in total.
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
use std::time::Duration;

/// How long to wait for a TCP connection to the server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a whole request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Handle to wherever the board document is stored.
pub struct Storage {
    /// Where the data lives.
    backend: Backend,
}

/// The two places a board can live; chosen once by [`Storage::new`].
enum Backend {
    /// A directory holding `data.json` and `archive.json`.
    Local(PathBuf),
    /// The server's `/api/data` URL and a client configured for it.
    Remote {
        /// Value of `TOHDOH_URL`.
        url: String,
        /// HTTP client with timeouts and (if configured) the bearer token.
        client: reqwest::blocking::Client,
    },
}

/// Returns `value` unless it is unset or only whitespace: an empty environment
/// variable counts as not set.
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// Builds the HTTP client: connect/total timeouts, no redirect following, and `Authorization: Bearer
/// <token>` on every request if a token is given. The header is marked
/// sensitive so it is redacted from debug output.
///
/// # Errors
///
/// Fails if the token is not a valid header value (the error does not contain
/// the token).
fn build_client(token: Option<&str>) -> Result<reqwest::blocking::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(token) = token {
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.trim()))
            .map_err(|_| anyhow::anyhow!("TOHDOH_TOKEN contains characters that are not allowed in an HTTP header."))?;
        value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .default_headers(headers)
        .build()
        .context("Could not create HTTP client")
}

/// `url` without any `user:password@` part, for use in messages; unparseable
/// input is not echoed at all.
fn redact(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.to_string()
        }
        Err(_) => "<invalid URL>".into(),
    }
}

/// Sends `request` (aimed at `url`, which is only used in error messages) and
/// returns the response if it is a success.
///
/// # Errors
///
/// * the request fails (the URL is shown without credentials);
/// * a 3xx answer: redirects are not followed, so the error names the
///   `Location` to put in `TOHDOH_URL` instead;
/// * a 401, with a hint that depends on whether the server sent
///   `WWW-Authenticate: Bearer` (token mode) or not (proxy mode);
/// * any other non-success status.
fn send(request: reqwest::blocking::RequestBuilder, url: &str) -> Result<reqwest::blocking::Response> {
    use reqwest::header::{LOCATION, WWW_AUTHENTICATE};
    let resp = request
        .send()
        .map_err(reqwest::Error::without_url)
        .with_context(|| format!("Could not reach {}", redact(url)))?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if status.is_redirection() {
        let to = resp
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|l| resp.url().join(l).ok())
            .map_or("an unknown location".to_string(), |u| redact(u.as_str()));
        bail!("Remote server redirected ({status}) to {to}. Redirects are not followed, as that could drop a write; set TOHDOH_URL to that address.");
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        let bearer = resp
            .headers()
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .any(|v| v.to_str().is_ok_and(|v| v.trim_start().to_ascii_lowercase().starts_with("bearer")));
        bail!(if bearer {
            "Remote server returned 401 Unauthorized: set TOHDOH_TOKEN to a valid access token."
        } else {
            "Remote server returned 401 Unauthorized: it expects an identity from its proxy; reach it through the proxy (for example `tailscale serve`)."
        });
    }
    bail!("Remote server returned error: {status}")
}

/// Derives another API endpoint from the `/api/data` URL by replacing its
/// trailing `/data` with `/<name>`: `http://h:3001/api/data` + `archive`
/// gives `http://h:3001/api/archive`. Trailing slashes are tolerated.
///
/// # Errors
///
/// Fails if the URL is invalid, has a query string or fragment, or does not
/// end in `/data`. Messages never include URL credentials.
pub fn sibling_url(data_url: &str, name: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(data_url).map_err(|e| anyhow::anyhow!("TOHDOH_URL is not a valid URL: {e}"))?;
    if url.query().is_some() || url.fragment().is_some() {
        bail!("TOHDOH_URL has a query string or fragment, so the /{name} endpoint cannot be derived from it.");
    }
    let Some(base) = url.path().trim_end_matches('/').strip_suffix("/data") else {
        bail!("TOHDOH_URL must end in /data (got '{}'), cannot derive the /{name} endpoint.", redact(data_url));
    };
    let path = format!("{base}/{name}");
    url.set_path(&path);
    Ok(url.into())
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
    /// If `TOHDOH_URL` is set and non-empty, remote mode is used and nothing
    /// else is touched.
    /// Otherwise local mode is used and `~/.tohdohboard` is created if missing.
    /// The data file itself is not created here (see [`Storage::load`]).
    ///
    /// # Errors
    ///
    /// Fails if `TOHDOH_TOKEN` is not a valid header value, if the home
    /// directory cannot be determined, or if the data directory cannot be
    /// created.
    pub fn new() -> Result<Self> {
        if let Some(url) = non_empty(env::var("TOHDOH_URL").ok()) {
            let url = url.trim().trim_end_matches('/').to_string();
            let client = build_client(non_empty(env::var("TOHDOH_TOKEN").ok()).as_deref())?;
            return Ok(Self { backend: Backend::Remote { url, client } });
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
            Backend::Remote { url, client } => get_json(client, url, "board"),
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
            Backend::Remote { url, client } => {
                send(client.put(url).json(data), url)?;
                Ok(())
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
            Backend::Remote { url, client } => {
                let archive: ArchiveData = get_json(client, &sibling_url(url, "archive")?, "archive")?;
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
            Backend::Remote { url, client } => post_ids(client, &sibling_url(url, "archive")?, ids, "archived"),
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
            Backend::Remote { url, client } => {
                post_ids(client, &sibling_url(url, "archive/restore")?, ids, "restored")
            }
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

/// Blocking `GET` of `url`, parsed as JSON. `what` names the document in errors.
fn get_json<T: DeserializeOwned>(client: &reqwest::blocking::Client, url: &str, what: &str) -> Result<T> {
    send(client.get(url), url)?
        .json()
        .map_err(reqwest::Error::without_url)
        .with_context(|| format!("Failed to parse {what} JSON from remote server"))
}

/// `POST {"taskIds": ids}` to `url` and reads the count under `key` from the
/// JSON reply (`{"archived": n}` / `{"restored": n}`).
fn post_ids(client: &reqwest::blocking::Client, url: &str, ids: &[String], key: &str) -> Result<usize> {
    let resp = send(client.post(url).json(&serde_json::json!({ "taskIds": ids })), url)?;
    let reply: serde_json::Value = resp
        .json()
        .map_err(reqwest::Error::without_url)
        .context("Failed to parse reply from remote server")?;
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
            due_date: None,
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
    fn sibling_url_rejects_queries_and_hides_credentials() {
        let err = sibling_url("http://h/api/data?board=1", "archive").unwrap_err().to_string();
        assert!(err.contains("query string"), "{err}");
        let err = sibling_url("http://alice:hunter2@h/board", "archive").unwrap_err().to_string();
        assert!(err.contains("must end in /data") && !err.contains("hunter2"), "{err}");
        assert!(!redact("http://alice:hunter2@h/x").contains("hunter2"));
        assert!(!redact("not a url hunter2").contains("hunter2"));
    }

    #[test]
    fn empty_env_values_count_as_unset() {
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some(String::new())), None);
        assert_eq!(non_empty(Some("  ".into())), None);
        assert_eq!(non_empty(Some("http://x/api/data".into())).as_deref(), Some("http://x/api/data"));
    }

    #[test]
    fn invalid_token_error_does_not_leak_it() {
        let err = build_client(Some("sekrit\nvalue")).unwrap_err().to_string();
        assert!(!err.contains("sekrit"), "{err}");
    }

    /// Answers one request with `status` (for example `"200 OK"`), extra raw
    /// `headers` and a JSON `body`; returns the URL and the raw request seen.
    fn serve_once(status: &'static str, headers: &'static str, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/data", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).unwrap();
            let resp = format!(
                "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        (url, handle)
    }

    const EMPTY_BOARD: &str = r#"{"tasks":[],"contexts":[],"columns":[]}"#;

    /// The error from a `PUT` answered with `status` and `headers`.
    fn put_error(status: &'static str, headers: &'static str) -> String {
        let (url, server) = serve_once(status, headers, "");
        let url = url.replace("http://", "http://alice:hunter2@");
        let client = build_client(None).unwrap();
        let err = format!("{:#}", send(client.put(&url), &url).unwrap_err());
        server.join().unwrap();
        assert!(!err.contains("hunter2"), "{err}");
        err
    }

    #[test]
    fn bearer_token_is_sent_on_requests() {
        let (url, server) = serve_once("200 OK", "", EMPTY_BOARD);
        let client = build_client(Some("s3cret")).unwrap();
        let board: TohdohBoardData = get_json(&client, &url, "board").unwrap();
        assert!(board.tasks.is_empty());
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(request.contains("authorization: bearer s3cret"), "{request}");
    }

    #[test]
    fn no_authorization_header_without_token() {
        let (url, server) = serve_once("200 OK", "", EMPTY_BOARD);
        let client = build_client(None).unwrap();
        let _: TohdohBoardData = get_json(&client, &url, "board").unwrap();
        assert!(!server.join().unwrap().to_ascii_lowercase().contains("authorization"));
    }

    #[test]
    fn redirects_are_errors_naming_the_location() {
        let err = put_error("303 See Other", "Location: /elsewhere/data\r\n");
        assert!(err.contains("303") && err.contains("/elsewhere/data"), "{err}");
    }

    #[test]
    fn unauthorized_hint_depends_on_the_challenge() {
        let err = put_error("401 Unauthorized", "WWW-Authenticate: Bearer\r\n");
        assert!(err.contains("TOHDOH_TOKEN"), "{err}");
        let err = put_error("401 Unauthorized", "");
        assert!(!err.contains("TOHDOH_TOKEN") && err.contains("proxy"), "{err}");
        assert!(put_error("404 Not Found", "").contains("404"));
    }

    #[test]
    fn connection_errors_do_not_leak_credentials() {
        let client = build_client(None).unwrap();
        let url = "http://alice:hunter2@127.0.0.1:1/api/data";
        let err = format!("{:#}", send(client.get(url), url).unwrap_err());
        assert!(err.contains("Could not reach") && !err.contains("hunter2"), "{err}");
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
