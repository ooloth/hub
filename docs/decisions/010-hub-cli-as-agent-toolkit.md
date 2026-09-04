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

## Addendum (2026-06-20) — post-019 scope

[Decision 019](019-drop-task-model-filesystem-sessions.md) removed the
`hub task *` session protocol; the CLI no longer brokers any session
lifecycle. With that use case gone, this sharpens what the toolkit is for.

**The dividing line between the CLI and skills.** Hub also exposes project
knowledge through Claude Code skills that read `hub.toml`. To avoid two
overlapping ways to teach an agent the same thing:

- A `hub` command earns its place when the operation needs **computation**,
  hub's **resolved config/secrets**, or **output-shaping** — things a markdown
  skill cannot do. The strongest case is context-window protection: a command
  returns exactly the fields an agent needs instead of the agent running a
  generic command and drowning in raw, unformatted output.
- **Skills** remain the home for instructions and workflows. They may call CLI
  accessors, but a thin wrapper around a generic command (no compute, no config,
  no shaping) belongs in a skill, not the binary.

**Guardrails:**

- **Stateless.** The toolkit reads context, shapes output, and performs discrete
  helper actions. It does not regain a lifecycle or a `report`-shaped state
  machine — 019 deleted that on purpose, and the CLI must not become a second
  source of truth.
- **Demand-driven.** Each command is nominated by an observed session failure —
  an agent guessing wrong about a generic command, or a context window flooded
  by unshaped output — the same iterative-grooming loop 019 prescribes for
  investigation prompts. Nothing is added speculatively.

This is non-blocking: post-019 the CLI may shrink to near-nothing and regrow
only as real sessions surface the need.

## Addendum (2026-09-04) — no unattended processes

A background surface for unattended PR detection (`hub-daemon`) was considered as an addition to
this CLI before being placed in its own surface instead. Recording explicitly, since nothing above
ruled it out directly: **hub-cli may not run as a long-lived or scheduled process.** It stays what
019 and the addendum above already established — stateless, invoked once per command, no lifecycle
of its own. Any capability that needs to run unattended, on a schedule, or outlive a single
invocation belongs in a separate surface, not this one.
