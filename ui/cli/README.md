# ui/cli

The `hub` binary — the **agent's in-session toolkit**.

No subcommands are currently wired. The task subcommand (`hub task get/report/comment`)
was removed with the task model (ADR 019). Future agent-facing subcommands belong here
when the filesystem session model is built out.

Humans do not use this CLI directly. `hub-tui` is the human-facing surface.

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
