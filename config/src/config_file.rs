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
    investigation_launcher: domain::InvestigationLauncher,
}

impl ConfigFile {
    /// Reads `hub.toml` from the working directory.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, lacks a required credential, names
    /// an investigation launcher that is not an absolute path, or gives a project a label hub
    /// refuses.
    pub fn read() -> Result<Self> {
        Self::read_from("hub.toml")
    }

    /// Reads the `hub.toml` at `path`, as [`ConfigFile::read`] does.
    pub(crate) fn read_from(path: &str) -> Result<Self> {
        let hub_toml = toml::parse_file(path)?;
        validate_required(&hub_toml.credentials)?;
        let investigation_mcp_servers = hub_toml.investigation.mcp_servers()?;
        let investigation_launcher = hub_toml.investigation.launcher()?;
        let projects = crate::Projects::resolve(hub_toml.project)
            .context("hub.toml has a [[project]] hub cannot use")?;
        Ok(Self {
            credentials: hub_toml.credentials,
            projects,
            monitor: hub_toml.monitor,
            investigation_mcp_servers,
            investigation_launcher,
        })
    }

    /// What investigations run in place of `claude`, from `[investigation] launcher`.
    #[must_use]
    pub(crate) const fn investigation_launcher(&self) -> &domain::InvestigationLauncher {
        &self.investigation_launcher
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
            investigation_launcher: self.investigation_launcher.clone(),
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
pub(crate) mod tests {
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
            investigation_launcher: domain::InvestigationLauncher::Claude,
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

    /// A `hub.toml` holding `body` under plain credentials, in a folder of its own under the
    /// system's temporary directory. Returns the folder, for the caller to remove, and the file.
    pub(crate) fn hub_toml_with(name: &str, body: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("hub-config-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.toml");
        std::fs::write(
            &path,
            format!("[credentials]\ngithub_token = \"tok\"\ngithub_username = \"user\"\n\n{body}"),
        )
        .unwrap();
        (dir, path.to_str().unwrap().to_string())
    }

    #[test]
    fn reading_refuses_a_launcher_that_is_not_absolute_naming_the_setting() {
        let (dir, path) = hub_toml_with(
            "relative-launcher",
            "[investigation]\nlauncher = \"tools/cc.bash\"\n",
        );

        let result = ConfigFile::read_from(&path);

        std::fs::remove_dir_all(&dir).unwrap();
        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("[investigation] launcher"), "{message}");
        assert!(message.contains("\"tools/cc.bash\""), "{message}");
    }

    /// The daemon reads `hub.toml` this way and launches no investigation, so a launcher that
    /// is absolute but missing must not stop it.
    #[test]
    fn reading_accepts_an_absolute_launcher_that_does_not_exist() {
        let (dir, path) = hub_toml_with(
            "missing-launcher",
            "[investigation]\nlauncher = \"/nonexistent/cc.bash\"\n",
        );

        let result = ConfigFile::read_from(&path);

        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            result.unwrap().investigation_launcher().program(),
            "/nonexistent/cc.bash"
        );
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
