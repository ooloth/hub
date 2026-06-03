use secrecy::Secret;
use serde::{Deserialize, Serialize};

pub mod duration_secs {
    use chrono::Duration;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(d: &Duration, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        d.num_seconds().serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = i64::deserialize(d)?;
        Ok(Duration::seconds(secs))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoSlug(String);

impl RepoSlug {
    pub fn new(owner: &str, repo: &str) -> Self {
        assert!(!owner.is_empty(), "owner must not be empty");
        assert!(!repo.is_empty(), "repo must not be empty");
        Self(format!("{owner}/{repo}"))
    }
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrKind {
    Mine,
    MyDraft,
    ToReview,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CiStatus {
    Success,
    Failure,
    Pending,
    Neutral,
}

/// Why a pull request cannot be merged right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MergeBlocker {
    /// Branch has a merge conflict (mergeStateStatus: DIRTY).
    Conflict,
    /// Branch is behind the base branch (mergeStateStatus: BEHIND).
    Behind,
    /// Merge is blocked by a branch protection rule (mergeStateStatus: BLOCKED).
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub patch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewComment {
    pub author: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewThread {
    pub path: String,
    pub line: Option<u32>,
    pub comments: Vec<ReviewComment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub kind: PrKind,
    pub author: String,
    pub review_decision: Option<ReviewDecision>,
    #[serde(default)]
    pub approval_count: u32,
    #[serde(default)]
    pub comment_count: u32,
    pub head_branch: String,
    pub base_branch: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub ci_status: Option<CiStatus>,
    #[serde(default)]
    pub changed_files: Vec<ChangedFile>,
    #[serde(default)]
    pub total_changed_files: u32,
    #[serde(default)]
    pub review_threads: Vec<ReviewThread>,
    #[serde(default)]
    pub pr_comments: Vec<ReviewComment>,
    #[serde(default)]
    pub merge_blocker: Option<MergeBlocker>,
}

pub const NEEDS_HUMAN_REVIEW_LABEL: &str = "status:needs-human-review";
pub const READY_FOR_AGENT_LABEL: &str = "status:ready-for-agent";
pub const WONTFIX_LABEL: &str = "wontfix";

/// Returns the label set that marks an issue as ready for an agent:
/// removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `READY_FOR_AGENT_LABEL`
/// (if not already present). All other labels are preserved in order. Idempotent.
pub fn agent_ready_labels(labels: &[String]) -> Vec<String> {
    let mut result: Vec<String> = labels
        .iter()
        .filter(|l| l.as_str() != NEEDS_HUMAN_REVIEW_LABEL)
        .cloned()
        .collect();
    if !result.iter().any(|l| l.as_str() == READY_FOR_AGENT_LABEL) {
        result.push(READY_FOR_AGENT_LABEL.to_string());
    }
    result
}

/// Returns the label set that dismisses an issue as won't fix:
/// removes `NEEDS_HUMAN_REVIEW_LABEL` (if present) and adds `WONTFIX_LABEL`
/// (if not already present). All other labels are preserved in order. Idempotent.
pub fn dismissed_labels(labels: &[String]) -> Vec<String> {
    let mut result: Vec<String> = labels
        .iter()
        .filter(|l| l.as_str() != NEEDS_HUMAN_REVIEW_LABEL)
        .cloned()
        .collect();
    if !result.iter().any(|l| l.as_str() == WONTFIX_LABEL) {
        result.push(WONTFIX_LABEL.to_string());
    }
    result
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub repo: RepoSlug,
    pub url: String,
    pub author: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub labels: Vec<String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinearIssue {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiFailure {
    pub repo: RepoSlug,
    pub workflow_name: String,
    pub job_name: Option<String>,
    pub step_name: Option<String>,
    pub error: Option<String>,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}

/// A Loki query to run for one monitoring scenario (e.g. app errors, worker panics).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiQuery {
    pub title: String,
    pub query: String,
    pub lookback: String,
    /// Stream label key used to extract a human-readable message from each entry.
    /// Falls back to the raw log line when absent. Defaults to `"message"`.
    pub message_field: String,
}

/// A GitHub repository configured for the github-prs workflow, with the PR authors to exclude.
#[derive(Clone, Debug)]
pub struct GithubPrsRepo {
    pub repo: String,
    pub exclude_authors: Vec<String>,
}

/// All Loki queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEnv {
    pub project: String,
    pub env: String,
    pub endpoint: String,
    #[serde(skip)]
    pub token: Option<Secret<String>>,
    pub grafana_url: Option<String>,
    pub queries: Vec<LokiQuery>,
}

/// One log entry returned by a Loki query.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LokiEntry {
    pub title: String,
    pub project: String,
    pub env: String,
    /// The `message` stream label — stable error category, used as the grouping key.
    pub message: String,
    /// Raw JSON log line, passed to investigation agents for context.
    pub line: String,
    pub lookback: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
}

/// A GCP Cloud Logging query to run for one monitoring scenario.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpQuery {
    pub title: String,
    /// Raw GCP Logging filter string, fully user-controlled.
    pub query: String,
    pub lookback: String,
    /// JSON payload key used to extract a human-readable message from each entry.
    /// Falls back to text_payload first line, then the raw log line. Defaults to `"message"`.
    pub message_field: String,
}

/// All GCP Cloud Logging queries configured for one deployment environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpEnv {
    pub project: String,
    pub env: String,
    /// GCP project ID, used as the `resourceNames` scope for API calls.
    pub gcp_project: String,
    /// GCP region — used for GCP Console log links. Optional.
    pub gcp_region: Option<String>,
    pub queries: Vec<GcpQuery>,
}

/// One log entry returned by a GCP Cloud Logging query.
/// Emitted once per raw log line; the display layer groups by `message`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GcpEntry {
    pub title: String,
    pub project: String,
    pub env: String,
    /// Display + grouping key extracted from the log payload.
    pub message: String,
    /// Raw JSON log line, passed to investigation agents for context.
    pub line: String,
    pub lookback: String,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
    pub url: String,
    /// GCP cloud project ID (e.g. "rp006-prod-49a893d8"), distinct from the hub project name.
    #[serde(default)]
    pub gcp_project: String,
}

/// A task managed by hub's agent dispatch system, e.g. "TASK-0001".
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    /// Constructs a `TaskId` from a value produced by the `tasks` SQLite table.
    /// Panics if the DB-generated value somehow violates the TASK-NNNN invariant.
    pub fn from_db(s: String) -> Self {
        assert!(
            s.starts_with("TASK-") && s.len() > 5 && s[5..].bytes().all(|b| b.is_ascii_digit()),
            "invalid task ID from DB: {s}"
        );
        Self(s)
    }
}

