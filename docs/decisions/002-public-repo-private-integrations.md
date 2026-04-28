# 002 — Public repo with private workflows via hub-private

## Context

Hub is worth building in public: it demonstrates Rust, systems thinking,
agent integration, and personal tooling craft. However some workflows
are professionally sensitive — personal lifestyle tooling that could
reflect poorly in certain hiring or employment contexts. Abstracting
names wouldn't reliably obscure them; API shapes and data models are
recognisable to anyone familiar with the domain.

A secondary concern: work-specific workflows (internal APIs, tools
that reveal a company's stack) may be inappropriate to publish depending
on employer policies.

## Decision

Hub is a public repo. Sensitive workflows and investigation skills live in a
private companion repo (`hub-private`) and are symlinked into gitignored
directories in hub:

```
hub/clients/src/private/   →  symlink  →  hub-private/clients/src/
hub/workflows/src/private/  →  symlink  →  hub-private/workflows/src/
hub/.claude/skills/<name>.md  →  symlink  →  hub-private/.claude/skills/<name>.md
```

`hub-private` is a private GitHub repo with the same owner. It is cloned
alongside hub on each device and linked via a setup script. This keeps
sensitive workflows and skills version-controlled and recoverable without ever
appearing in hub's public history.

The public repo shows only workflows and skills appropriate to share: GitHub,
Linear, Loki, Datadog, and similar professional tooling. The private
equivalents are invisible to the public codebase but fully functional
locally.

## Consequences

- New machine setup requires cloning both repos and running
  `just setup-private` to create the symlinks.
- Development workflow: edit files via either path (symlinks make them
  the same inode), build and test from `hub/`, commit from `hub-private/`.
  One context switch per commit, not per edit.
- `hub.toml` and `.env` (both gitignored) can live in `hub-private` and
  be symlinked into the hub root — giving per-device config backup for
  free.
