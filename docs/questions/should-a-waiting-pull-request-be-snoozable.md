---
opened: 2026-10-05
status: open
resolves_into: decision
---

# Should a waiting pull request be snoozable?

## Why it matters

The daemon notifies a pull request again at the first pass at least an hour after its last
notification, for as long as it stays my turn
([#344](https://github.com/ooloth/hub/issues/344)). That keeps a review request from being
forgotten. It also reminds me hourly about a pull request I have parked on purpose: one that is my
turn by hub's reading of the data, but that I have decided to leave until later. Reminders about
something I have already decided on get dismissed unread, and routine dismissal is the condition
under which [Decision 020](../decisions/020-hub-runs-an-unattended-surface.md) says hub should stop
interrupting at all.

## What would settle it

Living with hourly reminders, and finding a pull request that was reminded about repeatedly after I
had chosen to leave it, often enough that the reminders for it were dismissed without being read.
If parked pull requests turn out to be rare, or are better handled by fixing what hub counts as my
turn, the question closes with nothing built.

## Resolves into

[../decisions/](../decisions/), if snoozing is built: it needs a way to act on a notification and a
record of what is snoozed until when, which is new stored state.

## Source

Raised 2026-10-05 while settling how often #344 reminds about a pull request that stays my turn.

## Options

- **A. No snooze.** Hourly reminders stand as they are. Strongest case: nothing to build, and a
  reminder about a parked pull request may be the nudge it needs. Cost: reminders about deliberate
  delays, which train dismissal.
- **B. Snooze from the notification.** An action on the notification hides that pull request for a
  chosen time. Strongest case: the decision is made where the interruption happens. Cost: handling a
  response from the notification, and storing the snooze so a restart does not forget it.
- **C. Snooze from the TUI.** A key on the pull request's row hides it from notifications for a
  time. Strongest case: no notification interaction to build. Cost: snoozing needs hub open, and
  the snooze still has to be stored where the daemon reads it.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Sourced* (2026-10-05, `terminal-notifier -help`): terminal-notifier 3.1.0 can add buttons to a
  notification (`-action`) and prints the one chosen, waiting until a response or `-timeout`
  expires. Option B is reachable with the delivery tool #344 already uses, at the cost of a call
  that waits.
