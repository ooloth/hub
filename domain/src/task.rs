use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::urgency::Urgency;

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
    InReview,
    Done,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn urgency(self) -> Urgency {
        match self {
            Self::InReview | Self::Blocked => Urgency::High,
            _ => Urgency::Low,
        }
    }

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
}
