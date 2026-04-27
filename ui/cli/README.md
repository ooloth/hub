# ui/cli

The `hub` binary. Bootstraps config, wires dependencies, and calls workflows.

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
    Sync { slug: Option<String> },
    Status,
}

fn main() {
    let cli = Cli::parse();
}
```

Help text, `--version`, type coercion, and error messages are free.
