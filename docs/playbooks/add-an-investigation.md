# Add an Investigation

Investigations open a Claude Code session in a tmux split to diagnose a
specific failure. Each investigation is a Rust module in
`ui/tui/src/investigations/` that builds a context-specific prompt and
delegates to the shared tmux launcher in `mod.rs`.

## 1. Add an Effect variant

In `ui/tui/src/state/types.rs`, add a variant to the `Effect` enum:

```rust
LaunchGrafana { log_url: String },
```

## 2. Add an InvestigateAction variant

In `ui/tui/src/state/types.rs`, add a variant to `InvestigateAction`:

```rust
Grafana { log_url: String },
```

Then return it from `compute_investigate_action()` in
`ui/tui/src/state/update.rs` for the items that warrant this investigation.

## 3. Create the prompt file

Add `.agents/prompts/grafana-investigate.md` with the system prompt for the
Claude session. This is the skill content Claude receives.

## 4. Create the investigation module

Create `ui/tui/src/investigations/grafana.rs`:

```rust
use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../.agents/prompts/grafana-investigate.md");

pub(crate) fn config(log_url: &str) -> LaunchConfig {
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: format!("Investigate the Grafana log at {log_url}"),
        model: "opus".to_string(),
        allowed_tools: "Bash".to_string(),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn grafana_investigation_system_prompt_is_loaded() {
        let cfg = config("https://grafana.example.com/d/abc");
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---")); // no frontmatter leaking
    }

    #[test]
    fn grafana_investigation_prompt_contains_url() {
        let cfg = config("https://grafana.example.com/d/abc");
        assert!(cfg.prompt.contains("https://grafana.example.com/d/abc"));
    }
}
```

## 5. Register the module

In `ui/tui/src/investigations/mod.rs`:

```rust
pub(crate) mod grafana;
```

**Device-specific investigations only:** if this module is symlinked per-device
from hub-private, the file must still exist on every machine where `--features private`
is active — otherwise the compiler and `cargo fmt` will error. Add a `stub()` call to
`scripts/setup-private.sh` for non-target devices so the file is always present. See
[Private Workflows](../architecture/private-workflows.md) for the full pattern.

## 6. Handle the Effect in main.rs

In the `for effect in effects` loop in `ui/tui/src/main.rs`:

```rust
Effect::LaunchGrafana { log_url } => match std::env::current_dir() {
    Ok(cwd) => {
        if let Err(err) = investigations::launch(
            investigations::grafana::config(&log_url),
            &cwd,
        ) {
            app.ui.flash = Some(err.to_string());
        }
    }
    Err(e) => {
        app.ui.flash = Some(format!("Cannot determine working directory: {e}"));
    }
},
```

## Verify

```bash
just check
just test
```

Then confirm the investigation launches correctly in tmux (Tier 2 E2E — see
`ui/tui/README.md`): navigate to an item that triggers the new investigation,
press `i`, and verify the Claude session opens in a tmux split with the right
prompt.
