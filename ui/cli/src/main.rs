use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use domain::{CommentAuthor, TaskId, TaskStatus};

/// Hub CLI — the agent's communication channel with hub.
///
/// This binary exists so agent sessions can read their assigned task, write
/// captain's log entries, and report status back to hub. It does not expose
/// human-owned operations (create, promote, approve, cancel, dispatch) — those
/// belong in the TUI. The command surface here is exactly what an agent's skill
/// file would instruct it to call, nothing more.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read, report, and log — the agent's task interface
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
}

/// The two states an agent may report.
///
/// These are the only status transitions an agent owns. Everything else
/// (done, failed, cancelled, ready) is a human decision made in the TUI.
#[derive(ValueEnum, Clone)]
enum AgentStatus {
    /// Work is complete — hand off to the human for review
    #[value(name = "in-review")]
    InReview,
    /// Cannot continue — human attention needed
    Blocked,
}

impl From<AgentStatus> for TaskStatus {
    fn from(s: AgentStatus) -> Self {
        match s {
            AgentStatus::InReview => TaskStatus::InReview,
            AgentStatus::Blocked => TaskStatus::Blocked,
        }
    }
}

#[derive(Subcommand)]
enum TaskCommands {
    /// Read a task as JSON — call this at session start
    Get {
        /// Task ID: TASK-0001 or plain integer
        id: String,
    },

    /// Register an artifact link for this task
    ///
    /// Appends a URL or file path to the task's links list. Idempotent:
    /// calling with a value that already exists is a no-op.
    Link {
        /// Task ID: TASK-0001 or plain integer
        id: String,

        /// URL or file path to register (e.g. https://github.com/... or ~/.hub/agent-session-logs/TASK-0001-slug.md)
        #[arg(long)]
        value: String,
    },

    /// Report status back to hub
    ///
    /// Use `in-review` when the work is complete; use `blocked` when the
    /// agent cannot continue and needs human attention. Human-owned transitions
    /// (done, failed, cancelled) are handled in the TUI — the agent does not
    /// close its own task.
    Report {
        /// Task ID: TASK-0001 or plain integer
        id: String,

        /// Status to report — only agent-owned transitions are accepted
        #[arg(long)]
        status: AgentStatus,
    },

    /// Append a captain's log entry
    ///
    /// Record choices made, friction encountered, trade-offs taken — anything
    /// the human might wonder about when reviewing the session. This is a
    /// one-way log from agent to human, not a dialogue surface.
    Comment {
        /// Task ID: TASK-0001 or plain integer
        id: String,

        /// Log entry text
        #[arg(long)]
        content: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Task { command } => match command {
            TaskCommands::Get { id } => {
                let task_id = parse_task_ref(&id)?;
                let task = workflows::tasks::get(&task_id)?;
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
            TaskCommands::Link { id, value } => {
                let task_id = parse_task_ref(&id)?;
                workflows::tasks::add_link(&task_id, &value)?;
                println!("TASK_UPDATED {task_id}");
            }
            TaskCommands::Report { id, status } => {
                let task_id = parse_task_ref(&id)?;
                workflows::tasks::update_status(&task_id, status.into())?;
                println!("TASK_UPDATED {task_id}");
            }
            TaskCommands::Comment { id, content } => {
                let task_id = parse_task_ref(&id)?;
                workflows::tasks::add_comment(&task_id, CommentAuthor::Agent, &content)?;
                println!("TASK_UPDATED {task_id}");
            }
        },
    }
    Ok(())
}

/// Accepts `TASK-0001` or a bare integer `1` (zero-padded to four digits).
fn parse_task_ref(s: &str) -> Result<TaskId> {
    if s.starts_with("TASK-") {
        s.parse::<TaskId>().map_err(|e| anyhow::anyhow!("{e}"))
    } else if let Ok(n) = s.parse::<u64>() {
        format!("TASK-{n:04}")
            .parse::<TaskId>()
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else {
        anyhow::bail!("invalid task ID {s:?}: expected TASK-NNNN or a plain integer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case("TASK-0001", "TASK-0001")]
    #[case("TASK-9999", "TASK-9999")]
    #[case("1", "TASK-0001")]
    #[case("42", "TASK-0042")]
    #[case("10000", "TASK-10000")]
    fn parse_task_ref_accepts_valid_formats(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(parse_task_ref(input).unwrap().to_string(), expected);
    }

    #[rstest::rstest]
    #[case("task-0001")]
    #[case("TASK-")]
    #[case("TASK-abc")]
    #[case("abc")]
    #[case("")]
    fn parse_task_ref_rejects_invalid_formats(#[case] input: &str) {
        assert!(parse_task_ref(input).is_err());
    }
}