impl std::str::FromStr for TaskId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("TASK-") && s.len() > 5 && s[5..].bytes().all(|b| b.is_ascii_digit()) {
            Ok(Self(s.to_string()))
        } else {
            Err(format!(
                "invalid task ID {s:?}: expected TASK- followed by one or more digits"
            ))
        }
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Backlog,
    Ready,
    InProgress,
    Blocked,
    Review,
    Done,
    Archived,
}

impl TaskStatus {
    pub fn urgency(self) -> Urgency {
        match self {
            Self::Review | Self::Blocked => Urgency::High,
            _ => Urgency::Low,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::InProgress => "in-progress",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Done => "done",
            Self::Archived => "archived",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "backlog" => Ok(Self::Backlog),
            "ready" => Ok(Self::Ready),
            "in-progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "review" => Ok(Self::Review),
            "done" => Ok(Self::Done),
            "archived" => Ok(Self::Archived),
            _ => Err(format!("unknown task status: {s:?}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Implement,
    Debug,
    General,
}

impl std::fmt::Display for TaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Implement => "implement",
            Self::Debug => "debug",
            Self::General => "general",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for TaskKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "implement" => Ok(Self::Implement),
            "debug" => Ok(Self::Debug),
            "general" => Ok(Self::General),
            _ => Err(format!("unknown task kind: {s:?}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub kind: TaskKind,
    pub session_id: Option<String>,
    #[serde(with = "duration_secs")]
    pub age: chrono::Duration,
    pub urgency: Urgency,
}

/// One block of content from an agent session transcript.
#[derive(Clone, Debug)]
pub enum StreamBlock {
    HumanTurn(String),
    AssistantText(String),
    AssistantThinking(String),
    ToolCall { name: String, summary: String },
    ToolResult { is_error: bool, content: String },
}

/// Encodes an absolute path into the Claude project path segment used in session file paths.
/// Replaces every `/` with `-`, including the leading one.
pub fn encode_project_path(cwd: &str) -> String {
    cwd.replace('/', "-")
}

/// Parses a Claude Code session JSONL file into a sequence of stream blocks.
/// Malformed lines and unknown top-level types are silently skipped.
pub fn parse_session_jsonl(text: &str) -> Vec<StreamBlock> {
    let mut blocks = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match obj.get("type").and_then(|t| t.as_str()) {
            Some("assistant") => parse_assistant_message(&obj, &mut blocks),
            Some("user") => parse_user_message(&obj, &mut blocks),
            _ => {}
        }
    }
    blocks
}

fn parse_assistant_message(obj: &serde_json::Value, blocks: &mut Vec<StreamBlock>) {
    let Some(arr) = obj
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
        return;
    };
    for block in arr {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("thinking") => {
                let text = block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                if !text.is_empty() {
                    blocks.push(StreamBlock::AssistantThinking(text.to_string()));
                }
            }
            Some("text") => {
                let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if !text.is_empty() {
                    blocks.push(StreamBlock::AssistantText(text.to_string()));
                }
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let input = block
                    .get("input")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let summary = format_tool_input(&name, &input);
                blocks.push(StreamBlock::ToolCall { name, summary });
            }
            _ => {}
        }
    }
}

fn parse_user_message(obj: &serde_json::Value, blocks: &mut Vec<StreamBlock>) {
    let Some(content) = obj.get("message").and_then(|m| m.get("content")) else {
        return;
    };
    if let Some(text) = content.as_str() {
        if !text.is_empty() {
            blocks.push(StreamBlock::HumanTurn(text.to_string()));
        }
    } else if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                let is_error = block
                    .get("is_error")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false);
                let content = extract_tool_result_content(block);
                blocks.push(StreamBlock::ToolResult { is_error, content });
            }
            // text blocks in user messages are system/skill injections — skip
        }
    }
}

fn format_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => input
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        "Read" | "Write" | "Edit" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        _ => {
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 120 {
                format!("{}…", &s[..120])
            } else {
                s
            }
        }
    }
}

fn extract_tool_result_content(block: &serde_json::Value) -> String {
    match block.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

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

    // ── task_status_urgency ───────────────────────────────────────────────────

    #[test]
    fn task_status_review_has_high_urgency() {
        assert_eq!(TaskStatus::Review.urgency(), Urgency::High);
    }

    #[test]
    fn task_status_blocked_has_high_urgency() {
        assert_eq!(TaskStatus::Blocked.urgency(), Urgency::High);
    }

    #[test]
    fn task_status_in_progress_has_low_urgency() {
        assert_eq!(TaskStatus::InProgress.urgency(), Urgency::Low);
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
