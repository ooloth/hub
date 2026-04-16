# hub

Personal work hub — overview dashboard and CLI for PRs, errors, and anything else worth tracking.

## What it does

- **Overview page** — cards showing title + count for each data source (PRs waiting for review, production errors, etc.)
- **Detail pages** — per-integration drill-down views
- **CLI** — `hub sync`, `hub status`, query commands; also usable by agents
- **Local SQLite** — each device has its own database; no cloud sync

## Prerequisites

- [Bun](https://bun.sh)

## Running locally

```bash
bun install
bun run db:migrate
bun run dev          # web app at http://localhost:3000
```

## Syncing data

```bash
hub sync             # sync all integrations
hub sync github-prs  # sync one integration
hub status           # print current counts in the terminal
```

## Adding an integration

1. Create `packages/integrations/<slug>/`
2. Implement the integration contract (see [CLAUDE.md](./CLAUDE.md) for the full spec)
3. Add your Drizzle table to the db migration
4. Run `hub sync <slug>`

The card and detail page appear automatically — no registration needed.

## Integrations

| Name | Source | Tracks |
|---|---|---|
| GitHub PRs | GitHub API | PRs awaiting my review |
| Loki Errors | Grafana Loki | Production errors |
