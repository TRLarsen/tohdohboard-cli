use crate::models::{TohdohBoardData, Context, Column};
use anyhow::{Context as AnyhowContext, Result};
use std::fs;
use std::path::PathBuf;
use std::env;

pub struct Storage {
    file_path: Option<PathBuf>,
    remote_url: Option<String>,
}

impl Storage {
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
