---
opened: 2026-10-05
status: open
resolves_into: decision
---

# Should a waiting pull request be snoozable?

## Why it matters

The daemon notifies a pull request once, when it becomes my turn, and does not notify it again
while it stays my turn and the daemon keeps running
([#344](https://github.com/ooloth/hub/issues/344)). Nothing reminds me about a pull request whose
notification I dismissed. So today nothing needs snoozing, and the cost falls the other way: a
review request I dismiss and then forget stays forgotten until I open hub or the daemon restarts.

If that happens, the fix is a reminder: notifying a pull request again after it has stayed my turn
for a while. A reminder brings the snooze question back. It reminds me about a pull request I have
parked on purpose as well as one I forgot, and reminders about something I have already decided on
get dismissed unread. Routine dismissal is the condition under which
[Decision 020](../decisions/020-hub-runs-an-unattended-surface.md) says hub should stop
interrupting at all.

## What would settle it

Two observations, in order. First, a pull request whose notification I dismissed and then forgot,
which is what would justify reminders at all. Without it, the question stays open with nothing
built. Second, once reminders exist, a pull request reminded about repeatedly after I had chosen to
leave it, often enough that its reminders were dismissed without being read. If parked pull
requests turn out to be rare, or are better handled by fixing what hub counts as my turn, the
question closes with nothing built.

## Resolves into

[../decisions/](../decisions/), if snoozing is built: it needs a way to act on a notification and a
record of what is snoozed until when, which is new stored state.

## Source

Raised 2026-10-05 while settling how often #344 reminds about a pull request that stays my turn.
Revised 2026-10-17, when #344 dropped its hourly reminder: a pull request is notified once, and a
reminder waits for a forgotten pull request to be seen (amendment on the
[#327](https://github.com/ooloth/hub/issues/327) plan of record).

## Options

- **A. No reminder and no snooze.** What #344 builds. Strongest case: no interruption about
  anything I already know about. Cost: a dismissed and forgotten pull request is not raised again
  until the daemon restarts.
- **B. Reminders, no snooze.** A pull request that stays my turn is notified again after a while.
  Strongest case: a forgotten pull request comes back. Cost: reminders about deliberate delays,
  which train dismissal.
- **C. Reminders, snoozed from the notification.** An action on the notification hides that pull
  request for a chosen time. Strongest case: the decision is made where the interruption happens.
  Cost: handling a response from the notification, and storing the snooze so a restart does not
  forget it.
- **D. Reminders, snoozed from the TUI.** A key on the pull request's row hides it from
  notifications for a time. Strongest case: no notification interaction to build. Cost: snoozing
  needs hub open, and the snooze still has to be stored where the daemon reads it.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Sourced* (2026-10-05, `terminal-notifier -help`): terminal-notifier 3.1.0 can add buttons to a
  notification (`-action`) and prints the one chosen, waiting until a response or `-timeout`
  expires. Option C is reachable with the delivery tool #344 already uses, at the cost of a call
  that waits.
- *Measured* (2026-10-05, recorded on #344): posting again to an existing `-group` replaced the
  earlier notification and presented it again as the newest card. Any reminder is a new
  interruption, not a quiet update.
- *Reasoned* (2026-10-17): a reminder in a summary card naming every waiting pull request costs one
  interruption per reminder rather than one per pull request. That bears on B, C and D alike.
