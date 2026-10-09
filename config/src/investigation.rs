use anyhow::{anyhow, Context, Result};
use domain::{InvestigationMcpServers, McpServer, McpServerName};
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(parse("").unwrap().mcp_servers().unwrap().is_empty());
    }

    #[test]
    fn a_server_without_a_url_is_rejected() {
        assert!(parse("[mcp_servers.x]\ntype = \"http\"").is_err());
    }

    #[test]
    fn an_unknown_server_type_is_rejected() {
        assert!(parse("[mcp_servers.x]\ntype = \"stdio\"\ncommand = \"npx\"").is_err());
    }

    #[test]
    fn an_unknown_key_is_rejected() {
        assert!(
            parse("[mcp_servers.x]\ntype = \"http\"\nurl = \"https://e.com\"\nheaders = 1")
                .is_err()
        );
        assert!(parse("unknown = 1").is_err());
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
