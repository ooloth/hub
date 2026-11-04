use std::collections::HashMap;

use anyhow::{Context, Result};
use domain::credential_failure::CredentialFailure;

use crate::op_read::ResolvedSoFar;
use crate::op_responsibility::OpResponsibility;
use crate::resolved::{Config, Resolved};
use crate::toml;

/// `hub.toml`, read and checked, with its credentials still as written: plain values or `op://`
/// references nobody has asked 1Password about yet.
///
/// Reading it can fail only on the file itself. Resolving it can fail only on a credential, so a
/// caller can try [`ConfigFile::resolve`] again without ever retrying a broken file.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    credentials: toml::CredentialsToml,
    projects: crate::Projects,
    monitor: Option<toml::Monitor>,
    investigation_mcp_servers: domain::InvestigationMcpServers,
}

impl ConfigFile {
    /// Reads `hub.toml` from the working directory.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, lacks a required credential, or
    /// gives a project a label hub refuses.
    pub fn read() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        validate_required(&hub_toml.credentials)?;
        let investigation_mcp_servers = hub_toml.investigation.mcp_servers()?;
        let projects = crate::Projects::resolve(hub_toml.project)
            .context("hub.toml has a [[project]] hub cannot use")?;
        Ok(Self {
            credentials: hub_toml.credentials,
            projects,
            monitor: hub_toml.monitor,
            investigation_mcp_servers,
        })
    }

    /// Resolves every credential, asking 1Password for each `op://` reference through an `op`
    /// started as `responsibility` allows, and says which ways of starting `op` answered.
    ///
    /// # Errors
    /// Returns why the first credential that could not be resolved failed, redacted of every
    /// credential resolved before it.
    pub async fn resolve(
        &self,
        responsibility: OpResponsibility,
    ) -> Result<Resolved, CredentialFailure> {
        let creds = &self.credentials;
        let mut so_far = ResolvedSoFar::new(responsibility);
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
        let config = Config {
            github_token,
            github_username: creds.github_username.clone(),
            linear_token,
            loki_token,
            extra_credentials,
            projects: self.projects.clone(),
            monitor: self.monitor.clone(),
            investigation_mcp_servers: self.investigation_mcp_servers.clone(),
        };
        Ok(Resolved {
            config,
            spawns: so_far.spawns,
        })
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
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
        let file = ConfigFile {
            credentials: toml::CredentialsToml {
                linear_token: Some("lin".into()),
                extra: HashMap::from([("media".into(), "key".into())]),
                ..credentials("tok", "user")
            },
            projects: crate::Projects::default(),
            monitor: None,
            investigation_mcp_servers: domain::InvestigationMcpServers::default(),
        };

        // A plain value never reaches a spawn, so either responsibility will do.
        let resolved = file.resolve(OpResponsibility::Inherited).await.unwrap();
        let config = resolved.config;

        assert_eq!(resolved.spawns.to_string(), "none");
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

    #[test]
    fn validate_required_passes_with_both_fields_set() {
        assert!(validate_required(&credentials("tok", "user")).is_ok());
    }

    #[rstest]
    #[case::empty_github_token("", "user", "github_token")]
    #[case::empty_github_username("tok", "", "github_username")]
    fn validate_required_fails_naming_the_empty_field(
        #[case] github_token: &str,
        #[case] github_username: &str,
        #[case] field: &str,
    ) {
        let err = validate_required(&credentials(github_token, github_username)).unwrap_err();
        assert!(err.to_string().contains(field), "{err}");
    }
}
