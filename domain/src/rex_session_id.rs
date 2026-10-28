//! The Rex session a terminal belongs to.

/// The Rex session a terminal belongs to, as Rex reports it in `REX_SESSION`.
///
/// Rex sets the variable in every terminal it opens, so the TUI's own value
/// names the session its windows are listed in and opened into. Holding one is
/// proof the value was present and non-empty; whether the session still exists
/// is the Rex server's to say, and it says so when asked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RexSessionId(String);

impl RexSessionId {
    /// Reads the value of `REX_SESSION`, or `None` when it is empty.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        (!raw.is_empty()).then(|| Self(raw.to_string()))
    }

    /// The id as Rex prints and accepts it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RexSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::RexSessionId;

    #[test]
    fn a_session_id_is_kept_as_rex_wrote_it() {
        let id = RexSessionId::parse("session:01a176a8-c72c-777d-918d-80612ca64cc3").unwrap();
        assert_eq!(id.as_str(), "session:01a176a8-c72c-777d-918d-80612ca64cc3");
    }

    #[test]
    fn an_empty_value_is_no_session() {
        assert_eq!(RexSessionId::parse(""), None);
    }
}
