# Add a Private Workflow

Steps to add a workflow that lives in `hub-private` rather than the public repo.
The structure mirrors the public workflow pattern — the only difference is where
the files live.

## 1. Add the client

Create `hub-private/clients/src/<service>.rs` (or `<service>/mod.rs` for larger
clients) and add `pub mod <service>;` to `hub-private/clients/src/mod.rs`.

## 2. Add the workflow

Create `hub-private/workflows/src/<workflow-name>.rs` and add
`pub mod <workflow-name>;` to `hub-private/workflows/src/mod.rs`.

## 3. Add credentials to .env

Add the required `op://` secret references to `hub-private/.env`.

## 4. Register in the config schema

Private workflows are still validated by the public schema. Add a
`"workflow_<name>"` definition to `config/schemas/hub.toml.schema.json` and
reference it in the `"workflow"` oneOf. See [Add a Workflow](add-a-workflow.md)
step 5 for the exact shape.

## 5. Enable on your device

Add a `[[project.workflow]]` or `[[project.environment.workflow]]` entry to the
relevant `hub-private/devices/<device>.toml` files.

## 6. Verify

```bash
just check
```
