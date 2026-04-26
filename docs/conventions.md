# Rust Conventions

"Easy mode" Rust — prioritize clarity and forward momentum over optimization. Fight the borrow checker as little as possible.

## Error handling

Use `anyhow` for all application code. One type, zero boilerplate.

```rust
use anyhow::{Context, Result};

fn github_token() -> Result<String> {
    std::env::var("GITHUB_TOKEN").context("GITHUB_TOKEN not set")
}

fn main() -> Result<()> {
    let token = github_token()?;
    Ok(())
}
```

- `?` propagates any error upward
- `.context("msg")` adds a human-readable layer to the error chain
- `anyhow::bail!("msg")` returns an error from a string
- `main() -> Result<()>` prints the full error chain and exits non-zero automatically

Skip `thiserror` unless a library crate needs callers to match on specific error variants — which won't happen in this project.

## Owned types

Default to owned types. Avoid lifetime annotations entirely.

```rust
// Structs: always owned
struct Item {
    title: String,
    tags: Vec<String>,
}

// Functions: owned params unless read-only
fn process(item: Item) { ... }          // takes ownership
fn display(title: &str) { ... }         // read-only: &str is fine
```

If you're writing `'a`, stop. Return owned values instead of references to local data. The compiler is telling you the design needs to change, not that you need more annotations.

## Cloning

Clone freely. Don't fight the borrow checker.

```rust
// Fine
let title = item.title.clone();
process(title);
render(&item.title);

// Across .await points — clone before the await
let name = user.name.clone();
do_async_thing().await;
println!("{name}");
```

Clone costs almost never show up in profiling for a tool like this. Optimize when measurements say to.

## Async

Use tokio. `#[tokio::main]` on main, `features = ["full"]` in Cargo.toml.

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (prs, errors) = tokio::join!(fetch_prs(), fetch_errors());
    Ok(())
}
```

- Use `tokio::join!` for concurrent work
- Use `tokio::fs` and `tokio::time::sleep` inside async (not std equivalents)
- Clone or take ownership before `.await` — don't hold references across await points

Use sync when there's no concurrency benefit. Not everything needs to be async.

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

## Secrets

Secrets live in 1Password and are injected as env vars at runtime via `op run`. The Rust code only ever calls `std::env::var` — it never reads files or knows about 1Password.

```bash
# .env contains op:// references, safe to commit
GITHUB_TOKEN=op://Personal/GitHub/token

# inject at runtime
op run --env-file=.env -- just status
```

```rust
// config/src/lib.rs — the only place env var names are written
use anyhow::{Context, Result};

pub struct Config {
    pub github_token: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        Ok(Self {
            github_token: std::env::var("GITHUB_TOKEN")
                .context("GITHUB_TOKEN not set")?,
        })
    }
}
```

`config::Config::load()` is called once in `main()` and the struct is passed down as an argument. Nothing else calls `std::env::var`.

## HTTP (reqwest)

```toml
reqwest = { version = "0.12", features = ["json"] }
```

```rust
#[derive(serde::Deserialize)]
struct PullRequest { title: String, number: u64 }

let prs: Vec<PullRequest> = reqwest::Client::new()
    .get("https://api.github.com/...")
    .bearer_auth(&token)
    .send().await?
    .json().await?;
```

## SQLite (rusqlite)

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

`bundled` compiles SQLite in — no system dependency.

```rust
let conn = Connection::open(&db_path)?;
conn.execute("INSERT INTO items (title) VALUES (?1)", [&title])?;
let count: i64 = conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;
```

Upgrade to `sqlx` if async DB access becomes necessary.
