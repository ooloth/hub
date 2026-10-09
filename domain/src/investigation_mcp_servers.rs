//! The MCP servers an investigation may use, chosen per device in `hub.toml`.
//!
//! Investigations ignore every other MCP configuration, including the
//! `.mcp.json` of the repository under investigation, whose author may be a
//! stranger. These servers are the only ones a session gets.

use std::collections::BTreeMap;
use std::str::FromStr;
use url::Url;

/// The name Claude Code lists an MCP server under. Letters, digits, `-` and
/// `_` only, so it is the same name `/mcp` shows and Claude Code's stored
/// login for that server is found under.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct McpServerName(String);

impl FromStr for McpServerName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if valid {
            Ok(Self(s.to_string()))
        } else {
            Err(format!(
                "invalid MCP server name {s:?}: expected letters, digits, '-' and '_' only"
            ))
        }
    }
}

impl std::fmt::Display for McpServerName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How an investigation reaches one MCP server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum McpServer {
    /// A server Claude Code reaches over HTTP at this URL.
    Http {
        /// An `http` or `https` URL.
        url: Url,
    },
}

impl McpServer {
    /// # Errors
    /// Returns an error if `url` is not a URL or its scheme is not `http` or
    /// `https`.
    pub fn http(url: &str) -> Result<Self, String> {
        let parsed = Url::parse(url).map_err(|e| format!("invalid MCP server url {url:?}: {e}"))?;
        match parsed.scheme() {
            "http" | "https" => Ok(Self::Http { url: parsed }),
            other => Err(format!(
                "invalid MCP server url {url:?}: scheme {other:?} is not http or https"
            )),
        }
    }
}

/// Every MCP server an investigation on this device may use. Empty means none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InvestigationMcpServers(BTreeMap<McpServerName, McpServer>);

impl InvestigationMcpServers {
    /// Wraps servers that have each already been validated.
    #[must_use]
    pub const fn new(servers: BTreeMap<McpServerName, McpServer>) -> Self {
        Self(servers)
    }

    /// Whether this device gives investigations no MCP servers at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The servers in the JSON shape Claude Code's `--mcp-config` reads.
    #[must_use]
    pub fn claude_mcp_config(&self) -> String {
        let servers: serde_json::Map<String, serde_json::Value> = self
            .0
            .iter()
            .map(|(name, server)| {
                let entry = match server {
                    McpServer::Http { url } => {
                        serde_json::json!({ "type": "http", "url": url.as_str() })
                    }
                };
                (name.to_string(), entry)
            })
            .collect();
        serde_json::json!({ "mcpServers": servers }).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn atlassian() -> InvestigationMcpServers {
        InvestigationMcpServers::new(BTreeMap::from([(
            "atlassian-rovo".parse().unwrap(),
            McpServer::http("https://mcp.atlassian.com/v1/mcp").unwrap(),
        )]))
    }

    #[test]
    fn an_http_server_becomes_claude_code_mcp_config() {
        let json: serde_json::Value =
            serde_json::from_str(&atlassian().claude_mcp_config()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "mcpServers": {
                    "atlassian-rovo": {
                        "type": "http",
                        "url": "https://mcp.atlassian.com/v1/mcp"
                    }
                }
            })
        );
    }

    #[test]
    fn no_servers_is_empty() {
        assert!(InvestigationMcpServers::default().is_empty());
        assert!(!atlassian().is_empty());
    }

    #[rstest]
    #[case("atlassian-rovo")]
    #[case("github_2")]
    fn a_name_of_letters_digits_dashes_and_underscores_is_valid(#[case] name: &str) {
        assert_eq!(name.parse::<McpServerName>().unwrap().to_string(), name);
    }

    #[rstest]
    #[case("")]
    #[case("has space")]
    #[case("dotted.name")]
    #[case("quote\"d")]
    fn any_other_name_is_rejected(#[case] name: &str) {
        let err = name.parse::<McpServerName>().unwrap_err();
        assert!(err.contains(&format!("{name:?}")), "{err}");
    }

    #[rstest]
    #[case("not a url")]
    #[case("ftp://example.com/mcp")]
    #[case("file:///etc/passwd")]
    fn a_url_that_is_not_http_or_https_is_rejected(#[case] url: &str) {
        let err = McpServer::http(url).unwrap_err();
        assert!(err.contains(&format!("{url:?}")), "{err}");
    }

    #[rstest]
    #[case("https://mcp.atlassian.com/v1/mcp")]
    #[case("http://localhost:8080/mcp")]
    fn an_http_or_https_url_is_accepted(#[case] url: &str) {
        assert!(McpServer::http(url).is_ok());
    }
}
