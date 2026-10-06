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
your workflow. On success, push the resulting `StatusItem` variants into the `items` of the
`PrivateStatusResult` that `run()` returns. On failure, push
`SourceError::new("<source name>", error)` into its `failures`, keeping the error rather than
dropping it. The refresh names the source in `StatusReport::errors` and reports why it failed
in `Refresh::failures`, redacted of every credential.

## 4. Add variants to the public StatusItem enum

In the public hub repo, open `workflows/src/status.rs` and add one or more
`#[cfg(feature = "private")]` variants to `StatusItem` for the new workflow:

```rust
#[cfg(feature = "private")]
MyNewItem(crate::private::status::MyNewItem),
```

Also add match arms for `urgency()` and `age()` on the new variants. This is
required for the new items to be sorted into the unified ranked list.

## 5. Add TUI display

Add the new variants to the matches on `StatusItem` in `ui/tui/src/display/` (see step 4 of
[Add a Workflow](add-a-workflow.md)). Put any display logic specific to the private items in
`hub-private/ui/tui/src/`. The TUI is the only surface that renders items.

## 6. Add credentials to hub.toml

Add the required credential keys to the `[credentials]` table in each
device's `hub-private/devices/<device>.toml`. Values can be plain strings
or `op://` 1Password references.

## 7. Enable on your device

Add a `[[monitor.workflow]]` entry to the relevant `hub-private/devices/<device>.toml`
files, using the workflow name your `status.rs` checks for:

```toml
[[monitor.workflow]]
name = "your-workflow-name"
```

`[[monitor.workflow]]` is for integrations (like media servers) that aren't tied to
a specific code project. Its `name` is a free string, so a new name needs no change to
hub's config. Use `[[project.workflow]]` inside a `[[project]]` block for integrations
that are scoped to a repo. Those names are a closed set, the `WorkflowConfig` variants in
`config/src/toml.rs`, and an unknown one fails `Config::load`.

## 8. Verify

```bash
just check
just test
just tui
```
