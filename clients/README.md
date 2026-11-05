# clients

External API wrappers. The only code that calls external services' APIs, over HTTP with `reqwest`
or through the `gcloud` CLI. Other crates reach the network only through programs they run
(`workflows` runs `git fetch`, `config` runs `op read` and `ui/tui` runs `gh pr diff`) or by calling
`clients`.

**Rules:**
- One file or subdirectory per external service
- Adapts external API responses into domain types
- Never imports from store or workflows

**Lives here:** HTTP clients, auth handling, response mapping.

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
    .header("User-Agent", "hub-cli")
    .send().await?
    .error_for_status()?
    .json().await?;
```

GitHub refuses a request with no `User-Agent`. A request without its own timeout can only fail when
the whole source reaches its one-minute limit (`SOURCE_TIMEOUT` in `workflows/src/status.rs`).
