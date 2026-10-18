---
opened: 2026-10-17
status: open
resolves_into: decision
---

# Should clicking a notification start an investigation?

## Why it matters

Clicking a pull request's notification opens the pull request in the browser
([#344](https://github.com/ooloth/hub/issues/344)). Starting an investigation means opening hub
and pressing `i` on its row. If what I do after clicking is usually something else, such as
reviewing it with an agent or replying in hub, the click lands one step short every time, and
that step is the friction notifications exist to remove.

[#327](https://github.com/ooloth/hub/issues/327)'s ideal state names the investigation as the
convenient next step after a notification. The click opens the browser instead, until living with
it shows what is missing.

## What would settle it

Living with notifications that open the browser, and noticing what I do next. If I routinely
follow the click with the same manual step, that step is what the click should do. If the browser
is usually where the work happens, the question closes with nothing built.

## Resolves into

[../decisions/](../decisions/), if the click changes: running hub's own command from a
notification is a new interface between the notification and hub.

## Source

Raised 2026-10-17 while comparing #327's ideal state with what #344 built.

## Options

- **A. Open the pull request in the browser.** What #344 builds. Strongest case: GitHub is where a
  review or a merge happens. Cost: an investigation needs hub opened separately.
- **B. Start an investigation in a named tmux window.** `terminal-notifier -execute` runs a command
  when a notification is clicked. Strongest case: the agent session starts from the interruption.
  Cost: a click from outside a terminal has to reach tmux and Claude Code, and an investigation
  started without asking is heavier than a page opening.
- **C. Offer both as buttons.** `terminal-notifier -action` adds buttons. Strongest case: the
  choice is made where the interruption happens. Cost: `-action` waits for a response, so each card
  holds a process open until it is answered or times out.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Sourced* (2026-10-17, `terminal-notifier -help`, 3.1.0): `-execute COMMAND` runs a shell command
  on click, and `-action` adds buttons and waits for a response, printing the chosen title.
