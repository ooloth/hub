//! Who is asked to review a pull request, who spoke last in its open threads, and what each
//! reviewer concluded, read from one search response into [`PrActivity`].
use chrono::{DateTime, Utc};
use domain::pr_activity::{
    LatestVerdict, OpenThread, Opinion, Participant, PrActivity, Requested, ReviewBody,
};
use serde::Deserialize;

/// The fields every search asks for so that whose move it is can be read.
///
/// `comments(last: 10)` on each thread keeps the query's node count near today's: GitHub prices a
/// query by its `first` and `last` arguments before it runs, and each extra comment per thread
/// adds 5,000 nodes against a limit of 500,000 (measured 2026-10-26). Reading each thread's first
/// comment as well, to see who opened it, nearly doubled the query's cost, so it is not read.
pub(super) const ACTIVITY_FIELDS: &str = "
            latestOpinionatedReviews(first: 100) { nodes { state author { __typename login } submittedAt } }
            reviewRequests(first: 20) { nodes { requestedReviewer { __typename
                ... on User { login } ... on Bot { login } ... on Mannequin { login } } } }
            reviewThreads(first: 50) { nodes { isResolved path line diffSide
                comments(last: 10) { nodes { author { __typename login } createdAt body } }
            } }
            recentReviews: reviews(last: 100) { nodes { state author { __typename login } submittedAt body } }
            recentComments: comments(last: 100) { nodes { author { login } createdAt } }";

/// The activity fields of one pull request in a search response. Every search asks for all of
/// them, so a response without one fails to parse rather than reading as no activity.
#[derive(Deserialize)]
pub(super) struct ActivityNodes {
    #[serde(rename = "latestOpinionatedReviews")]
    latest_opinionated_reviews: Connection<VerdictNode>,
    #[serde(rename = "reviewRequests")]
    review_requests: Connection<RequestNode>,
    #[serde(rename = "reviewThreads")]
    pub(super) review_threads: Connection<ThreadNode>,
    #[serde(rename = "recentReviews")]
    recent_reviews: Connection<ReviewNode>,
    #[serde(rename = "recentComments")]
    recent_comments: Connection<TopLevelCommentNode>,
}

#[derive(Deserialize)]
pub(super) struct Connection<T> {
    pub(super) nodes: Vec<T>,
}

