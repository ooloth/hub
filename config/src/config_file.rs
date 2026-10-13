use std::collections::HashMap;

use anyhow::{Context, Result};
use domain::credential_failure::CredentialFailure;
use domain::known_secrets::KnownSecrets;
use secrecy::Secret;

use crate::resolved::Config;
use crate::toml;

/// `hub.toml`, read and checked, with its credentials still as written: plain values or `op://`
/// references nobody has asked 1Password about yet.
///
/// Reading it can fail only on the file itself. Resolving it can fail only on a credential, so a
/// caller can try [`ConfigFile::resolve`] again without ever retrying a broken file.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    credentials: toml::CredentialsToml,
    projects: Vec<toml::Project>,
    monitor: Option<toml::Monitor>,
    investigation_mcp_servers: domain::InvestigationMcpServers,
}

impl ConfigFile {
    /// Reads `hub.toml` from the working directory.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, or lacks a required credential.
    pub fn read() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        validate_required(&hub_toml.credentials)?;
        let investigation_mcp_servers = hub_toml.investigation.mcp_servers()?;
        Ok(Self {
            credentials: hub_toml.credentials,
            projects: hub_toml.project,
            monitor: hub_toml.monitor,
            investigation_mcp_servers,
        })
    }

    /// Resolves every credential, asking 1Password for each `op://` reference.
    ///
    /// # Errors
    /// Returns why the first credential that could not be resolved failed, redacted of every
    /// credential resolved before it.
    pub async fn resolve(&self) -> Result<Config, CredentialFailure> {
        let creds = &self.credentials;
        let mut so_far = ResolvedSoFar::default();
        let github_token = so_far.read(&creds.github_token).await?;
        let linear_token = match &creds.linear_token {
            Some(value) => Some(so_far.read(value).await?),
            None => None,
        };
        let loki_token = match &creds.loki_token {
            Some(value) => Some(so_far.read(value).await?),
            None => None,
        };
        let mut extra_credentials = HashMap::new();
        for (name, value) in &creds.extra {
            let _ = extra_credentials.insert(name.clone(), so_far.read(value).await?);
        }
        Ok(Config {
            github_token,
            github_username: creds.github_username.clone(),
            linear_token,
            loki_token,
            extra_credentials,
            projects: self.projects.clone(),
            monitor: self.monitor.clone(),
            investigation_mcp_servers: self.investigation_mcp_servers.clone(),
        })
    }
}

/// The credentials one call to [`ConfigFile::resolve`] has resolved so far, which a later failure
/// in the same call must not repeat.
#[derive(Debug, Default)]
struct ResolvedSoFar {
    values: Vec<String>,
}

impl ResolvedSoFar {
    /// Resolves `value`, remembering it, or fails redacted of everything remembered before it.
    async fn read(&mut self, value: &str) -> Result<Secret<String>, CredentialFailure> {
        match read_credential(value).await {
            Ok(resolved) => {
                self.values.push(resolved.clone());
                Ok(Secret::new(resolved))
            }
            Err(error) => Err(self.failed(&error)),
        }
    }

    fn failed(&self, error: &anyhow::Error) -> CredentialFailure {
        CredentialFailure::new(error, &KnownSecrets::new(self.values.clone()))
    }
}

fn validate_required(creds: &toml::CredentialsToml) -> Result<()> {
    if creds.github_token.is_empty() {
        anyhow::bail!("credentials.github_token is required in hub.toml");
    }
    if creds.github_username.is_empty() {
        anyhow::bail!("credentials.github_username is required in hub.toml");
    }
    Ok(())
}

/// `value` itself, or for an `op://` reference, what `op read` returns for it.
async fn read_credential(value: &str) -> Result<String> {
    if !value.starts_with("op://") {
        return Ok(value.to_string());
    }
    let output = tokio::process::Command::new("op")
        .args(["read", value])
        .output()
        .await
        .context("failed to run `op read` — is the 1Password CLI installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to resolve credential from 1Password: {}",
            stderr.trim()
        );
    }
    let resolved = String::from_utf8(output.stdout).context("op read returned non-UTF-8 output")?;
    Ok(resolved.trim().to_string())
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    fn credentials(github_token: &str, github_username: &str) -> toml::CredentialsToml {
        toml::CredentialsToml {
            github_token: github_token.into(),
            github_username: github_username.into(),
            linear_token: None,
            loki_token: None,
            extra: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn plain_credentials_resolve_without_asking_1password() {
        let mut creds = credentials("tok", "user");
        creds.linear_token = Some("lin".into());
        let _ = creds.extra.insert("media".into(), "key".into());
        let file = ConfigFile {
            credentials: creds,
            projects: Vec::new(),
            monitor: None,
            investigation_mcp_servers: domain::InvestigationMcpServers::default(),
        };

        let config = file.resolve().await.unwrap();

        assert_eq!(config.github_token.expose_secret(), "tok");
        assert_eq!(config.github_username, "user");
        assert_eq!(
            config
                .linear_token
                .as_ref()
                .map(ExposeSecret::expose_secret),
            Some(&"lin".to_string())
        );
        assert_eq!(config.extra_credentials["media"].expose_secret(), "key");
    }

    #[tokio::test]
    async fn a_failure_never_repeats_a_credential_resolved_before_it() {
        let mut so_far = ResolvedSoFar::default();
        let _ = so_far.read("tok-4f9a").await.unwrap();

        let failure = so_far.failed(&anyhow::anyhow!("op echoed tok-4f9a back"));

        assert!(
            !failure.reason().as_str().contains("tok-4f9a"),
            "{}",
            failure.reason()
        );
    }

    #[test]
    fn validate_required_passes_with_both_fields_set() {
        assert!(validate_required(&credentials("tok", "user")).is_ok());
    }

    #[test]
    fn validate_required_fails_on_empty_github_token() {
        let err = validate_required(&credentials("", "user")).unwrap_err();
        assert!(err.to_string().contains("github_token"));
    }

    #[test]
    fn validate_required_fails_on_empty_github_username() {
        let err = validate_required(&credentials("tok", "")).unwrap_err();
        assert!(err.to_string().contains("github_username"));
    }

    #[tokio::test]
    async fn a_plain_value_is_its_own_credential() {
        assert_eq!(read_credential("plain-value").await.unwrap(), "plain-value");
    }

    #[tokio::test]
    async fn an_empty_value_is_its_own_credential() {
        assert_eq!(read_credential("").await.unwrap(), "");
    }
}
