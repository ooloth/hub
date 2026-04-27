# clients

External API wrappers. The only code that makes network calls.

**Rules:**
- One file or subdirectory per external service
- Adapts external API responses into domain types
- Never imports from store or workflows

**Lives here:** HTTP clients, auth handling, rate limit logic, response mapping.

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
