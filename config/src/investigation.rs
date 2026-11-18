use anyhow::{anyhow, Context, Result};
use domain::{InvestigationLauncher, InvestigationMcpServers, McpServer, McpServerName};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The `[investigation]` section of `hub.toml`: what every investigation
/// launched on this device receives.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InvestigationToml {
    /// `[investigation.mcp_servers.<name>]` tables, keyed by the name Claude
    /// Code lists the server under. Absent means no MCP servers.
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerToml>,
    /// The absolute path of an executable every investigation runs in place
    /// of `claude`. Absent means `claude`.
    #[serde(default)]
    pub launcher: Option<String>,
}

/// One `[investigation.mcp_servers.<name>]` table.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum McpServerToml {
    /// `type = "http"`: a server reached over HTTP.
    Http {
        /// The server's `http` or `https` URL.
        url: String,
    },
}

impl InvestigationToml {
    /// # Errors
    /// Returns an error naming the server if a name or URL is invalid.
    pub fn mcp_servers(&self) -> Result<InvestigationMcpServers> {
        let mut servers = BTreeMap::new();
        for (name, server) in &self.mcp_servers {
            let context = || format!("invalid [investigation.mcp_servers.{name}] in hub.toml");
            let parsed_name: McpServerName = name
                .parse()
                .map_err(|e| anyhow!("{e}"))
                .with_context(context)?;
            let parsed_server = match server {
                McpServerToml::Http { url } => McpServer::http(url),
            }
            .map_err(|e| anyhow!("{e}"))
            .with_context(context)?;
            let _ = servers.insert(parsed_name, parsed_server);
        }
        Ok(InvestigationMcpServers::new(servers))
    }

    /// # Errors
    /// Returns an error naming the setting if `launcher` is not an absolute path.
    pub fn launcher(&self) -> Result<InvestigationLauncher> {
        match &self.launcher {
            None => Ok(InvestigationLauncher::Claude),
            Some(path) => InvestigationLauncher::executable(path)
                .map_err(|e| anyhow!("{e}"))
                .context("invalid [investigation] launcher in hub.toml"),
        }
    }
}

/// Checks the launcher can run, so hub refuses to start rather than open
/// windows that fail. `claude` is looked up on each window's `PATH` when it
/// opens, so it has nothing to check here.
///
/// # Errors
/// Returns an error naming the setting and the path if the path is missing,
/// is not a file, or has no execute bit.
pub fn ensure_launcher_runs(launcher: &InvestigationLauncher) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let Some(path) = launcher.path() else {
        return Ok(());
    };
    let context = || {
        format!(
            "[investigation] launcher \"{}\" in hub.toml can't run",
            path.display()
        )
    };
    let metadata = std::fs::metadata(path).with_context(context)?;
    if !metadata.is_file() {
        return Err(anyhow!("it is not a file")).with_context(context);
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(anyhow!("it has no execute bit")).with_context(context);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn parse(toml: &str) -> Result<InvestigationToml> {
        Ok(::toml::from_str(toml)?)
    }

    #[test]
    fn an_http_server_parses() {
        let servers = parse(
            r#"
            [mcp_servers.atlassian-rovo]
            type = "http"
            url = "https://mcp.atlassian.com/v1/mcp"
            "#,
        )
        .unwrap()
        .mcp_servers()
        .unwrap();
        assert_eq!(
            servers,
            InvestigationMcpServers::new(BTreeMap::from([(
                "atlassian-rovo".parse().unwrap(),
                McpServer::http("https://mcp.atlassian.com/v1/mcp").unwrap(),
            )]))
        );
    }

    #[test]
    fn no_mcp_servers_table_means_no_servers() {
        assert_eq!(
            parse("").unwrap().mcp_servers().unwrap(),
            InvestigationMcpServers::default()
        );
    }

    #[rstest]
    #[case::a_server_without_a_url("[mcp_servers.x]\ntype = \"http\"")]
    #[case::an_unknown_server_type("[mcp_servers.x]\ntype = \"stdio\"\ncommand = \"npx\"")]
    #[case::an_unknown_server_key(
        "[mcp_servers.x]\ntype = \"http\"\nurl = \"https://e.com\"\nheaders = 1"
    )]
    #[case::an_unknown_top_level_key("unknown = 1")]
    fn a_malformed_table_is_rejected(#[case] toml: &str) {
        assert!(parse(toml).is_err());
    }

    #[test]
    fn an_invalid_url_is_rejected_naming_the_server() {
        let err = parse("[mcp_servers.jira]\ntype = \"http\"\nurl = \"not a url\"")
            .unwrap()
            .mcp_servers()
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains("[investigation.mcp_servers.jira]"),
            "{message}"
        );
        assert!(message.contains("\"not a url\""), "{message}");
    }

    #[test]
    fn no_launcher_means_claude() {
        assert_eq!(
            parse("").unwrap().launcher().unwrap(),
            InvestigationLauncher::Claude
        );
    }

    #[test]
    fn an_absolute_launcher_path_is_the_launcher() {
        let launcher = parse("launcher = \"/opt/tools/cc.bash\"")
            .unwrap()
            .launcher()
            .unwrap();
        assert_eq!(launcher.program(), "/opt/tools/cc.bash");
    }

    #[rstest]
    #[case::empty("")]
    #[case::relative("tools/cc.bash")]
    fn a_launcher_that_is_not_absolute_is_rejected_naming_the_setting(#[case] value: &str) {
        let err = parse(&format!("launcher = {value:?}"))
            .unwrap()
            .launcher()
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("[investigation] launcher"), "{message}");
        assert!(message.contains(&format!("{value:?}")), "{message}");
    }

    /// A folder of its own under the system's temporary directory, removed
    /// when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("hub-launcher-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str, mode: u32) -> std::path::PathBuf {
            use std::os::unix::fs::PermissionsExt as _;

            let path = self.0.join(name);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn launcher_at(path: &std::path::Path) -> InvestigationLauncher {
        InvestigationLauncher::executable(path.to_str().unwrap()).unwrap()
    }

    #[test]
    fn claude_and_an_executable_file_can_run() {
        let scratch = Scratch::new("runs");
        assert!(ensure_launcher_runs(&InvestigationLauncher::Claude).is_ok());
        assert!(ensure_launcher_runs(&launcher_at(&scratch.file("cc.bash", 0o755))).is_ok());
    }

    #[rstest]
    #[case::missing("missing")]
    #[case::a_folder("folder")]
    #[case::not_executable("not-executable")]
    fn a_launcher_that_cant_run_is_refused_naming_the_setting_and_path(#[case] kind: &str) {
        let scratch = Scratch::new(kind);
        let path = match kind {
            "missing" => scratch.0.join("cc.bash"),
            "folder" => scratch.0.clone(),
            _ => scratch.file("cc.bash", 0o644),
        };

        let err = ensure_launcher_runs(&launcher_at(&path)).unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("[investigation] launcher"), "{message}");
        assert!(
            message.contains(&format!("\"{}\"", path.display())),
            "{message}"
        );
    }

    #[test]
    fn an_invalid_name_is_rejected_naming_the_server() {
        let err = parse("[mcp_servers.\"has space\"]\ntype = \"http\"\nurl = \"https://e.com\"")
            .unwrap()
            .mcp_servers()
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains("[investigation.mcp_servers.has space]"),
            "{message}"
        );
    }
}
