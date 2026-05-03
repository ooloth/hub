# Add a Private Workflow

## Should you add this?

Use hub-private when the workflow involves infrastructure, credentials, or integrations that
shouldn't be in the public repo — for example, references to private work integrations.

If the workflow has no sensitive implications, add it to the public repo using
[Add a Workflow](add-a-workflow.md) instead.

## How to add it

The structure mirrors the public workflow pattern — the only difference is where the
files live.

## 1. Add the client

Create `hub-private/clients/src/<service>.rs` (or `<service>/mod.rs` for larger
clients) and add `pub mod <service>;` to `hub-private/clients/src/mod.rs`.

## 2. Add the workflow

Create `hub-private/workflows/src/<workflow-name>.rs` and add
`pub mod <workflow-name>;` to `hub-private/workflows/src/mod.rs`.

## 3. Wire into the status orchestrator

`hub-private/workflows/src/status.rs` is the entry point that hub calls. Add a
branch to `run()` that checks for your workflow name in `workflow_names` and calls
your workflow, populating the relevant field on `PrivateStatusData` (add the field
if it doesn't exist yet).

## 4. Add CLI rendering

Add (or extend) `hub-private/ui/cli/src/status.rs` to render the new field. The
public hub binary calls `crate::private::status::render(&report.private)` — your
renderer reads from `PrivateStatusData` and prints lines to stdout.

Add a corresponding renderer in `hub-private/ui/tui/src/` for any TUI-specific display logic.

## 5. Add credentials to .env

Add the required `op://` secret references to `hub-private/.env`.

## 6. Enable on your device

Add a `[[monitor.workflow]]` entry to the relevant `hub-private/devices/<device>.toml`
files, using the workflow name your `status.rs` checks for:

```toml
[[monitor.workflow]]
name = "your-workflow-name"
```

`[[monitor.workflow]]` is for integrations (like media servers) that aren't tied to
a specific code project. Use `[[project.workflow]]` inside a `[[project]]` block for
integrations that are scoped to a repo.

## 7. Verify

```bash
just check
just test
just status
```
