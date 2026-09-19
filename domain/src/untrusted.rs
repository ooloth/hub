//! Text hub did not write.

use serde::{Deserialize, Serialize};

/// Text that came from outside hub: a log line, an alert message, a title
/// somebody else typed.
///
/// Modelled on `secrecy::Secret`, for the same reason. There is no `Display`,
/// no `Deref` and no `AsRef<str>`, so one cannot be interpolated into a string
/// by accident, and every read goes through [`UntrustedText::expose`], which
/// makes the use sites greppable.
///
/// `InvestigationPrompt` is the only thing that should put one of these into a
/// prompt. It fences the value on the way in, so an agent can tell hub's
/// instructions from a stranger's words.
///
/// Serialised transparently, so wrapping a field that was already a `String`
/// leaves the cached JSON unchanged and needs no schema bump.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UntrustedText(String);

impl UntrustedText {
    /// Marks text as externally authored.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text itself.
    ///
    /// Deliberately conspicuous at the call site, the same reason `secrecy`
    /// calls its accessor `expose_secret`. Reaching for this to build a prompt
    /// defeats the point of the type; `InvestigationPrompt::untrusted` is the
    /// way in.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::UntrustedText;

    #[test]
    fn text_survives_being_marked_untrusted() {
        assert_eq!(UntrustedText::new("OOM killed").expose(), "OOM killed");
    }

    /// The cache holds these inside `LokiEntry` and `GcpEntry`, which were
    /// serialised with plain `String` fields before this type existed. A
    /// wrapper object here would strand every cached row.
    #[test]
    fn serialisation_is_indistinguishable_from_a_plain_string() {
        let value = UntrustedText::new("OOM killed");
        let json = serde_json::to_string(&value).expect("serialise");
        assert_eq!(json, r#""OOM killed""#);
        let back: UntrustedText = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, value);
    }
}
