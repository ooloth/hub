# Hub

A personal command center that surfaces what needs my attention today — including (especially!)
the things I didn't know were broken. Everything I'm responsible for appears in a single terminal
window, ranked by urgency so the most important signal is always at the top.

## What This Is

Other tools already surface individual signals — Sentry monitors production errors, GitHub
shows PRs, Loki aggregates logs. Hub's value is in how it combines those insights in one
prioritized view with agentic capabilities built in:

- **Urgency-ranked** — items sorted by `(urgency, age)`; the top item is always the most pressing
- **Cross-domain triage** — signals from GitHub, Linear, Loki, home servers, and any other
  source appear in one ranked list; hub is the only place their urgency is compared
- **Pre-loaded investigation** — a keypress on any signal opens the right Claude Code skill
  with `hub.toml` context already loaded (endpoint, query, project name); investigation
  starts immediately, not after setup
- **Keypress execution** — for issues labelled `status:ready-for-agent`, a keypress in the
  TUI launches an agent in a git worktree; hub handles setup so the focus is on reviewing
  the result, not orchestrating the run
- **Trust-building flywheel** — every agent run is visible, reviewable, and mined to
  improve the next one's context and instructions; the goal is agents you can delegate to
  at scale, not steer one at a time; see [Vision](docs/vision.md)
- **Single config source of truth** — `hub.toml` is git-ignored and per-device; onboarding
  a new project to existing workflows is one file edit, no code changes; work and personal
  contexts stay naturally separate
- **Launch pad, not chat** — hub surfaces signals and launches Claude Code skills or
  autonomous agents preloaded with the right context; it leverages Claude Code rather than
  reinventing its interface; see [Decision 007](docs/decisions/007-tui-over-web-app.md)
- **Extensible** — adding a new workflow follows a short, documented playbook spanning
  `clients/`, `workflows/`, and `config/`; see [add a workflow](docs/playbooks/add-a-workflow.md)
- **Local-only** — no server, no cloud sync; each profile has its own SQLite database
  (`~/.hub/<profile>/hub.db`, [Decision 024](docs/decisions/024-hub-state-is-per-profile.md)) and
  runs independently
- **Rust** — three binaries: `hub` (CLI), `hub-tui` (Ratatui dashboard) and `hub-daemon`
  (refreshes the cache with nobody present); not a web app

## Docs

- [Vision](docs/vision.md) — what this is, why, and where it's going
- [Decisions](docs/decisions/) — architectural decisions and their rationale
- [Daemon](daemon/README.md) — `hub-daemon`, the unattended refresh
- [Conventions](CLAUDE.md#rust-conventions) — Rust patterns used throughout
- [Playbooks](docs/playbooks/) — step-by-step guides for common tasks
- [Contributing](CONTRIBUTING.md) — setup and development instructions
- [Private Workflows](docs/architecture/private-workflows.md) — `hub-private` wiring, symlinks, and Cargo features
