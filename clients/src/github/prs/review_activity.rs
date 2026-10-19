//! The newest reviews and top-level comments of a pull request, read into who did what and when.
use chrono::{DateTime, Utc};
use domain::review_waiting::{Review, ReviewActivity, Reviewer, ReviewerKind, Verdict};
use serde::Deserialize;

/// `reviews(last: 100)`: the newest reviews, which decide whether one waits on me however many
/// the pull request has. Each reply in a review thread is a review of its own.
#[derive(Deserialize, Default)]
pub(super) struct RecentReviewConnection {
    nodes: Vec<RecentReviewNode>,
}

#[derive(Deserialize)]
struct RecentReviewNode {
    state: String,
    /// `None` if the reviewer's account was deleted.
    author: Option<Actor>,
    /// `None` for a `PENDING` review, which has not been submitted.
    #[serde(rename = "submittedAt")]
    submitted_at: Option<String>,
    body: String,
    comments: TotalCount,
}

#[derive(Deserialize)]
struct Actor {
    #[serde(rename = "__typename")]
    typename: String,
    login: String,
}

#[derive(Deserialize)]
struct TotalCount {
    #[serde(rename = "totalCount")]
    total_count: u32,
}

/// `comments(last: 100)`: the newest top-level comments, read only for whether I replied.
#[derive(Deserialize, Default)]
pub(super) struct RecentCommentConnection {
    nodes: Vec<RecentCommentNode>,
}

#[derive(Deserialize)]
struct RecentCommentNode {
    /// `None` if the commenter's account was deleted.
    author: Option<CommentAuthor>,
    #[serde(rename = "createdAt")]
    created_at: String,
}

#[derive(Deserialize)]
struct CommentAuthor {
    login: String,
}

/// Who did what on a pull request, as `me` sees it.
///
/// A review that was not submitted, has no author or a state GitHub does not document, or a time
/// that does not parse, says nothing about whose move it is, so it is left out.
pub(super) fn review_activity(
    reviews: &RecentReviewConnection,
    comments: &RecentCommentConnection,
    me: &str,
) -> ReviewActivity {
    ReviewActivity {
        reviews: reviews
            .nodes
            .iter()
            .filter_map(|node| review(node, me))
            .collect(),
        my_comments: comments
            .nodes
            .iter()
            .filter(|node| node.author.as_ref().is_some_and(|a| a.login == me))
            .filter_map(|node| time(&node.created_at))
            .collect(),
    }
}

