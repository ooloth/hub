# hub

A personal work hub: overview dashboard, per-integration detail pages, CLI, and agent interface — all backed by a local SQLite database.

## What This Is

- **Web app** — overview page (cards with title + count per integration), detail pages per integration
- **CLI** — `hub sync`, `hub status`, `hub <integration> list` etc.; also the interface for agents
- **Local-only** — runs on localhost, each device has its own SQLite db with its own data
- **Growable** — adding a new integration = adding a new directory; no registration step

## Stack

| Concern | Choice | Why |
|---|---|---|
| Runtime / package manager | Bun | Native TS execution, fast installs, built-in test runner |
| Monorepo | Bun workspaces | Lightweight, no separate orchestration tool needed initially |
| Web framework | Next.js (App Router) | File-based routing maps naturally to per-integration pages |
| Database | SQLite via Drizzle ORM | Local-first, per-device, type-safe queries and migrations |
| CLI framework | Citty | TypeScript-native, minimal API, clean subcommand support |

## Monorepo Structure

```
packages/
  db/                  # @hub/db — schema, migrations, typed query helpers
  integrations/        # @hub/integrations — shared integration types + auto-discovery

apps/
  web/                 # Next.js app (imports @hub/db directly)
  cli/                 # Citty CLI (imports @hub/db directly)
```

Both `web` and `cli` are consumers of `@hub/db`. Neither calls the other. The db package is the canonical data layer.

## Integration Contract

Each integration lives at `packages/integrations/<name>/` and must export:

```ts
// config.ts
export const config: IntegrationConfig = {
  name: string        // display name
  slug: string        // used in routing and CLI commands
  icon: string        // lucide icon name or emoji
  description: string
}

// schema.ts
// Drizzle table definitions for this integration's data

// fetch.ts
export async function fetch(db: Database): Promise<void>
// Pulls from the external source and upserts into SQLite

// card.tsx
export default function Card(): React.ReactNode
// Overview card: shows title + count; used on the hub home page

// page.tsx
export default function Page(): React.ReactNode
// Detail view: full list/breakdown for this integration
```

Auto-discovery scans `packages/integrations/*/config.ts` at build time — no manual registration.

## Adding a New Integration

1. Create `packages/integrations/<slug>/`
2. Add `config.ts`, `schema.ts`, `fetch.ts`, `card.tsx`, `page.tsx` following the contract above
3. Add the integration's Drizzle table to the db migration
4. Run `hub sync <slug>` to populate data
5. The card appears on the overview page and the detail page is routed automatically

## Development

```bash
bun install          # install all workspace dependencies
bun run dev          # start Next.js dev server
bun run db:migrate   # run pending migrations
hub sync             # sync all integrations
hub sync <slug>      # sync one integration
hub status           # print counts for all integrations
```

## Integrations

| Slug | Source | What it tracks |
|---|---|---|
| `github-prs` | GitHub API | PRs waiting for my review |
| `loki-errors` | Grafana Loki | Production errors/exceptions |

Add rows here as new integrations land.
