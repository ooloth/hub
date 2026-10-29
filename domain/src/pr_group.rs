//! The notification group of the card about waiting pull requests, and leftover cards.
//!
//! The group names the profile, so the dev daemon and the installed one never touch each other's
//! card, and neither touches the missing-credentials card. Until #382 each pull request had a card
//! in a group of its own, `hub-<profile>-pr-<owner>/<repo>#<number>`; those are removed at startup.
use crate::profile::Profile;

/// The group `profile`'s card is posted in: `hub-<profile>-turns`.
#[must_use]
pub fn waiting_group(profile: Profile) -> String {
    format!("hub-{profile}-turns")
}

/// The groups of `profile`'s per-pull-request cards that `terminal-notifier -list ALL` reported
/// showing, so they can be removed.
///
/// Measured with terminal-notifier 3.1.0 on 2026-10-17: a header line, then one tab-separated line
/// per card starting with its group, and nothing at all when no card is showing. Lines for other
/// groups, the header among them, are not this profile's leftovers and are skipped.
#[must_use]
pub fn leftover_cards(profile: Profile, list_output: &str) -> Vec<String> {
    let prefix = format!("hub-{profile}-pr-");
    list_output
        .lines()
        .map(|line| line.split('\t').next().unwrap_or_default())
        .filter(|group| group.starts_with(&prefix))
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn each_profile_has_its_own_group() {
        assert_eq!(waiting_group(Profile::Default), "hub-default-turns");
        assert_eq!(waiting_group(Profile::Dev), "hub-dev-turns");
    }

    /// What `terminal-notifier -list ALL` printed on 2026-10-17 with per-pull-request cards
    /// showing, plus a card of each other kind hub posts.
    const LISTED: &str = "GroupID\tTitle\tSubtitle\tMessage\tDelivered At\n\
        hub-default-pr-ooloth/hub#344\tPR review requested\thub #344 · @someone\t[WIP] a title\t2026-10-17 23:27:04 +0000\n\
        hub-dev-pr-ooloth/hub#9\tPR ready to merge\thub #9\tanother\t2026-10-17 23:28:00 +0000\n\
        hub-credentials\thub: signals paused\t\tOpen 1Password.\t2026-10-17 23:29:00 +0000\n\
        hub-default-turns\t2 PRs are waiting for you\t\t2 need your review\t2026-10-28 23:30:00 +0000\n\
        hub-default-pr-not-a-pull-request\tPR review requested\t\tx\t2026-10-17 23:30:00 +0000\n";

    #[test]
    fn leftovers_are_this_profiles_per_pull_request_cards() {
        assert_eq!(
            leftover_cards(Profile::Default, LISTED),
            vec![
                "hub-default-pr-ooloth/hub#344".to_string(),
                "hub-default-pr-not-a-pull-request".to_string(),
            ]
        );
    }

    #[test]
    fn the_other_profiles_leftovers_are_left_to_it() {
        assert_eq!(
            leftover_cards(Profile::Dev, LISTED),
            vec!["hub-dev-pr-ooloth/hub#9".to_string()]
        );
    }

    #[test]
    fn no_cards_showing_leaves_nothing_over() {
        assert!(leftover_cards(Profile::Default, "").is_empty());
    }

    fn profile() -> impl Strategy<Value = Profile> {
        prop_oneof![Just(Profile::Default), Just(Profile::Dev)]
    }

    proptest! {
        #[test]
        fn a_leftover_is_always_this_profiles_per_pull_request_card(
            profile in profile(),
            groups in prop::collection::vec(
                prop_oneof![
                    "hub-(default|dev)-pr-[a-z]{1,8}/[a-z]{1,8}#[0-9]{1,4}",
                    "hub-(default|dev)-turns",
                    Just("hub-credentials".to_string()),
                    Just("GroupID".to_string()),
                    "[a-z-]{0,20}",
                ],
                0..10,
            ),
        ) {
            let listed: String = groups
                .iter()
                .map(|group| format!("{group}\tt\ts\tm\t2026-10-17 23:27:04 +0000\n"))
                .collect();

            let leftovers = leftover_cards(profile, &listed);

            let prefix = format!("hub-{profile}-pr-");
            let expected: Vec<String> = groups.iter().filter(|g| g.starts_with(&prefix)).cloned().collect();
            prop_assert_eq!(&leftovers, &expected);
            prop_assert!(!leftovers.contains(&waiting_group(profile)));
        }
    }
}
