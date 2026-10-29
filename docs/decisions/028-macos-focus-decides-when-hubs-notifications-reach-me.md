---
number: 028
status: accepted
date: 2026-10-18
amended: 2026-10-28
---

# 028 — macOS Focus decides when hub's notifications reach me, and hub has no quiet hours or pause of its own

## Forced by

- [#327](https://github.com/ooloth/hub/issues/327)'s ideal state: "I'm notified locally only while
  logged in and able to act — never off-hours". The plan of record reads "able to act" as being
  logged in to the desktop session, which a LaunchAgent gives for free, and tracks presence gating
  beyond that as not built.
- The same issue lists pausing or muting the whole system as out of scope for now, on condition
  that the design does not foreclose adding it later.
- [#344](https://github.com/ooloth/hub/issues/344) posts every notification through
  terminal-notifier (`fr.julienxx.oss.terminal-notifier`), which macOS treats as an app of its own,
  separate from every other notification on the machine.
- Two observations on 2026-10-18, each with a test card posted through terminal-notifier in a
  group of its own:
  - **A Focus that does not allow terminal-notifier holds its cards.** With such a Focus turned
    on from Control Center, the card appeared nowhere. When the Focus was turned off, it appeared
    on screen. *Measured.* A Focus schedule can leave terminal-notifier out of its allowed apps,
    checked in System Settings. *Measured.*
  - **Switching off terminal-notifier's notifications drops its cards.** With "Allow
    notifications" off for terminal-notifier in System Settings > Notifications, the card appeared
    nowhere, and still nowhere after notifications were switched back on. *Measured.*

## Decision

When hub's notifications reach me is decided by macOS, not by hub. A scheduled Focus that leaves
terminal-notifier out of its allowed apps keeps them away outside the hours I choose, and turning on
such a Focus by hand pauses them for as long as I want. Hub keeps posting and removing cards as
turns begin and end, whatever the Focus. While a Focus holds cards, the daemon still removes those
whose turn ends, so when it lifts I see only the pull requests still waiting on me.

Switching off terminal-notifier's notifications in System Settings is not a way to pause hub. A card
posted while it is off is lost, and the daemon does not post a turn again for the same reason while
it keeps running, so a pull request that became my turn in that time is never notified until the
daemon restarts. (The 2026-10-28 addendum below changes this: the card is now posted on every
refresh.)

The option kept open: hub can still gain a pause or quiet hours of its own later. Nothing in the
daemon assumes notifications are always delivered, because a card that never reaches the screen
already changes nothing about what the daemon records or does next.

## Rejected

- **Quiet hours and a pause built into hub**, such as hours in `hub.toml` and a pause from the TUI.
  Its strongest case: it needs no operating system setup, it could hold back one kind of signal
  while letting another through, and hub would know a pause is on, so it could show that in the
  status bar and post again when the pause ends. Rejected because a Focus already holds hub's cards
  and delivers them when it ends, so this would build in hub what macOS does for every app, and add
  time and pause state to a daemon that has none today. Reverses if hub's notifications go somewhere
  macOS does not govern.
- **Not yet: leave it unrecorded until someone asks for quiet hours.** Its strongest case: nothing
  is broken, and a record constrains later work. Rejected because quiet hours and a pause are the
  obvious next requests of any notifier, so a future session would scope and build one without
  knowing the operating system already covers both. Reverses if this record's decision is reversed.
- **Switching off terminal-notifier's notifications as the pause.** Its strongest case: it silences
  hub alone, leaving every other app's notifications alone. Rejected because it drops cards rather
  than holding them, and the daemon does not post them again for the same reason, as measured
  above. Reverses if the daemon starts posting a turn again after its card was lost, which is the
  reminder question in #344's Out of scope.

## Risk

Hub cannot tell a held card from a delivered one. The status bar never says notifications are
paused, so a Focus left on by mistake looks the same as a quiet day until I check Notification
Center or notice the Focus. The pause also covers all of hub's notifications at once: a Focus
cannot hold back one kind of turn while letting another through.

## Revisit when

Hub's notifications go to a channel macOS does not control, such as Slack
(`docs/questions/should-notifications-also-go-to-slack.md`). Or I reach for a pause often enough
that turning on a Focus gets in the way, or want hub to hold back some turns and not others.

## Also update

- [x] questions/README.md — nothing settled or re-scoped. Snoozing a single pull request is a
      different question and stays open in `should-a-waiting-pull-request-be-snoozable.md`.
- [x] vision.md — nothing defined or foreclosed. "A notification firehose" already says hub
      interrupts only when the ball lands in my court; this record says when that interruption is
      allowed to arrive.

## Addendum (2026-10-28) — one card, posted on every refresh

Since [#382](https://github.com/ooloth/hub/issues/382), hub posts one card that counts the pull
requests that are my turn, after every refresh that finds one, instead of a card per pull request
posted once. The decision stands: a Focus still decides when the card reaches me. Two things above
no longer hold as written:

- **A card lost while terminal-notifier's notifications are switched off comes back.** The next
  refresh after they are switched on posts the card again, within one 15-minute interval. The
  third rejected option's reversal condition, "the daemon starts posting a turn again after its card
  was lost", is met. Switching them off now works as a pause that drops what arrives during it and
  shows the current count once it ends, which a Focus still does better by holding the card.
- **"Hub keeps posting and removing cards as turns begin and end"** now reads: hub posts the card
  after every refresh that finds a turn, and removes it after a refresh that heard from every
  source and found none.
