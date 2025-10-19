use crate::error::{AppError, Result};
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub user_agent: String,
    pub cookies: Vec<String>,
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::Config(format!("Config file not found at: {}", path.display()))
            } else {
                AppError::Io(e)
            }
        })?;

        let reader = BufReader::new(file);
        let config = serde_json::from_reader(reader)?;
        Ok(config)
    }

    pub fn cookie_header(&self) -> String {
        self.cookies.join("; ")
    }
}
