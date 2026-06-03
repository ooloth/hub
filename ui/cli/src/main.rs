use anyhow::Result;
use clap::{Parser, Subcommand};
use domain::{TaskId, TaskKind};

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
            TaskCommands::Create { title, kind } => {
                let id = workflows::tasks::create(&title, kind)?;
                println!("TASK_CREATED {id}");
            }
            TaskCommands::Ready { id } => {
                workflows::tasks::set_ready(&id)?;
                println!("TASK_UPDATED {id}");
            }
        },
    }
    Ok(())
}
