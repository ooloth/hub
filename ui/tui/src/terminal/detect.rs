//! Which terminal the TUI is running in.

use anyhow::{bail, Result};

/// The terminal hub opens its windows in.
///
/// Rex is a GUI terminal, so it is always the outermost layer: tmux started in
/// a Rex pane inherits `REX_SESSION`, but a Rex terminal is never inside tmux.
/// That is why `TMUX` decides first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Terminal {
    /// A tmux session, addressed through the `TMUX` socket tmux set.
    Tmux,
    /// Rex, where each window is a tab. `REX_SESSION` only says the TUI runs in
    /// Rex: each tab opens in its repository's session (decision 032).
    Rex,
}

impl Terminal {
    /// Chooses the terminal from the values of `TMUX` and `REX_SESSION`.
    ///
    /// `purpose` names what the caller was trying to do, so the error stays
    /// actionable: "opening lazygit requires a tmux session or a Rex terminal"
    /// tells the user more than "no terminal" alone.
    ///
    /// # Errors
    /// Returns an error naming both terminals when neither is present.
    pub(super) fn detect(
        tmux: Option<&str>,
        rex_session: Option<&str>,
        purpose: &str,
    ) -> Result<Self> {
        if tmux.is_some() {
            return Ok(Self::Tmux);
        }
        match rex_session {
            Some(session) if !session.is_empty() => Ok(Self::Rex),
            _ => {
                bail!("not in tmux or Rex; {purpose} requires a tmux session or a Rex terminal")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Terminal;
    use rstest::rstest;

    const SESSION: &str = "session:01a176a8-c72c-777d-918d-80612ca64cc3";

    /// tmux running in a Rex pane inherits `REX_SESSION`, and the windows the
    /// user can see are tmux's.
    #[rstest]
    #[case(Some("/private/tmp/tmux-501/default,1234,0"), None)]
    #[case(Some("/private/tmp/tmux-501/default,1234,0"), Some(SESSION))]
    fn tmux_is_chosen_whenever_tmux_is_present(
        #[case] tmux: Option<&str>,
        #[case] rex_session: Option<&str>,
    ) {
        assert_eq!(
            Terminal::detect(tmux, rex_session, "investigation").unwrap(),
            Terminal::Tmux
        );
    }

    #[test]
    fn rex_is_chosen_when_tmux_is_absent() {
        assert_eq!(
            Terminal::detect(None, Some(SESSION), "investigation").unwrap(),
            Terminal::Rex
        );
    }

    #[rstest]
    #[case(None)]
    #[case(Some(""))]
    fn without_either_terminal_the_error_names_both_and_the_purpose(
        #[case] rex_session: Option<&str>,
    ) {
        let message = Terminal::detect(None, rex_session, "opening lazygit")
            .unwrap_err()
            .to_string();
        assert!(message.contains("tmux"), "{message}");
        assert!(message.contains("Rex"), "{message}");
        assert!(message.contains("opening lazygit"), "{message}");
    }
}
