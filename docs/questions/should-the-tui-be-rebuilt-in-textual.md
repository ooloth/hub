---
opened: 2026-09-16
status: open
resolves_into: decision
---

# Should the TUI be rebuilt in Python and Textual instead of Rust and Ratatui?

## Why it matters

[Decision 007](../decisions/007-tui-over-web-app.md) settles TUI over web app. Ratatui appears only
in its Consequences, riding along on that headline. A reasonable person could have decided TUI over
web app one way and Ratatui over Textual the other, so the framework is a second decision that no
record argues. It constrains every file in `ui/tui`, and nothing states what would reverse it.

Decision 007 also names the one condition that reopens the UI question: hub becomes an agent
operations center where parallel runs are reviewed side by side. Under Ratatui, reaching that state
means building a second UI. Textual serves the same application to a browser with `textual serve`,
so the two frameworks differ in what that future costs. That difference is the kind of asymmetry
that decides a choice, as opposed to which framework is more pleasant today.

Answering it wrong in the other direction is expensive too. `ui/tui` is roughly 9,800 lines of the
workspace's 18,154, and a port takes 641 tests and 51 snapshot files with it.

## What would settle it

A spike. Rebuild two screens in Textual, the unified list and the PR detail pane, reading the same
JSON payload the Rust TUI already writes to the `status_cache` row in `~/.hub/hub.db`. Budget a day,
delete it afterwards, keep the observations.

Two observations decide it:

1. **Whether the hand-drawn chrome comes free.** There are two places hub patches the terminal
   buffer directly because Ratatui has no widget for them: the `├` and `┤` stitching where an
   urgency divider meets the list border (`ui/tui/src/render/unified.rs:235`), and the `┬`-capped,
   `┴`-footed column divider in the PR detail pane (`ui/tui/src/render/pr.rs:169`). Textual styles
   borders with CSS. If both come out of a stylesheet, the claim that the framework costs real work
   holds. If they need the same hand-drawing, it does not.
2. **What the snapshot workflow costs.** Textual's snapshots are SVG; insta's are plain text
   buffers. None of the existing ones port, so every one has to be regenerated and eyeballed. The
   spike is where that stops being a guess.

Sequencing matters more than the spike's result. Decisions 019 through 022 are accepted and unbuilt:
the daemon does not exist, and the TUI still fetches and writes its own cache. A spike run before
they land measures code whose shape is about to change, and a port started before they land ports it
twice.

## Resolves into

[../decisions/](../decisions/), as a record naming the TUI framework, whichever way it goes. "Stay
with Ratatui" earns one as much as "move to Textual" does, because the record that would establish
it does not exist and the choice moves a boundary rather than a value.

## Source

Raised 2026-09-16, in a discussion of rewriting hub in Python. The Decision 007 gap surfaced in the
same discussion and holds regardless of what the spike shows.

## Options

- **A. Stay with Ratatui.** Strongest case: the seam is already in the right place, so the framework
  is not distorting the design. 16 widget draw calls across 6 files are the entire imperative
  surface, and nothing in `state/`, `display/`, `input.rs` or `investigations/` imports Ratatui at
  all. Cost: the browser surface Decision 007 deferred stays expensive to reach, and the hand-drawn
  chrome plus the 545-line markdown renderer stay hub's to maintain.
- **B. Rebuild in Textual.** Strongest case: `textual serve` keeps Decision 007's one reopening
  condition cheap; Textual ships the `Markdown` widget, `TextArea` and CSS borders that five
  dependencies currently provide (`pulldown-cmark`, `syntect`, `two-face`, `ansi-to-tui`,
  `tui-textarea`); and Python is where the Claude Agent SDK lives if hub ever drives investigation
  sessions rather than launching them through tmux. Cost: 641 tests and 51 snapshots do not port
  mechanically, and the domain newtypes lose compile-time enforcement in exchange for mypy at CI
  time.
- **C. Not yet.** Strongest case: Decisions 019 through 022 are unbuilt, so the code a port would
  carry is not settled, and neither the spike nor the port measures anything durable until it is.
  Cost: none today, beyond the question staying open.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a decision record._

- *Measured* (2026-09-16): the workspace is 18,154 lines of Rust. `just test` runs 641 tests,
  expanded by rstest parametrization from 483 test functions, and there are 51 `.snap` files. All
  three drift with every commit, so re-derive them rather than trusting the numbers here:
  `fd -e rs -X cat | wc -l`, `just test`, `fd -e snap | wc -l`.
- *Measured* (2026-09-16): 16 `render_widget`/`render_stateful_widget` calls exist, in
  `render/mod.rs` (8), `render/pr.rs` (3), `render/unified.rs` (2), and one each in
  `render/detail.rs`, `render/issue.rs` and `render/log.rs`. Ratatui is imported only by files under
  `render/` and `markdown/`, plus `main.rs`; `render/status_bar.rs` imports none of it.
- *Measured* (2026-09-16): Textual's latest release is v8.2.8, 30 June 2026, from
  https://github.com/Textualize/textual/releases.
- *Sourced*: `ui/tui/Cargo.toml` pins ratatui at 0.29 because 0.30 moved `Widget` into a separate
  crate, and pins itertools at 0.13 solely to match ratatui's own copy and satisfy `cargo-deny`'s
  duplicate-version ban. Both pins carry the reason in a manifest comment.
- *Sourced*: there is no release pipeline. `.github/workflows/ci.yml` runs checks and produces no
  artifacts; installation is `cargo install --path ui/tui` on the machine that will run it. Neither
  language has a distribution advantage here until that changes.
- *Reasoned*: Rust's performance advantage does not bind on this workload. Every operation hub
  performs is network I/O (GitHub GraphQL, Linear, Loki) or a subprocess (`git`, `tmux`, `op`).
- *Unverified*: the detail pane re-parses markdown and re-runs syntect highlighting on every frame,
  with no memoization between frames. Reported by an agent reading `render/pr.rs` and
  `render/issue.rs`; not independently checked.
- *Unverified*: Textualize's standing as a company. A search on 2026-09-16 settled nothing either
  way, so the release cadence above is the only health signal recorded here.
