# Secrets

Secrets live in 1Password and are injected as env vars at runtime via `op run`. Rust
code only ever calls `std::env::var` — it never reads files or knows about 1Password.

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

`config::Config::load()` is called once in `main()` and the struct is passed down as
an argument. Nothing else calls `std::env::var`.
