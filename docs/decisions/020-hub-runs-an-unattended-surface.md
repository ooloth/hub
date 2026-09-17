---
number: 020
status: accepted
date: 2026-09-16
---

# 020 — Hub runs an unattended surface

## Forced by

[Decision 009](009-no-scheduled-runs.md) removed every unattended path from hub, for two reasons it
states directly: `claude -p` usage stops being covered by the subscription, and Claude Desktop
Routines cover the lightweight scheduled work that "doesn't require local system access."

Neither reason reaches the surface this record introduces. It makes no Claude calls at all, so
there is no per-call cost for a human to approve. It reads `hub.toml`, resolves credentials through
the 1Password CLI, and reads the local SQLite cache, which is the local system access 009 names as
the line a Routine cannot cross.

What it is for comes from [#327](https://github.com/ooloth/hub/issues/327): a PR queue turn lands
while hub is closed, and nothing reports it until hub is next opened. Nothing about the turn
arriving causes hub to be opened, so the delay is unbounded by anything except how often hub gets
opened anyway.

Delivery from an unattended process is measured, not assumed. Spike, 2026-09-04, recorded in #327:
an `osascript` notification fired from a launchd agent running with `PPID=1`, parent
`/sbin/launchd`, no TTY, exit 0, with a delivery record present in Notification Center.

## Decision

Hub runs a long-lived local process that polls with no human present and notifies.

It makes no Claude calls. Detection is the only thing that becomes unattended: pressing `i` on a
signal is still what starts an investigation, so what 009 protected is intact.

It lives in its own binary, `hub-daemon`. That placement is not open:
[Decision 010](010-hub-cli-as-agent-toolkit.md)'s 2026-09-04 addendum states that hub-cli may not
run as a long-lived or scheduled process, and that anything needing to belongs in a separate
surface.

## Rejected

- **A Claude Desktop Routine, which 009 nominated for exactly this kind of job** — because a
  Routine does not run on this machine, which is the limit 009 itself records when it keeps
  `implement-issue` local. So a Routine could read neither `hub.toml`, nor credentials through the
  1Password CLI, nor the cache, and would have to re-implement fetching and cross-domain ranking
  outside hub. The capability claim is inherited from 009 and has not been re-checked against
  current Routines. Reverses if Routines gain execution on the local machine.
- **Stay pull-only, and rely on opening the TUI** — because a surface that only runs when opened
  cannot report an event that happens while it is closed, which is the entire event class in
  question. Reverses if the turns worth knowing about start arriving while hub is already open.

## Risk

An unattended process is one that can be silently lost. Measured in the same 2026-09-04 spike:
installing a LaunchAgent creates a Login Items & Extensions entry, and macOS can disable the agent
from there with hub never learning it happened. A silently disabled daemon looks exactly like a
quiet week.

That is knowingly accepted, and it makes one thing a requirement rather than a nicety: daemon
health belongs in the TUI status bar permanently, not behind a diagnostic command. Even then the
window does not close, since someone who has stopped opening the TUI never sees the status bar
either.

## Revisit when

Notifications are routinely dismissed without action. That is the observable sign that the
interruption costs more than the missed turn, and it reopens whether hub should interrupt at all.

## Also update

- [x] questions/README.md — no open question closes into this record.
      `should-notifications-also-go-to-slack.md` presumes an unattended surface exists, so this
      re-scopes it rather than settling it.
- [x] vision.md — "What hub is not" no longer promises that hub does not interrupt, and the
      daemon joins the surfaces section.
