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

### Immutability

Default to `let`. Only reach for `let mut` when mutation is actually needed. Prefer
returning new values over mutating in place — it keeps functions easier to test and
reason about.

## Type-first modeling

Use types to make invalid states unrepresentable. When the compiler rejects the bug,
you don't have to write a test for it.

**Enums over booleans or stringly-typed states:**

```rust
// Two booleans can produce a combination you never intend
is_open: bool, is_merged: bool   // (true, true)?

// One enum can't
enum PrState { Open, Merged, Closed }
```

**`Option<T>` over sentinel values:**

```rust
// Empty string is ambiguous: missing, or intentionally blank?
description: String

// Absence is explicit
description: Option<String>
```

**Newtypes for identifiers with structural invariants:**

```rust
// Nothing prevents assigning a bare URL to this field
repo: String   // must be "owner/repo" format — implicit, unenforced

// Newtype enforces the invariant at construction
struct RepoSlug(String);
impl RepoSlug {
    fn parse(url: &str) -> Result<Self> { ... }
}
```

Reach for newtypes when a plain `String` is secretly a domain concept with a format
or set of valid values — and when code in more than one place needs to produce or
consume it.

## Tiger Style

For the rest, see [Tiger Style](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md).
