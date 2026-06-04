use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use domain::{CommentAuthor, TaskId, TaskKind, TaskStatus};

#[derive(Parser)]
#[command(version, about = "Hub CLI — agent task management and status tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage agent tasks
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    /// Create a new task in backlog status
    Create {
        /// Task title
        #[arg(long)]
        title: String,

        /// Task type: implement, debug, or general
        #[arg(long = "type", default_value = "general")]
        kind: TaskKind,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Link to a related issue (may be specified multiple times)
        #[arg(long = "issue-link")]
        issue_links: Vec<String>,
    },

    /// Print a task as JSON (accepts TASK-0001 or bare integer 1)
    Get {
        /// Task ID: TASK-0001 or plain integer
        id: String,
    },

    /// Transition a task to a new status
    Update {
        /// Task ID: TASK-0001 or plain integer
        id: String,

        /// New status
        #[arg(long)]
        status: String,
    },

    /// Append a comment to a task
    Comment {
        /// Task ID: TASK-0001 or plain integer
        id: String,

        /// Comment author: human or agent
        #[arg(long)]
        author: String,

        /// Comment text
        #[arg(long)]
        content: String,
    },

    /// Transition a task from backlog to ready status
    Ready {
        /// Task ID (e.g. TASK-0001)
        id: TaskId,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Task { command } => match command {
            TaskCommands::Create {
                title,
                kind,
                description,
                issue_links,
            } => {
                let id =
                    workflows::tasks::create(&title, kind, description.as_deref(), &issue_links)?;
                println!("TASK_CREATED {id}");
            }
            TaskCommands::Get { id } => {
                let task_id = parse_task_ref(&id)?;
                let task = workflows::tasks::get(&task_id)?;
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
            TaskCommands::Update { id, status } => {
                let task_id = parse_task_ref(&id)?;
                let task_status: TaskStatus = status
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!("{e}"))
                    .context("invalid status")?;
                workflows::tasks::update_status(&task_id, task_status)?;
                println!("TASK_UPDATED {task_id}");
            }
            TaskCommands::Comment {
                id,
                author,
                content,
            } => {
                let task_id = parse_task_ref(&id)?;
                let comment_author: CommentAuthor = author
                    .parse()
                    .map_err(|e: String| anyhow::anyhow!("{e}"))
                    .context("invalid author")?;
                workflows::tasks::add_comment(&task_id, comment_author, &content)?;
                println!("TASK_UPDATED {task_id}");
            }
            TaskCommands::Ready { id } => {
                workflows::tasks::set_ready(&id)?;
                println!("TASK_UPDATED {id}");
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