fn review(node: &RecentReviewNode, me: &str) -> Option<Review> {
    let author = node.author.as_ref()?;
    let verdict = match node.state.as_str() {
        "APPROVED" => Verdict::Approved,
        "CHANGES_REQUESTED" => Verdict::ChangesRequested,
        "COMMENTED" => Verdict::Commented,
        "DISMISSED" => Verdict::Dismissed,
        _ => return None,
    };
    let by = if author.login == me {
        Reviewer::Me
    } else {
        Reviewer::Other {
            login: author.login.clone(),
            kind: if author.typename == "Bot" {
                ReviewerKind::Bot
            } else {
                ReviewerKind::Person
            },
        }
    };
    Some(Review {
        by,
        verdict,
        inline_comments: node.comments.total_count,
        has_body: !node.body.trim().is_empty(),
        submitted: time(node.submitted_at.as_deref()?)?,
    })
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
        time(rfc3339).unwrap()
    }

    fn reviews(json: serde_json::Value) -> RecentReviewConnection {
        serde_json::from_value(serde_json::json!({ "nodes": json })).unwrap()
    }

    fn comments(json: serde_json::Value) -> RecentCommentConnection {
        serde_json::from_value(serde_json::json!({ "nodes": json })).unwrap()
    }

    /// Shaped like hub#53 and a later pull request read on 2026-10-19: Copilot's overview, a
    /// Copilot review with a comment, and my reply in its thread.
    fn copilot_then_my_reply() -> RecentReviewConnection {
        reviews(serde_json::json!([
            { "state": "COMMENTED", "author": { "__typename": "Bot", "login": "copilot-pull-request-reviewer" },
              "submittedAt": "2026-05-16T00:19:51Z", "body": "## Pull request overview", "comments": { "totalCount": 0 } },
            { "state": "COMMENTED", "author": { "__typename": "Bot", "login": "copilot-pull-request-reviewer" },
              "submittedAt": "2026-05-16T20:33:50Z", "body": "", "comments": { "totalCount": 2 } },
            { "state": "COMMENTED", "author": { "__typename": "User", "login": "ooloth" },
              "submittedAt": "2026-05-16T22:34:13Z", "body": "", "comments": { "totalCount": 1 } }
        ]))
    }

    #[test]
    fn reviews_are_read_as_mine_or_someone_elses_with_whether_they_are_a_bot() {
        let activity = review_activity(
            &copilot_then_my_reply(),
            &RecentCommentConnection::default(),
            "ooloth",
        );

        let copilot = Reviewer::Other {
            login: "copilot-pull-request-reviewer".to_string(),
            kind: ReviewerKind::Bot,
        };
        assert_eq!(
            activity.reviews,
            vec![
                Review {
                    by: copilot.clone(),
                    verdict: Verdict::Commented,
                    inline_comments: 0,
                    has_body: true,
                    submitted: at("2026-05-16T00:19:51Z"),
                },
                Review {
                    by: copilot,
                    verdict: Verdict::Commented,
                    inline_comments: 2,
                    has_body: false,
                    submitted: at("2026-05-16T20:33:50Z"),
                },
                Review {
                    by: Reviewer::Me,
                    verdict: Verdict::Commented,
                    inline_comments: 1,
                    has_body: false,
                    submitted: at("2026-05-16T22:34:13Z"),
                },
            ]
        );
    }

    #[test]
    fn a_person_is_read_as_a_person() {
        let activity = review_activity(
            &reviews(serde_json::json!([
                { "state": "CHANGES_REQUESTED", "author": { "__typename": "User", "login": "ada" },
                  "submittedAt": "2026-05-16T00:00:00Z", "body": "  ", "comments": { "totalCount": 0 } }
            ])),
            &RecentCommentConnection::default(),
            "ooloth",
        );

        assert_eq!(
            activity.reviews[0].by,
            Reviewer::Other {
                login: "ada".to_string(),
                kind: ReviewerKind::Person
            }
        );
        assert!(!activity.reviews[0].has_body, "a blank body is no body");
    }

    #[rstest::rstest]
    #[case::pending(serde_json::json!({ "state": "PENDING", "author": { "__typename": "User", "login": "ada" },
        "submittedAt": null, "body": "", "comments": { "totalCount": 1 } }))]
    #[case::deleted_account(serde_json::json!({ "state": "COMMENTED", "author": null,
        "submittedAt": "2026-05-16T00:00:00Z", "body": "x", "comments": { "totalCount": 1 } }))]
    #[case::undocumented_state(serde_json::json!({ "state": "SOMETHING_NEW", "author": { "__typename": "User", "login": "ada" },
        "submittedAt": "2026-05-16T00:00:00Z", "body": "x", "comments": { "totalCount": 1 } }))]
    fn a_review_that_says_nothing_about_whose_move_it_is_is_left_out(
        #[case] node: serde_json::Value,
    ) {
        let activity = review_activity(
            &reviews(serde_json::json!([node])),
            &RecentCommentConnection::default(),
            "ooloth",
        );

        assert_eq!(activity.reviews, vec![]);
    }

    #[test]
    fn only_my_top_level_comments_count_as_replies() {
        let activity = review_activity(
            &RecentReviewConnection::default(),
            &comments(serde_json::json!([
                { "author": { "login": "ada" }, "createdAt": "2026-05-16T01:00:00Z" },
                { "author": { "login": "ooloth" }, "createdAt": "2026-05-16T02:00:00Z" },
                { "author": null, "createdAt": "2026-05-16T03:00:00Z" }
            ])),
            "ooloth",
        );

        assert_eq!(activity.my_comments, vec![at("2026-05-16T02:00:00Z")]);
    }
}
