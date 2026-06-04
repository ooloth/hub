# ui/cli

The `hub` binary — the **agent-facing CLI toolkit**. Agents call `hub tasks`
commands to read their assigned task, signal progress, and update status. The
system calls `hub tasks dispatch` on a polling loop to claim ready tasks and
spawn agent sessions.

Humans do not use this CLI directly — `hub-tui` is the human-facing surface.

Bootstraps config, wires dependencies, and calls workflows.

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