impl<T> Default for Connection<T> {
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

/// Who did something. Where GitHub returns `null` instead, for a deleted account, the field is an
/// `Option<Actor>`.
#[derive(Deserialize)]
pub(super) struct Actor {
    #[serde(rename = "__typename")]
    typename: String,
    pub(super) login: String,
}

#[derive(Deserialize)]
struct VerdictNode {
    state: String,
    author: Option<Actor>,
    #[serde(rename = "submittedAt")]
    submitted_at: Option<String>,
}

#[derive(Deserialize)]
struct RequestNode {
    /// `None` when the requested reviewer cannot be seen with this token.
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<RequestedReviewer>,
}

/// A team is read by its kind alone: its name needs the `read:org` scope, and hub's token has only
/// `repo`, so asking for it makes GitHub refuse the whole query (seen 2026-10-28).
#[derive(Deserialize)]
struct RequestedReviewer {
    #[serde(rename = "__typename")]
    typename: String,
    login: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ThreadNode {
    #[serde(rename = "isResolved")]
    pub(super) is_resolved: bool,
    pub(super) path: String,
    pub(super) line: Option<u32>,
    #[serde(rename = "diffSide")]
    pub(super) diff_side: String,
    pub(super) comments: Connection<ThreadCommentNode>,
}

#[derive(Deserialize)]
pub(super) struct ThreadCommentNode {
    pub(super) author: Option<Actor>,
    #[serde(rename = "createdAt")]
    pub(super) created_at: String,
    pub(super) body: String,
}

#[derive(Deserialize)]
struct ReviewNode {
    state: String,
    author: Option<Actor>,
    /// `None` for a `PENDING` review, which has not been submitted.
    #[serde(rename = "submittedAt")]
    submitted_at: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct TopLevelCommentNode {
    author: Option<CommentAuthor>,
    #[serde(rename = "createdAt")]
    created_at: String,
}

#[derive(Deserialize)]
struct CommentAuthor {
    login: String,
}

/// What `nodes` say about whose move it is on a pull request, as `me` sees it.
///
/// Anything GitHub sends that names nobody, such as a comment from a deleted account, a review
/// that was never submitted or a time that does not parse, says nothing about whose move it is,
/// so it is left out.
pub(super) fn activity(nodes: &ActivityNodes, me: &str) -> PrActivity {
    let my_reviews = nodes
        .recent_reviews
        .nodes
        .iter()
        .filter(|review| review.author.as_ref().is_some_and(|a| a.login == me))
        .filter_map(|review| time(review.submitted_at.as_deref()?));
    let my_comments = nodes
        .recent_comments
        .nodes
        .iter()
        .filter(|comment| comment.author.as_ref().is_some_and(|a| a.login == me))
        .filter_map(|comment| time(&comment.created_at));
    PrActivity {
        requests: nodes
            .review_requests
            .nodes
            .iter()
            .map(|request| requested(request.requested_reviewer.as_ref()))
            .collect(),
        open_threads: nodes
            .review_threads
            .nodes
            .iter()
            .filter(|thread| !thread.is_resolved)
            .filter_map(|thread| open_thread(thread, me))
            .collect(),
        verdicts: nodes
            .latest_opinionated_reviews
            .nodes
            .iter()
            .filter_map(|verdict| latest_verdict(verdict, me))
            .collect(),
        bodies: nodes
            .recent_reviews
            .nodes
            .iter()
            .filter_map(|review| review_body(review, me))
            .collect(),
        my_latest: my_reviews.chain(my_comments).max(),
    }
}

fn participant(actor: &Actor, me: &str) -> Participant {
    if actor.login == me {
        Participant::Me
    } else if actor.typename == "Bot" {
        Participant::Bot(actor.login.clone())
    } else {
        Participant::Person(actor.login.clone())
    }
}

/// Who a review is requested from. A reviewer this token cannot see, which GitHub returns as
/// `null` for a team on a public repository, is still someone else being asked, so it counts.
fn requested(reviewer: Option<&RequestedReviewer>) -> Requested {
    let Some(reviewer) = reviewer else {
        return Requested::Team("an unseen reviewer".to_string());
    };
    match (reviewer.typename.as_str(), &reviewer.login) {
        ("User" | "Mannequin", Some(login)) => Requested::Person(login.clone()),
        ("Bot", Some(login)) => Requested::Bot(login.clone()),
        (typename, _) => Requested::Team(typename.to_string()),
    }
}

fn open_thread(thread: &ThreadNode, me: &str) -> Option<OpenThread> {
    let comments: Vec<(Participant, DateTime<Utc>)> = thread
        .comments
        .nodes
        .iter()
        .filter_map(|comment| {
            Some((
                participant(comment.author.as_ref()?, me),
                time(&comment.created_at)?,
            ))
        })
        .collect();
    OpenThread::from_comments(&comments)
}

fn latest_verdict(node: &VerdictNode, me: &str) -> Option<LatestVerdict> {
    let opinion = match node.state.as_str() {
        "APPROVED" => Opinion::Approved,
        "CHANGES_REQUESTED" => Opinion::ChangesRequested,
        _ => return None,
    };
    Some(LatestVerdict {
        by: participant(node.author.as_ref()?, me),
        opinion,
        at: time(node.submitted_at.as_deref()?)?,
    })
}

/// A person's submitted review with something in its body, whatever its verdict. A bot's body is
/// left out: a reviewer bot writes an overview on every review, and its points arrive as threads.
fn review_body(node: &ReviewNode, me: &str) -> Option<ReviewBody> {
    let author = node.author.as_ref()?;
    let counts = matches!(
        node.state.as_str(),
        "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED"
    ) && !node.body.trim().is_empty();
    match participant(author, me) {
        Participant::Person(login) if counts => Some(ReviewBody {
            by: login,
            at: time(node.submitted_at.as_deref()?)?,
        }),
        Participant::Person(_) | Participant::Me | Participant::Bot(_) => None,
    }
}

fn time(rfc3339: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|t| t.to_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    /// `json`, with every activity field it leaves out read as empty.
    fn nodes(json: serde_json::Value) -> ActivityNodes {
        let mut full = serde_json::json!({
            "latestOpinionatedReviews": { "nodes": [] },
            "reviewRequests": { "nodes": [] },
            "reviewThreads": { "nodes": [] },
            "recentReviews": { "nodes": [] },
            "recentComments": { "nodes": [] }
        });
        for (field, value) in json.as_object().unwrap() {
            full[field] = value.clone();
        }
        serde_json::from_value(full).unwrap()
    }

    fn read(json: serde_json::Value) -> PrActivity {
        activity(&nodes(json), "ooloth")
    }

    /// Shaped like an agent pull request on ooloth/hub: Copilot reviewed it with two inline
    /// comments, and I replied in one thread.
    fn copilot_threads() -> serde_json::Value {
        serde_json::json!({
            "latestOpinionatedReviews": { "nodes": [] },
            "reviewRequests": { "nodes": [] },
            "reviewThreads": { "nodes": [
                { "isResolved": false, "path": "a.rs", "line": 1, "diffSide": "RIGHT", "comments": { "nodes": [
                    { "author": { "__typename": "Bot", "login": "copilot-pull-request-reviewer" }, "createdAt": "2026-05-16T20:33:50Z", "body": "Consider this" },
                    { "author": { "__typename": "User", "login": "ooloth" }, "createdAt": "2026-05-16T22:34:13Z", "body": "Done" }
                ] } },
                { "isResolved": false, "path": "b.rs", "line": 2, "diffSide": "RIGHT", "comments": { "nodes": [
                    { "author": { "__typename": "Bot", "login": "copilot-pull-request-reviewer" }, "createdAt": "2026-05-16T20:33:51Z", "body": "And this" }
                ] } },
                { "isResolved": true, "path": "c.rs", "line": 3, "diffSide": "RIGHT", "comments": { "nodes": [
                    { "author": { "__typename": "User", "login": "ada" }, "createdAt": "2026-05-16T20:33:52Z", "body": "Resolved" }
                ] } }
            ] },
            "recentReviews": { "nodes": [
                { "state": "COMMENTED", "author": { "__typename": "Bot", "login": "copilot-pull-request-reviewer" },
                  "submittedAt": "2026-05-16T20:33:50Z", "body": "## Pull request overview" },
                { "state": "COMMENTED", "author": { "__typename": "User", "login": "ooloth" },
                  "submittedAt": "2026-05-16T22:34:13Z", "body": "" }
            ] },
            "recentComments": { "nodes": [] }
        })
    }

    fn copilot() -> Participant {
        Participant::Bot("copilot-pull-request-reviewer".to_string())
    }

    #[test]
    fn only_unresolved_threads_are_read_each_from_its_newest_comment() {
        let activity = read(copilot_threads());

        let lasts: Vec<_> = activity.open_threads.iter().map(OpenThread::last).collect();
        assert_eq!(lasts, vec![&Participant::Me, &copilot()]);
    }

    #[test]
    fn a_bots_review_body_is_not_feedback_and_my_reply_is_my_latest_activity() {
        let activity = read(copilot_threads());

        assert_eq!(activity.bodies, vec![]);
        assert_eq!(activity.my_latest, Some(at("2026-05-16T22:34:13Z")));
    }

    #[test]
    fn a_persons_review_body_counts_whatever_its_verdict() {
        let activity = read(serde_json::json!({
            "recentReviews": { "nodes": [
                { "state": "APPROVED", "author": { "__typename": "User", "login": "ada" },
                  "submittedAt": "2026-05-16T01:00:00Z", "body": "LGTM, one nit" },
                { "state": "COMMENTED", "author": { "__typename": "User", "login": "grace" },
                  "submittedAt": "2026-05-16T02:00:00Z", "body": "  " },
                { "state": "DISMISSED", "author": { "__typename": "User", "login": "alan" },
                  "submittedAt": "2026-05-16T03:00:00Z", "body": "Withdrawn" }
            ] }
        }));

        assert_eq!(
            activity.bodies,
            vec![ReviewBody {
                by: "ada".to_string(),
                at: at("2026-05-16T01:00:00Z"),
            }]
        );
    }

    #[test]
    fn my_latest_activity_is_my_newest_review_or_top_level_comment() {
        let activity = read(serde_json::json!({
            "recentReviews": { "nodes": [
                { "state": "COMMENTED", "author": { "__typename": "User", "login": "ooloth" },
                  "submittedAt": "2026-05-16T01:00:00Z", "body": "" }
            ] },
            "recentComments": { "nodes": [
                { "author": { "login": "ada" }, "createdAt": "2026-05-16T03:00:00Z" },
                { "author": { "login": "ooloth" }, "createdAt": "2026-05-16T02:00:00Z" },
                { "author": null, "createdAt": "2026-05-16T04:00:00Z" }
            ] }
        }));

        assert_eq!(activity.my_latest, Some(at("2026-05-16T02:00:00Z")));
    }

    #[test]
    fn each_reviewers_latest_verdict_is_read_with_who_gave_it() {
        let activity = read(serde_json::json!({
            "latestOpinionatedReviews": { "nodes": [
                { "state": "APPROVED", "author": { "__typename": "User", "login": "ada" }, "submittedAt": "2026-05-16T01:00:00Z" },
                { "state": "CHANGES_REQUESTED", "author": { "__typename": "Bot", "login": "reviewer-bot" }, "submittedAt": "2026-05-16T02:00:00Z" },
                { "state": "APPROVED", "author": null, "submittedAt": "2026-05-16T03:00:00Z" },
                { "state": "COMMENTED", "author": { "__typename": "User", "login": "grace" }, "submittedAt": "2026-05-16T04:00:00Z" }
            ] }
        }));

        assert_eq!(
            activity.verdicts,
            vec![
                LatestVerdict {
                    by: Participant::Person("ada".to_string()),
                    opinion: Opinion::Approved,
                    at: at("2026-05-16T01:00:00Z"),
                },
                LatestVerdict {
                    by: Participant::Bot("reviewer-bot".to_string()),
                    opinion: Opinion::ChangesRequested,
                    at: at("2026-05-16T02:00:00Z"),
                },
            ]
        );
    }

    /// GitHub stores each reply in a review thread as a review of its own, so a long discussion
    /// pushed later approvals out of the oldest 50 reviews (#379). Verdicts come from each
    /// reviewer's latest one instead, however many replies there are.
    #[test]
    fn verdicts_are_counted_however_many_thread_replies_there_are() {
        let replies: Vec<_> = (0..60)
            .map(|n| serde_json::json!({ "state": "COMMENTED", "author": { "__typename": "User", "login": "ada" },
                "submittedAt": format!("2026-05-16T01:{n:02}:00Z"), "body": "" }))
            .collect();
        let activity = read(serde_json::json!({
            "recentReviews": { "nodes": replies },
            "latestOpinionatedReviews": { "nodes": [
                { "state": "APPROVED", "author": { "__typename": "User", "login": "ada" }, "submittedAt": "2026-05-16T02:00:00Z" }
            ] }
        }));

        assert_eq!(activity.count(Opinion::Approved), 1);
    }

    /// A team's name needs the `read:org` scope, which hub's token does not have, so a team is
    /// read by its kind alone.
    #[test]
    fn the_query_never_asks_for_a_teams_name() {
        assert!(!ACTIVITY_FIELDS.contains("slug"), "{ACTIVITY_FIELDS}");
    }

    #[test]
    fn requests_are_read_as_people_bots_or_teams() {
        let activity = read(serde_json::json!({
            "reviewRequests": { "nodes": [
                { "requestedReviewer": { "__typename": "User", "login": "ada" } },
                { "requestedReviewer": { "__typename": "Bot", "login": "Copilot" } },
                { "requestedReviewer": { "__typename": "Team" } },
                { "requestedReviewer": { "__typename": "EnterpriseTeam" } },
                { "requestedReviewer": null }
            ] }
        }));

        assert_eq!(
            activity.requests,
            vec![
                Requested::Person("ada".to_string()),
                Requested::Bot("Copilot".to_string()),
                Requested::Team("Team".to_string()),
                Requested::Team("EnterpriseTeam".to_string()),
                Requested::Team("an unseen reviewer".to_string()),
            ]
        );
    }

    #[test]
    fn a_thread_whose_comments_all_came_from_deleted_accounts_is_left_out() {
        let activity = read(serde_json::json!({
            "reviewThreads": { "nodes": [
                { "isResolved": false, "path": "a.rs", "line": 1, "diffSide": "RIGHT", "comments": { "nodes": [
                    { "author": null, "createdAt": "2026-05-16T01:00:00Z", "body": "Gone" }
                ] } }
            ] }
        }));

        assert_eq!(activity.open_threads, vec![]);
    }
}
