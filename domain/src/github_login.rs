//! The GitHub login of whoever wrote a pull request or issue, as a tab label shows it.

/// A GitHub login: letters, digits, `-` and `_`, at most [`Self::MAX_LEN`] characters, never
/// starting with `-`.
///
/// GitHub's own logins are letters, digits and single hyphens, and an enterprise-managed account
/// adds `_` and a short code. Anything else cannot be a login, so it cannot reach a Rex label,
/// where a parenthesis, colon or line break would change what the label says (decision 033).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubLogin(String);

impl GitHubLogin {
    /// The longest login GitHub allows.
    pub const MAX_LEN: usize = 39;

    /// The login in `raw`, or why it is not one.
    ///
    /// # Errors
    /// Returns why `raw` is not a GitHub login.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let well_formed = !raw.is_empty()
            && raw.len() <= Self::MAX_LEN
            && !raw.starts_with('-')
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if well_formed {
            Ok(Self(raw.to_string()))
        } else {
            Err(format!(
                "{raw:?} is not a GitHub login: letters, digits, - and _, at most {} characters, not starting with -",
                Self::MAX_LEN
            ))
        }
    }

    /// The login as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GitHubLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::GitHubLogin;
    use rstest::rstest;

    #[rstest]
    #[case("alice")]
    #[case("ghost")]
    #[case("dependabot")]
    #[case("copilot-pull-request-reviewer")]
    #[case("a_b")]
    #[case("a")]
    #[case("abcdefghijklmnopqrstuvwxyz-0123456789ab")]
    fn a_login_github_could_send_is_accepted(#[case] raw: &str) {
        assert_eq!(GitHubLogin::parse(raw).unwrap().as_str(), raw);
    }

    /// Each of these would change what a label says, or start it like a flag.
    #[rstest]
    #[case::empty("")]
    #[case::space("al ice")]
    #[case::open_paren("al(ice")]
    #[case::close_paren("alice)")]
    #[case::colon("alice:")]
    #[case::newline("alice\n")]
    #[case::leading_dash("-alice")]
    #[case::brackets("dependabot[bot]")]
    #[case::too_long("abcdefghijklmnopqrstuvwxyz-0123456789abc")]
    fn anything_else_is_refused(#[case] raw: &str) {
        let reason = GitHubLogin::parse(raw).unwrap_err();
        assert!(reason.contains("is not a GitHub login"), "{reason}");
    }
}
