# 010 — `hub` CLI repurposed as agent toolkit

## Context

The `hub` CLI today prints status data to stdout for human consumption.
This duplicates what the TUI shows. The TUI is the primary human-facing
surface (per [decision 007](007-tui-over-web-app.md)) and the place
hub status data is meant to be read.

At the same time, agents have been calling `git`, `gh`, and other tools
directly from skill prompts. Each prompt re-encodes project-specific
knowledge — labels, reviewers, branch naming, check commands — and the
same knowledge drifts between prompts.

Hub already has a central place for project knowledge in `hub.toml`.
What's missing is the action surface that consumes it.

## Decision

The `hub` CLI binary stays. Its purpose changes:

- Status display is removed.
- The new purpose is to serve as an agent-facing toolkit that
  encapsulates project-conditional logic so skill prompts don't have
  to.

The specific surface — which commands exist, how they're named and
grouped, what conventions they follow — is deferred. It will be designed
as agent usage patterns surface real needs.

## Consequences

- `hub status` and its formatters are removed.
- [Decision 008](008-tui-owns-refresh-loop.md)'s mention of `hub status`
  is superseded by this decision. The TUI remains the sole human-facing
  surface for hub status data.
- The CLI gains the agent-toolkit role described above; concrete
  commands are added as needed, not speculatively.
- Skill prompts simplify once the toolkit exists. Specifics deferred.
