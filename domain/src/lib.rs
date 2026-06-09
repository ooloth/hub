pub mod ci;
pub mod gcp;
pub mod issue;
pub mod loki;
pub mod pr;
pub mod serde_helpers;
pub mod session;
pub mod signal_identity;
pub mod task;
pub mod task_origin;
pub mod urgency;

// Re-export duration_secs at the crate root so existing
// #[serde(with = "crate::duration_secs")] attributes in non-domain
// crates continue to resolve (e.g. in any future cross-crate users).
pub use serde_helpers::duration_secs;

pub use ci::*;
pub use gcp::*;
pub use issue::*;
pub use loki::*;
pub use pr::*;
pub use session::*;
pub use signal_identity::*;
pub use task::*;
pub use task_origin::*;
pub use urgency::*;

#[cfg(test)]
mod tests {
    use super::*;

    // ── agent_ready_labels ────────────────────────────────────────────────────

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[rstest::rstest]
    // needs-human-review removed, ready-for-agent added
    #[case(vec![s(NEEDS_HUMAN_REVIEW_LABEL)], vec![s(READY_FOR_AGENT_LABEL)])]
    // already has ready-for-agent and needs-human-review → only needs-human-review removed
    #[case(
        vec![s(NEEDS_HUMAN_REVIEW_LABEL), s(READY_FOR_AGENT_LABEL)],
        vec![s(READY_FOR_AGENT_LABEL)]
    )]
    // already fully ready → unchanged (idempotent)
    #[case(vec![s(READY_FOR_AGENT_LABEL)], vec![s(READY_FOR_AGENT_LABEL)])]
    // no relevant labels → ready-for-agent added, others preserved
    #[case(vec![s("bug"), s("wontfix")], vec![s("bug"), s("wontfix"), s(READY_FOR_AGENT_LABEL)])]
    // empty → only ready-for-agent
    #[case(vec![], vec![s(READY_FOR_AGENT_LABEL)])]
    // unrelated label + needs-human-review → unrelated preserved, needs-human-review removed
    #[case(
        vec![s("bug"), s(NEEDS_HUMAN_REVIEW_LABEL)],
        vec![s("bug"), s(READY_FOR_AGENT_LABEL)]
    )]
    fn agent_ready_labels_cases(#[case] input: Vec<String>, #[case] expected: Vec<String>) {
        assert_eq!(agent_ready_labels(&input), expected);
    }

    #[test]
    fn agent_ready_labels_idempotent() {
        let input = vec![s(NEEDS_HUMAN_REVIEW_LABEL), s("bug")];
        let once = agent_ready_labels(&input);
        let twice = agent_ready_labels(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn agent_ready_labels_never_contains_needs_human_review() {
        // property: result never contains NEEDS_HUMAN_REVIEW_LABEL, regardless of input
        let inputs: &[&[&str]] = &[
            &[NEEDS_HUMAN_REVIEW_LABEL],
            &[NEEDS_HUMAN_REVIEW_LABEL, READY_FOR_AGENT_LABEL],
            &["bug", NEEDS_HUMAN_REVIEW_LABEL, "wontfix"],
            &[],
        ];
        for labels in inputs {
            let input: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
            let result = agent_ready_labels(&input);
            assert!(
                !result.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL),
                "result contained NEEDS_HUMAN_REVIEW_LABEL for input {labels:?}"
            );
            assert!(
                result.iter().any(|l| l == READY_FOR_AGENT_LABEL),
                "result missing READY_FOR_AGENT_LABEL for input {labels:?}"
            );
        }
    }

    // ── dismissed_labels ──────────────────────────────────────────────────────

    #[rstest::rstest]
    // needs-human-review removed, wontfix added
    #[case(vec![s(NEEDS_HUMAN_REVIEW_LABEL)], vec![s(WONTFIX_LABEL)])]
    // already has wontfix and needs-human-review → only needs-human-review removed
    #[case(
        vec![s(NEEDS_HUMAN_REVIEW_LABEL), s(WONTFIX_LABEL)],
        vec![s(WONTFIX_LABEL)]
    )]
    // already dismissed → unchanged (idempotent)
    #[case(vec![s(WONTFIX_LABEL)], vec![s(WONTFIX_LABEL)])]
    // no relevant labels → wontfix added, others preserved
    #[case(vec![s("bug")], vec![s("bug"), s(WONTFIX_LABEL)])]
    // empty → only wontfix
    #[case(vec![], vec![s(WONTFIX_LABEL)])]
    // unrelated label + needs-human-review → unrelated preserved, needs-human-review removed
    #[case(
        vec![s("bug"), s(NEEDS_HUMAN_REVIEW_LABEL)],
        vec![s("bug"), s(WONTFIX_LABEL)]
    )]
    fn dismissed_labels_cases(#[case] input: Vec<String>, #[case] expected: Vec<String>) {
        assert_eq!(dismissed_labels(&input), expected);
    }

    #[test]
    fn dismissed_labels_idempotent() {
        let input = vec![s(NEEDS_HUMAN_REVIEW_LABEL), s("bug")];
        let once = dismissed_labels(&input);
        let twice = dismissed_labels(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn dismissed_labels_never_contains_needs_human_review() {
        let inputs: &[&[&str]] = &[
            &[NEEDS_HUMAN_REVIEW_LABEL],
            &[NEEDS_HUMAN_REVIEW_LABEL, WONTFIX_LABEL],
            &["bug", NEEDS_HUMAN_REVIEW_LABEL],
            &[],
        ];
        for labels in inputs {
            let input: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
            let result = dismissed_labels(&input);
            assert!(
                !result.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL),
                "result contained NEEDS_HUMAN_REVIEW_LABEL for input {labels:?}"
            );
            assert!(
                result.iter().any(|l| l == WONTFIX_LABEL),
                "result missing WONTFIX_LABEL for input {labels:?}"
            );
        }
    }

    // ── task_id ───────────────────────────────────────────────────────────────

    #[test]
    fn task_id_from_str_accepts_valid_format() {
        assert!("TASK-0001".parse::<TaskId>().is_ok());
        assert!("TASK-9999".parse::<TaskId>().is_ok());
        assert!("TASK-10000".parse::<TaskId>().is_ok());
    }

    #[test]
    fn task_id_from_str_rejects_invalid_format() {
        assert!("task-0001".parse::<TaskId>().is_err());
        assert!("TASK-".parse::<TaskId>().is_err());
        assert!("TASK-abc".parse::<TaskId>().is_err());
        assert!("0001".parse::<TaskId>().is_err());
    }

    #[test]
    fn task_id_display_round_trips() {
        let id: TaskId = "TASK-0042".parse().unwrap();
        assert_eq!(id.to_string(), "TASK-0042");
    }

    // ── comment_author ────────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case("human", CommentAuthor::Human)]
    #[case("agent", CommentAuthor::Agent)]
    fn comment_author_serde_round_trips(#[case] s: &str, #[case] expected: CommentAuthor) {
        let json = format!(r#""{s}""#);
        let author: CommentAuthor = serde_json::from_str(&json).unwrap();
        assert_eq!(author, expected);
        let back = serde_json::to_string(&author).unwrap();
        assert_eq!(back, json);
    }

    #[rstest::rstest]
    #[case("human", Ok(CommentAuthor::Human))]
    #[case("agent", Ok(CommentAuthor::Agent))]
    #[case("bot", Err(()))]
    fn comment_author_from_str(#[case] s: &str, #[case] expected: Result<CommentAuthor, ()>) {
        assert_eq!(s.parse::<CommentAuthor>().map_err(|_| ()), expected);
    }

    // ── task_status_urgency ───────────────────────────────────────────────────

    #[test]
    fn task_status_review_has_high_urgency() {
        assert_eq!(TaskStatus::InReview.urgency(), Urgency::High);
    }

    #[test]
    fn task_status_blocked_has_high_urgency() {
        assert_eq!(TaskStatus::Blocked.urgency(), Urgency::High);
    }

    #[test]
    fn task_status_in_progress_has_low_urgency() {
        assert_eq!(TaskStatus::InProgress.urgency(), Urgency::Low);
    }

    #[test]
    fn task_status_failed_has_low_urgency() {
        assert_eq!(TaskStatus::Failed.urgency(), Urgency::Low);
    }

    // ── task_status_is_terminal ───────────────────────────────────────────────

    #[rstest::rstest]
    #[case(TaskStatus::Done, true)]
    #[case(TaskStatus::Failed, true)]
    #[case(TaskStatus::Cancelled, true)]
    #[case(TaskStatus::Backlog, false)]
    #[case(TaskStatus::Ready, false)]
    #[case(TaskStatus::InProgress, false)]
    #[case(TaskStatus::Blocked, false)]
    #[case(TaskStatus::InReview, false)]
    fn task_status_is_terminal(#[case] status: TaskStatus, #[case] expected: bool) {
        assert_eq!(status.is_terminal(), expected);
    }

    // ── task_status_display_and_fromstr ───────────────────────────────────────

    #[rstest::rstest]
    #[case(TaskStatus::Backlog, "backlog")]
    #[case(TaskStatus::Ready, "ready")]
    #[case(TaskStatus::InProgress, "in-progress")]
    #[case(TaskStatus::Blocked, "blocked")]
    #[case(TaskStatus::InReview, "in-review")]
    #[case(TaskStatus::Done, "done")]
    #[case(TaskStatus::Failed, "failed")]
    #[case(TaskStatus::Cancelled, "cancelled")]
    fn task_status_display_and_fromstr_roundtrip(#[case] status: TaskStatus, #[case] s: &str) {
        assert_eq!(status.to_string(), s);
        assert_eq!(s.parse::<TaskStatus>(), Ok(status));
    }

    #[test]
    fn task_status_fromstr_rejects_unknown() {
        assert!("unknown".parse::<TaskStatus>().is_err());
    }

    // ── repo_slug ─────────────────────────────────────────────────────────────

    #[test]
    fn repo_slug_new_formats_owner_and_repo() {
        let slug = RepoSlug::new("ooloth", "hub");
        assert_eq!(slug.to_string(), "ooloth/hub");
    }

    #[test]
    #[should_panic(expected = "owner must not be empty")]
    fn repo_slug_new_panics_on_empty_owner() {
        RepoSlug::new("", "hub");
    }

    #[test]
    #[should_panic(expected = "repo must not be empty")]
    fn repo_slug_new_panics_on_empty_repo() {
        RepoSlug::new("ooloth", "");
    }

    // ── merge_blocker serde default ───────────────────────────────────────────

    #[rstest::rstest]
    #[case(Some(r#""Conflict""#), Some(MergeBlocker::Conflict))]
    #[case(Some(r#""Behind""#), Some(MergeBlocker::Behind))]
    #[case(Some(r#""Blocked""#), Some(MergeBlocker::Blocked))]
    #[case(None, None)]
    fn merge_blocker_deserializes_with_default(
        #[case] field_value: Option<&str>,
        #[case] expected: Option<MergeBlocker>,
    ) {
        let blocker_field = match field_value {
            Some(v) => format!(r#","merge_blocker":{v}"#),
            None => String::new(),
        };
        let json = format!(
            r#"{{"number":1,"title":"t","repo":"o/r","url":"u","age":0,"urgency":"Low","kind":"Mine","author":"a","head_branch":"h","base_branch":"b"{blocker_field}}}"#
        );
        let pr: PullRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(pr.merge_blocker, expected);
    }

    // ── encode_project_path ───────────────────────────────────────────────────

    #[test]
    fn encode_project_path_replaces_slashes_with_dashes() {
        assert_eq!(
            encode_project_path("/Users/michael/Repos/ooloth/hub"),
            "-Users-michael-Repos-ooloth-hub"
        );
    }

    #[test]
    fn encode_project_path_empty_string_returns_empty() {
        assert_eq!(encode_project_path(""), "");
    }

    // ── parse_session_jsonl ───────────────────────────────────────────────────

    fn fixture_jsonl() -> &'static str {
        concat!(
            "{\"type\":\"permission-mode\",\"permissionMode\":\"bypassPermissions\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Fix the auth bug\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"Let me look at auth\",\"signature\":\"sig\"},{\"type\":\"text\",\"text\":\"I'll read the auth file.\"},{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\"input\":{\"file_path\":\"/src/auth.rs\"}}]}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"pub fn auth() {}\"}]}}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t2\",\"name\":\"Bash\",\"input\":{\"command\":\"cargo test\",\"description\":\"run\"}},{\"type\":\"tool_use\",\"id\":\"t3\",\"name\":\"Edit\",\"input\":{\"file_path\":\"/src/auth.rs\",\"old_string\":\"old\",\"new_string\":\"new\"}}]}}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t2\",\"content\":\"5 passed\"},{\"type\":\"tool_result\",\"tool_use_id\":\"t3\",\"is_error\":true,\"content\":\"not found\"}]}}\n",
            "{\"type\":\"system\",\"systemPrompt\":\"...\"}\n",
        )
    }

    #[test]
    fn parse_session_jsonl_produces_correct_block_sequence() {
        let blocks = parse_session_jsonl(fixture_jsonl());
        assert_eq!(blocks.len(), 9);
        assert!(matches!(&blocks[0], StreamBlock::HumanTurn(t) if t == "Fix the auth bug"));
        assert!(
            matches!(&blocks[1], StreamBlock::AssistantThinking(t) if t == "Let me look at auth")
        );
        assert!(matches!(&blocks[2], StreamBlock::AssistantText(t) if t.contains("read the auth")));
        assert!(
            matches!(&blocks[3], StreamBlock::ToolCall { name, summary } if name == "Read" && summary == "/src/auth.rs")
        );
        assert!(
            matches!(&blocks[4], StreamBlock::ToolResult { is_error: false, content } if content == "pub fn auth() {}")
        );
        assert!(
            matches!(&blocks[5], StreamBlock::ToolCall { name, summary } if name == "Bash" && summary == "cargo test")
        );
        assert!(
            matches!(&blocks[6], StreamBlock::ToolCall { name, summary } if name == "Edit" && summary == "/src/auth.rs")
        );
        assert!(
            matches!(&blocks[7], StreamBlock::ToolResult { is_error: false, content } if content == "5 passed")
        );
        assert!(
            matches!(&blocks[8], StreamBlock::ToolResult { is_error: true, content } if content == "not found")
        );
    }

    #[test]
    fn parse_session_jsonl_empty_input_returns_empty_vec() {
        assert_eq!(parse_session_jsonl("").len(), 0);
    }

    #[test]
    fn parse_session_jsonl_ignores_unknown_top_level_types() {
        let jsonl = "{\"type\":\"permission-mode\"}\n{\"type\":\"system\",\"systemPrompt\":\"...\"}\n{\"type\":\"file-history-snapshot\"}\n{\"type\":\"ai-title\"}";
        assert_eq!(parse_session_jsonl(jsonl).len(), 0);
    }

    #[test]
    fn parse_session_jsonl_tool_result_array_content_is_joined() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":[{\"type\":\"text\",\"text\":\"line one\"},{\"type\":\"text\",\"text\":\"line two\"}]}]}}";
        let blocks = parse_session_jsonl(jsonl);
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], StreamBlock::ToolResult { is_error: false, content } if content == "line one\nline two")
        );
    }

    #[test]
    fn parse_session_jsonl_tool_result_is_error_flag() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"is_error\":true,\"content\":\"boom\"}]}}";
        let blocks = parse_session_jsonl(jsonl);
        assert!(matches!(
            &blocks[0],
            StreamBlock::ToolResult { is_error: true, .. }
        ));
    }

    #[test]
    fn parse_session_jsonl_skips_malformed_json_lines() {
        let jsonl = "not json\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\nalso not json";
        let blocks = parse_session_jsonl(jsonl);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], StreamBlock::HumanTurn(t) if t == "hello"));
    }
}
