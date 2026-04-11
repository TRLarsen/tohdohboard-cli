use crate::models::{OmniBoardData, Context};
use anyhow::{Context as AnyhowContext, Result};
use std::fs;
use std::path::PathBuf;

pub struct Storage {
    file_path: PathBuf,
}

impl Storage {
    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir().context("Could not find home directory")?;
        let config_dir = home_dir.join(".omniboard");
        
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).context("Could not create config directory")?;
        }
        
        Ok(Self {
            file_path: config_dir.join("data.json"),
        })
    }

    pub fn load(&self) -> Result<OmniBoardData> {
        if !self.file_path.exists() {
            let default_data = OmniBoardData {
                tasks: vec![],
                contexts: vec![
                    Context { id: "work".to_string(), name: "Work".to_string(), order: 1 },
                    Context { id: "school".to_string(), name: "School".to_string(), order: 2 },
                    Context { id: "projects".to_string(), name: "Projects".to_string(), order: 3 },
                ],
            };
            self.save(&default_data)?;
            return Ok(default_data);
        }

        let content = fs::read_to_string(&self.file_path).context("Could not read data file")?;
        let data: OmniBoardData = serde_json::from_str(&content).context("Could not parse data file")?;
        Ok(data)
    }

    pub fn save(&self, data: &OmniBoardData) -> Result<()> {
        let content = serde_json::to_string_pretty(data).context("Could not serialize data")?;
        fs::write(&self.file_path, content).context("Could not write data file")?;
        Ok(())
    }
}
