# 007 — TUI over web app

## Context

Hub needs a persistent UI beyond the one-shot CLI. The natural candidates are:

- **TUI** — a Ratatui terminal dashboard, auto-refreshing, keyboard-navigable
- **Web app** — a browser UI served by a local HTTP server (Next.js, Tauri, etc.)
- **Desktop app** — a native GUI via Tauri or similar

The web app option is the easiest to reach for: it offers arbitrary layout,
rich components, and an obvious home for an embedded chat or agent interface.
It also conflicts with hub's foundational constraints (local-only, no server,
no shared state) and risks pulling development time into UI infrastructure
instead of signal quality.

## Decision

TUI (Ratatui). Not a web app.

The core loop hub is built around — observe ranked signals, select one, launch
an investigation — maps directly onto a TUI. A ranked list with keyboard
navigation is the entire UI needed. Nothing about this loop requires a browser.

The agent interaction layer is Claude Code, not hub. Hub's job is to surface
signals and hand off to the right investigation skill with context pre-loaded.
A keybinding that opens `claude /github-ci-investigate` in a new tmux pane,
with the repo and run URL pre-loaded from the selected item, is the pattern —
already implemented for CI failures. The same pattern extends to any signal
type (logs, alerts, disk usage). Hub is the launcher; Claude Code is the
investigator. Hub does not need to build a chat interface.

The TUI constraints are features, not limitations:

- No server means no deployment, no auth surface, no sync complexity
- No build step means the binary stays self-contained
- Staying in the terminal means zero context-switch for a developer already
  working there
- Keyboard-first navigation is faster than pointer interaction for triage

## Consequences

- Ratatui is the UI library. No browser dependencies, no HTTP server, no
  JavaScript build toolchain.
- The keybinding that launches a Claude Code skill from a selected item is
  load-bearing. It must pre-load context from the selected item and hub.toml
  so the investigation starts immediately with zero manual setup. Design each
  new handoff explicitly — it is the seam between hub and the agent layer.
  See `ui/tui/src/investigations/` for the existing CI pattern.
- Features that seem to call for browser UI are a signal to question whether
  the feature is core, not a signal to add a web app. "This would be easier
  in HTML/CSS" is not a sufficient reason to abandon the no-server constraint.
- The one scenario that would legitimately revisit this decision: hub evolves
  into an agent operations center where multiple parallel agent runs are in
  flight and the user is reviewing diffs, approving changes, and coordinating
  work side by side. That is a qualitatively different product. If hub gets
  there, the right answer will be obvious — and by then Claude Code's own
  interface will likely handle much of it anyway.
