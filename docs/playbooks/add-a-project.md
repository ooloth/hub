# Add a Project

Steps to add a new codebase to hub so its workflows appear on this device.
This is a config-only change — no Rust code required unless you also need a
new workflow (see `add-a-workflow.md`).

## 1. Add the project entry to your device config

Edit your local `hub.toml` (or `hub-private/devices/<device>.toml` if using
hub-private) and add a `[[project]]` block:

```toml
[[project]]
name = "my-app"
repo = "org/my-app"
```

`name` is the human-readable label shown in the UI. `repo` is the GitHub
repository in `owner/name` format — workflows that talk to GitHub read it
from here.

## 2. Add codebase-level workflows

For observations that don't depend on a deployment environment (PRs, issues,
CI), add `[[project.workflow]]` entries immediately after the project block:

```toml
[[project.workflow]]
name = "github-prs"

[[project.workflow]]
name = "github-issues"
```

## 3. Add environments (if the project is deployed)

If the project runs in one or more environments, add `[[project.environment]]`
blocks. Each environment carries the platform context its workflows need:

```toml
[[project.environment]]
env = "prod"
gcp_project = "my-org-prod"
gcp_region = "us-central1"

[[project.environment.workflow]]
name = "gcp-logs"
title = "app errors"
query = 'severity>=ERROR'
lookback = "1h"
```

Repeat for each environment (dev, uat, prod, etc.).

## 4. Ensure required credentials are in hub.toml

Each workflow documents which credential key it reads. Check the `[credentials]`
table in your `hub.toml` (or `hub-private/devices/<device>.toml`).
`github_token` and `github_username` are required: if either is missing or empty,
`Config::load` fails. `linear_token` and `loki_token` are optional. Without
`linear_token` there are no Linear items, and without `loki_token` Loki queries are
sent without an auth token.

## Notes

- A project entry is device-specific. Add it only to the devices where it's
  relevant — work projects on the work laptop, personal projects on the
  personal laptop.
- Taplo will validate your config against the schema in
  `config/schemas/hub.toml.schema.json` and surface unknown fields or
  missing required keys inline in your editor.

## Done when

`just check` passes and `just tui` shows the project's
workflow items alongside existing projects.
