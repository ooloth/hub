//! The notification group a pull request's card is posted in, and reading groups back.
//!
//! Each pull request gets its own group, so its card can be replaced or removed without touching
//! the others. The group names the profile, so the dev daemon and the installed one never touch
//! each other's cards, and neither touches the missing-credentials card.
use crate::pr::RepoSlug;
use crate::pr_id::PrId;
use crate::profile::Profile;

/// The group `pr`'s card is posted in, for `profile`: `hub-<profile>-pr-<owner>/<repo>#<number>`.
#[must_use]
pub fn group_for(profile: Profile, pr: &PrId) -> String {
    format!("{}{pr}", prefix(profile))
}

fn prefix(profile: Profile) -> String {
    format!("hub-{profile}-pr-")
}

/// The cards for `profile`'s pull requests that `terminal-notifier -list ALL` reported showing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShownCards {
    /// The pull requests with a card showing.
    pub prs: Vec<PrId>,
    /// Groups in this profile's form that did not name a pull request.
    pub unreadable: Vec<String>,
}

/// Reads the output of `terminal-notifier -list ALL`.
///
/// Measured with terminal-notifier 3.1.0 on 2026-10-17: a header line, then one tab-separated line
/// per card starting with its group, and nothing at all when no card is showing. Lines for other
/// groups, the header among them, are not this profile's and are skipped.
#[must_use]
pub fn shown_cards(profile: Profile, list_output: &str) -> ShownCards {
    let prefix = prefix(profile);
    let mut shown = ShownCards::default();
    for line in list_output.lines() {
        let group = line.split('\t').next().unwrap_or_default();
        let Some(reference) = group.strip_prefix(&prefix) else {
            continue;
        };
        match pr_in(reference) {
            Some(pr) => shown.prs.push(pr),
            None => shown.unreadable.push(group.to_string()),
        }
    }
    shown
}

/// The pull request `owner/repo#number` names.
fn pr_in(reference: &str) -> Option<PrId> {
    let (repo, number) = reference.rsplit_once('#')?;
    Some(PrId {
        repo: repo.parse::<RepoSlug>().ok()?,
        number: number.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn id(owner: &str, repo: &str, number: u64) -> PrId {
        PrId {
            repo: RepoSlug::new(owner, repo),
            number,
        }
    }

    #[test]
    fn a_group_names_the_profile_and_the_pull_request() {
        assert_eq!(
            group_for(Profile::Dev, &id("ooloth", "hub", 344)),
            "hub-dev-pr-ooloth/hub#344"
        );
    }

    /// What `terminal-notifier -list ALL` printed on 2026-10-17 with one card showing, plus a card
    /// of each other kind hub posts.
    const LISTED: &str = "GroupID\tTitle\tSubtitle\tMessage\tDelivered At\n\
        hub-default-pr-ooloth/hub#344\tPR review requested\thub #344 · @someone\t[WIP] a title\t2026-10-17 23:27:04 +0000\n\
        hub-dev-pr-ooloth/hub#9\tPR ready to merge\thub #9\tanother\t2026-10-17 23:28:00 +0000\n\
        hub-credentials\thub: signals paused\t\tOpen 1Password.\t2026-10-17 23:29:00 +0000\n\
        hub-default-pr-not-a-pull-request\tPR review requested\t\tx\t2026-10-17 23:30:00 +0000\n";

    #[test]
    fn listed_cards_are_read_back_as_this_profiles_pull_requests() {
        let shown = shown_cards(Profile::Default, LISTED);

        assert_eq!(shown.prs, vec![id("ooloth", "hub", 344)]);
        assert_eq!(
            shown.unreadable,
            vec!["hub-default-pr-not-a-pull-request".to_string()]
        );
    }

    #[test]
    fn the_other_profiles_cards_are_left_to_it() {
        let shown = shown_cards(Profile::Dev, LISTED);

        assert_eq!(shown.prs, vec![id("ooloth", "hub", 9)]);
        assert!(shown.unreadable.is_empty(), "{shown:?}");
    }

    #[test]
    fn no_cards_showing_lists_nothing() {
        assert_eq!(shown_cards(Profile::Default, ""), ShownCards::default());
    }

    fn profile() -> impl Strategy<Value = Profile> {
        prop_oneof![Just(Profile::Default), Just(Profile::Dev)]
    }

    proptest! {
        #[test]
        fn a_group_reads_back_as_the_pull_request_it_was_made_for(
            profile in profile(),
            owner in "[A-Za-z0-9][A-Za-z0-9-]{0,20}",
            repo in "[A-Za-z0-9._-]{1,30}",
            number in 1..1_000_000_u64,
        ) {
            // GitHub refuses these names, and so does `RepoSlug`.
            prop_assume!(repo != "." && repo != "..");
            let pr = id(&owner, &repo, number);
            let line = format!("{}\tPR review requested\tx\ty\t2026-10-17 23:27:04 +0000", group_for(profile, &pr));

            prop_assert_eq!(shown_cards(profile, &line).prs, vec![pr.clone()]);
            let other = match profile { Profile::Default => Profile::Dev, Profile::Dev => Profile::Default };
            prop_assert_eq!(shown_cards(other, &line), ShownCards::default());
        }
    }
}
