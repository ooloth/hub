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
exclude_labels = ["wontfix"]
```

## 3. Add environments (if the project is deployed)

If the project runs in one or more environments, add `[[project.environment]]`
blocks. Each environment carries the platform context its workflows need:

```toml
[[project.environment]]
env = "prod"
gcp_project = "my-org-prod"
gcp_region = "us-central1"
service = "my-app"

[[project.environment.workflow]]
name = "user-activity-gcp"
exclude_users = ["bot@my-org.com"]
```

Repeat for each environment (dev, uat, prod, etc.).

## 4. Ensure required credentials are in .env

Each workflow documents which env var it reads. Check `.env.example` for the
full list. If a variable is missing, the workflow produces no items — it
doesn't error.

## Notes

- A project entry is device-specific. Add it only to the devices where it's
  relevant — work projects on the work laptop, personal projects on the
  personal laptop.
- Taplo will validate your config against the schema in
  `config/schemas/hub.toml.schema.json` and surface unknown fields or
  missing required keys inline in your editor.
