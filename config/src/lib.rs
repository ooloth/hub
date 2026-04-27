pub mod toml;

use anyhow::{Context, Result};

pub struct Config {
    pub github_token: String,
    pub projects: Vec<toml::Project>,
    pub monitor: Option<toml::Monitor>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        Ok(Self {
            github_token: std::env::var("GITHUB_TOKEN")
                .context("GITHUB_TOKEN not set")?,
            projects: hub_toml.project,
            monitor: hub_toml.monitor,
        })
    }
}
