# hub

Personal command center. Surfaces what needs attention today across software I'm responsible for.

- Local-only — each device has its own SQLite database
- Context-aware — work laptop tracks work software; personal laptop tracks personal software
- Rust — single binary, CLI today, TUI planned

The goal: a single terminal window showing everything I'm responsible for across my software, ranked by what needs attention. When something warrants a closer look, I can launch an agent investigation from there — it queries the logs, reads the code, and tells me what's happening. I act on it and move on.

## Docs

- [Vision](docs/vision.md) — what this is, why, and where it's going
- [Decisions](docs/decisions/) — architectural decisions and their rationale
- [Conventions](docs/conventions/) — Rust patterns used throughout
- [Playbooks](docs/playbooks/) — step-by-step guides for common tasks
- [Contributing](CONTRIBUTING.md) — setup and development instructions
- [Private Workflows](docs/architecture/private-workflows.md) — hub-private wiring, symlinks, and Cargo features
