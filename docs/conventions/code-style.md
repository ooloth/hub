# Code Style

## Easy Mode Rust

Prioritize clarity and forward momentum over optimization. Fight the borrow checker as
little as possible — pick the highest-level tool available, default to the simplest
memory model, and save cleverness for when profiling demands it.

### Error handling

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

Skip `thiserror` unless a library crate needs callers to match on specific error
variants — which won't happen in this project.

### Owned types

Default to owned types. Avoid lifetime annotations entirely.

```rust
// Structs: always owned
struct Item {
    title: String,
    tags: Vec<String>,
}

// Functions: owned params unless read-only
fn process(item: Item) { ... }     // takes ownership
fn display(title: &str) { ... }    // read-only: &str is fine
```

If you're writing `'a`, stop. Return owned values instead of references to local data.
The compiler is telling you the design needs to change, not that you need more
annotations.

### Cloning

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

Clone costs almost never show up in profiling for a tool like this. Optimize when
measurements say to.

### Async

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

## Tiger Style

For the rest, see [Tiger Style](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md).
