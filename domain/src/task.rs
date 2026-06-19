use chrono::Duration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::urgency::Urgency;

/// Namespace UUID for deriving deterministic Claude Code session IDs via uuid5.
/// Using this constant ensures the same task always maps to the same session ID,
/// making session resume reliable across re-dispatches.
pub const TASK_DISPATCH_NS: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x14, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

/// A task managed by hub's agent dispatch system, e.g. "TASK-0001".
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    /// Constructs a `TaskId` from a value produced by the `tasks` `SQLite` table.
    ///
    /// # Panics
    /// Panics if the DB-generated value violates the `TASK-NNNN` invariant.
    #[must_use]
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

impl TaskId {
    /// Returns a deterministic UUID v5 for this task's Claude Code session.
    /// The same task ID always produces the same UUID, enabling reliable resume
    /// via `claude --resume <session-id>` after a crash or re-dispatch.
    #[must_use]
    pub fn session_id(&self) -> Uuid {
        Uuid::new_v5(&TASK_DISPATCH_NS, self.0.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Backlog,
    Ready,
    InProgress,
    Blocked,
    InReview,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    #[must_use]
    pub fn urgency(self) -> Urgency {
        match self {
            Self::InReview | Self::Blocked => Urgency::High,
            _ => Urgency::Low,
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::InProgress => "in-progress",
            Self::Blocked => "blocked",
            Self::InReview => "in-review",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
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
            "in-review" => Ok(Self::InReview),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unknown task status: {s:?}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Review,
    Implement,
    Debug,
}

impl TaskKind {
    /// Returns the preferred Claude model for this task kind.
    ///
    /// Debug tasks use Opus — they require the most capable reasoning for root-cause
    /// analysis. Review and Implement tasks use Sonnet for a better cost/quality balance.
    ///
    /// When multi-runner support is added, this becomes `agent_config()` returning an
    /// `AgentConfig { runner, model }`. The call sites are identical; the rename is
    /// mechanical.
    #[must_use]
    pub fn model(self) -> &'static str {
        match self {
            Self::Debug => "claude-opus-4-8",
            Self::Review | Self::Implement => "claude-sonnet-4-6",
        }
    }
}

impl std::fmt::Display for TaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Review => "review",
            Self::Implement => "implement",
            Self::Debug => "debug",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for TaskKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "review" => Ok(Self::Review),
            "implement" => Ok(Self::Implement),
            "debug" => Ok(Self::Debug),
            _ => Err(format!("unknown task kind: {s:?}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentAuthor {
    Human,
    Agent,
}

impl std::fmt::Display for CommentAuthor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Human => f.write_str("human"),
            Self::Agent => f.write_str("agent"),
        }
    }
}

impl std::str::FromStr for CommentAuthor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(Self::Human),
            "agent" => Ok(Self::Agent),
            _ => Err(format!("unknown comment author: {s:?}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskComment {
    pub id: i64,
    pub author: CommentAuthor,
    pub content: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: TaskStatus,
    pub kind: TaskKind,
    pub session_id: Option<String>,
    #[serde(default)]
    pub repo: Option<crate::pr::RepoSlug>,
    /// The signal this task was created from (provenance). Defaults to `Idea`
    /// for blank tasks and for historical rows with no persisted origin.
    #[serde(default)]
    pub origin: crate::task_origin::TaskOrigin,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(with = "crate::serde_helpers::duration_secs")]
    pub age: Duration,
    pub urgency: Urgency,
    #[serde(default)]
    pub comments: Vec<TaskComment>,
}

/// A task that is ready to be dispatched — the minimal data dispatch needs.
/// Returned by the store when querying for the next claimable task.
#[derive(Clone, Debug)]
pub struct ReadyTask {
    pub id: TaskId,
    pub repo: Option<crate::pr::RepoSlug>,
    pub kind: TaskKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case(TaskKind::Debug, "claude-opus-4-8")]
    #[case(TaskKind::Implement, "claude-sonnet-4-6")]
    #[case(TaskKind::Review, "claude-sonnet-4-6")]
    fn task_kind_model(#[case] kind: TaskKind, #[case] expected: &str) {
        assert_eq!(kind.model(), expected);
    }

    #[test]
    fn session_id_is_deterministic() {
        let id: TaskId = "TASK-0001".parse().unwrap();
        assert_eq!(id.session_id(), id.session_id());
    }

    #[test]
    fn session_id_differs_across_tasks() {
        let a: TaskId = "TASK-0001".parse().unwrap();
        let b: TaskId = "TASK-0002".parse().unwrap();
        assert_ne!(a.session_id(), b.session_id());
    }

    #[test]
    fn session_id_is_uuid_v5() {
        let id: TaskId = "TASK-0001".parse().unwrap();
        assert_eq!(id.session_id().get_version_num(), 5);
    }
}
