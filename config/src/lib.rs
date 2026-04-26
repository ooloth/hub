use anyhow::{Context, Result};

pub struct Config {
    pub github_token: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        Ok(Self {
            github_token: std::env::var("GITHUB_TOKEN")
                .context("GITHUB_TOKEN not set")?,
        })
    }
}
