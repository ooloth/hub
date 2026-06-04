# ui/cli

The `hub` binary — the **agent's communication channel with hub**.

Agent sessions call `hub task` commands to read their assigned task, write
captain's log entries, and report status. This is the complete intended surface.
It does not mirror or shadow the TUI's capabilities — human-owned operations
(create, promote, approve, cancel, dispatch) intentionally do not exist here.

Humans do not use this CLI directly. `hub-tui` is the human-facing surface;
the dispatch workflow is TUI-managed. Any command that feels like "something
a human would do in the TUI" does not belong in this binary.

## Commands

```
hub task get TASK-XXXX
    Read the full task as JSON at session start. Returns title, description,
    kind, status, linked resources, and the agent comment thread.

hub task report TASK-XXXX --status <in-review|blocked>
    Report status back to hub. Use `in-review` when work is complete;
    use `blocked` when the agent cannot continue. These are the only two
    status transitions the agent owns — done, failed, and cancelled are
    human decisions made in the TUI.

hub task comment TASK-XXXX --content "..."
    Append a captain's log entry. Record choices made, friction encountered,
    trade-offs taken — anything the human might wonder about when reviewing
    the session. One-way: agent writes, human reads.
```

All output is single-line and machine-parseable. Errors go to stderr, exit 1,
with an actionable message.

## CLI (clap)

Use the derive API. Annotate structs; don't use the builder.

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Fetch,
    Status,
}

fn main() {
    let cli = Cli::parse();
}
```

Help text, `--version`, type coercion, and error messages are free.
