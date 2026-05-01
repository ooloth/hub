# Add a Private Workflow

## Should you add this?

Use hub-private when the workflow involves infrastructure, credentials, or integrations
that shouldn't be in the public repo — for example, home server media integrations
(Sonarr, Radarr, Prowlarr) where the existence of the service implies something you'd
rather not publish.

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

## 3. Add credentials to .env

Add the required `op://` secret references to `hub-private/.env`.

## 4. No schema change needed

The public schema (`config/schemas/hub.toml.schema.json`) contains a
`workflow_private` catch-all that accepts any workflow name not in the known
public list. Private workflow entries pass taplo validation automatically —
do not add private workflow definitions to the public schema.

## 5. Enable on your device

Add a `[[project.workflow]]` or `[[project.environment.workflow]]` entry to the
relevant `hub-private/devices/<device>.toml` files.

## 6. Verify

```bash
just check
```
