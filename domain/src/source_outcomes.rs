//! Which sources a refresh heard from, and which failed.
use crate::source_failure::SourceFailure;

/// Every source a refresh asked, split into those that answered and those that failed.
///
/// How far the refresh reached is derived from the two lists by [`SourceOutcomes::reach`], never
/// stored beside them, so the two cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutcomes {
    answered: Vec<String>,
    failed: Vec<SourceFailure>,
}

/// How far a refresh reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Every source answered.
    EverySource,
    /// Some sources failed and at least one answered.
    SomeSources,
    /// Every source failed.
    NoSource,
}

impl SourceOutcomes {
    /// The sources that `answered` and the ones that `failed`, each in the order they were asked.
    ///
    /// # Panics
    /// If both lists are empty. Every refresh asks the GitHub sources, so only a bug asks none.
    #[must_use]
    pub fn new(answered: Vec<String>, failed: Vec<SourceFailure>) -> Self {
        assert!(
            !answered.is_empty() || !failed.is_empty(),
            "a refresh asks at least one source"
        );
        Self { answered, failed }
    }

    /// The sources that answered, even if with nothing.
    #[must_use]
    pub fn answered(&self) -> &[String] {
        &self.answered
    }

    /// The sources that failed, and why.
    #[must_use]
    pub fn failed(&self) -> &[SourceFailure] {
        &self.failed
    }

    /// How far the refresh reached.
    #[must_use]
    pub const fn reach(&self) -> Reach {
        match (self.answered.is_empty(), self.failed.is_empty()) {
            (_, true) => Reach::EverySource,
            (true, false) => Reach::NoSource,
            (false, false) => Reach::SomeSources,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::known_secrets::KnownSecrets;

    fn failure(source: &str) -> SourceFailure {
        SourceFailure::new(
            source,
            &anyhow::anyhow!("down"),
            &KnownSecrets::new(Vec::new()),
        )
    }

    /// Up to three source names, often none, so the empty lists the outcome turns on come up.
    fn names() -> impl Strategy<Value = Vec<String>> {
        prop_oneof![Just(Vec::new()), prop::collection::vec("[a-z]{1,8}", 1..=3),]
    }

    proptest! {
        #[test]
        fn no_source_was_reached_exactly_when_none_answered(
            answered in names(),
            failed in names(),
        ) {
            prop_assume!(!answered.is_empty() || !failed.is_empty());
            let none_answered = answered.is_empty();
            let outcomes = SourceOutcomes::new(
                answered,
                failed.iter().map(|name| failure(name)).collect(),
            );

            prop_assert_eq!(outcomes.reach() == Reach::NoSource, none_answered);
        }

        #[test]
        fn every_source_was_reached_exactly_when_none_failed(
            answered in names(),
            failed in names(),
        ) {
            prop_assume!(!answered.is_empty() || !failed.is_empty());
            let none_failed = failed.is_empty();
            let outcomes = SourceOutcomes::new(
                answered,
                failed.iter().map(|name| failure(name)).collect(),
            );

            prop_assert_eq!(outcomes.reach() == Reach::EverySource, none_failed);
        }
    }

    #[test]
    #[should_panic(expected = "a refresh asks at least one source")]
    fn a_refresh_that_asked_no_source_is_a_bug() {
        let _ = SourceOutcomes::new(vec![], vec![]);
    }
}
