# Hub

My personal command center: a single terminal window where I see everything I'm responsible
for, ranked by what needs attention most.

Other tools already surface individual signals — Grafana monitors production errors, GitHub
shows PRs, Loki aggregates logs. Hub's value is the triage and agency layer on top of all
of them:

- **Cross-domain triage** — a Loki production error, a failing CI run, and a home server
  import failure appear in the same urgency-ranked list; no other tool compares their urgency
- **Pre-loaded investigation** — a keypress on any signal opens the right Claude Code skill
  with `hub.toml` context already loaded (endpoint, query, project name); investigation
  starts immediately, not after five minutes of setup
- **Automated proposals** — for well-understood problem categories, hub drafts the work (a
  structured GitHub issue, and where the solution is clear, a draft PR) for my review; the
  goal is waking up to proposed solutions, not just notifications
- **Single config source of truth** — `hub.toml` defines what matters on each device; one
  file instead of Grafana dashboards, PagerDuty rules, GitHub notification settings, and
  browser bookmarks maintained separately

This is a personal tool. It runs locally on each of my devices, has no server, and I'm not
offering support for other installations. I'm sharing it as a reference for how I think
about personal tooling and agentic workflows.

## Docs

- [Vision](docs/vision.md) — what this is, why, and where it's going
- [Decisions](docs/decisions/) — architectural decisions and their rationale
- [Conventions](docs/conventions/) — Rust patterns used throughout
- [Playbooks](docs/playbooks/) — step-by-step guides for common tasks
- [Contributing](CONTRIBUTING.md) — setup and development instructions
- [Private Workflows](docs/architecture/private-workflows.md) — `hub-private` wiring, symlinks, and Cargo features
