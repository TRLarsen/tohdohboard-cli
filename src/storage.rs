//! Persistence for the board document.
//!
//! [`Storage`] hides where the board lives. It is chosen once, in
//! [`Storage::new`], from the environment:
//!
//! * **Remote mode**: if `TOHDOH_URL` is set (even to an empty string), the
//!   board is read with `GET` and written with `PUT` on that URL, which must be
//!   the backend's `/api/data` endpoint. Nothing is stored locally and no
//!   timeout is configured.
//! * **Local mode**: otherwise the board is the JSON file
//!   `~/.tohdohboard/data.json`; the directory is created on demand.
//!
//! Every command loads the whole document and, if it changed anything, saves
//! the whole document back; there is no locking or merging, so the last writer
//! wins.

use crate::models::{TohdohBoardData, Context, Column};
use anyhow::{Context as AnyhowContext, Result};
use std::fs;
use std::path::PathBuf;
use std::env;

/// Handle to wherever the board document is stored.
///
/// Exactly one of the two fields is `Some`; [`Storage::new`] guarantees this.
pub struct Storage {
    /// Path of the local JSON file; `Some` in local mode, `None` in remote mode.
    file_path: Option<PathBuf>,
    /// Value of `TOHDOH_URL`; `Some` in remote mode, `None` in local mode.
    remote_url: Option<String>,
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
        let remote_url = env::var("TOHDOH_URL").ok();
        
        if remote_url.is_some() {
            return Ok(Self {
                file_path: None,
                remote_url,
            });
        }

        let home_dir = dirs::home_dir().context("Could not find home directory")?;
        let config_dir = home_dir.join(".tohdohboard");
        
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).context("Could not create config directory")?;
        }
        
        Ok(Self {
            file_path: Some(config_dir.join("data.json")),
            remote_url: None,
        })
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
        if let Some(url) = &self.remote_url {
            let resp = reqwest::blocking::get(url)
                .context("Failed to fetch data from remote server")?;
            if !resp.status().is_success() {
                anyhow::bail!("Remote server returned error: {}", resp.status());
            }
            let data: TohdohBoardData = resp.json().context("Failed to parse JSON from remote server")?;
            return Ok(data);
        }

        let file_path = self.file_path.as_ref().unwrap();
        if !file_path.exists() {
            let default_data = TohdohBoardData {
                tasks: vec![],
                contexts: vec![
                    Context { id: "work".to_string(), name: "Work".to_string(), position: 0 },
                    Context { id: "school".to_string(), name: "School".to_string(), position: 1 },
                    Context { id: "projects".to_string(), name: "Projects".to_string(), position: 2 },
                ],
                columns: vec![
                    Column { id: "todo".to_string(), name: "To Do".to_string(), position: 0 },
                    Column { id: "doing".to_string(), name: "Doing".to_string(), position: 1 },
                    Column { id: "done".to_string(), name: "Done".to_string(), position: 2 },
                ],
            };
            self.save(&default_data)?;
            return Ok(default_data);
        }

        let content = fs::read_to_string(file_path).context("Could not read data file")?;
        let data: TohdohBoardData = serde_json::from_str(&content).context("Could not parse data file")?;
        Ok(data)
    }

    /// Writes the whole board document, replacing what was there.
    ///
    /// In remote mode this is a blocking `PUT` of the JSON document to
    /// `TOHDOH_URL`. In local mode the data file is overwritten in place with
    /// pretty-printed JSON (not an atomic write).
    ///
    /// # Errors
    ///
    /// Fails on network errors, a non-success HTTP status, or (local mode)
    /// serialization and I/O errors.
    pub fn save(&self, data: &TohdohBoardData) -> Result<()> {
        if let Some(url) = &self.remote_url {
            let client = reqwest::blocking::Client::new();
            let resp = client.put(url)
                .json(data)
                .send()
                .context("Failed to save data to remote server")?;
            if !resp.status().is_success() {
                anyhow::bail!("Remote server returned error: {}", resp.status());
            }
            return Ok(());
        }

        let file_path = self.file_path.as_ref().unwrap();
        let content = serde_json::to_string_pretty(data).context("Could not serialize data")?;
        fs::write(file_path, content).context("Could not write data file")?;
        Ok(())
    }
}
