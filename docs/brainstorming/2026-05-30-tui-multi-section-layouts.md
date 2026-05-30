# TUI Multi-Section Layouts

Aspirational layouts for hub's TUI at fullscreen on a 13" laptop (~195 columns).
These are designed to answer: _can a TUI compete with a web dashboard for
hub's purposes?_

Each layout shows a different "screen" you'd navigate to. Together they form
a coherent app where Enter drills in and Esc backs out.

## Visual conventions

Plain text can't show the styling layer that does much of the work. In a real
ratatui implementation, all of the following are stacked:

- **Bold + bright** for primary text (titles, counts)
- **Normal** for secondary text (status, metadata)
- **Dim 50–60%** for tertiary text (dates, separators, hints)
- **Color accents** for urgency: `red`=critical, `yellow`=high, `cyan`=medium, gray=low
- **Color accents** for type: `magenta`=PR, `green`=task, `red`=error, `blue`=session
- **Reverse video** for the focused row/panel
- **Rounded corners** (`╭╮╰╯`) for cards; **light lines** (`─│`) for the app shell
- **Subtle background tint** for the selected pane in a multi-pane view (terminals support this)

Icons assume a nerd font: `●` working, `◐` paused, `✓` done, `✗` failed,
`⚠` error, `◌` no-reviews, `◉` task, `▸` selected, `↑↓` trending,
`▇▆▅▃▂` sparkline blocks, `⠋` spinner.

---

## Navigation model

```

                                            ┌──────────────────┐
                                            │       HOME       │  ← dashboard overview
                                            │   (Layout A)     │    1 screen, 6 panel summaries
                                            └────────┬─────────┘
                       ┌─────────┬─────────┬─────────┼─────────┬─────────┐
                      [p]       [a]       [e]       [t]       [T]       [/]
                       ▼         ▼         ▼         ▼         ▼         ▼
                    ┌─────┐  ┌─────┐   ┌─────┐   ┌─────┐  ┌─────┐  ┌────────┐
                    │ PRs │  │AGNTS│   │ERR  │   │TASKS│  │TODAY│  │ search │
                    │  B  │  │  C  │   │     │   │     │  │  E  │  │        │
                    └──┬──┘  └──┬──┘   └─────┘   └─────┘  └─────┘  └────────┘
                       │        │
                      [↩]      [↩]
                       ▼        ▼
                  ┌──────┐  ┌──────┐
                  │ PR   │  │SESSN │
                  │detail│  │focus │
                  │(in B)│  │  D   │
                  └──────┘  └──┬───┘
                               │
                              [t]
                               ▼
                          ┌──────────┐
                          │transcript│
                          │  (full)  │
                          └──────────┘

   esc           ─── up one level
   `  (or h)     ─── jump back to HOME from anywhere
   tab           ─── cycle focus between panels in current screen
   1 2 3 ...     ─── focus panel by number (within a screen)
```

---

## Layout A · HOME · multi-section dashboard

The overview screen — every category visible at once, with the top items
from each. Press a letter to enter that category in full.

```
╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  hub                                                                                            ground control · 2026-05-30 14:22 · daemon ● 4h12m · sync 1m ago  │
├─────────────────────────────────────────────────────────────┬──────────────────────────────────────────────────────────────┬──────────────────────────────────────┤
│                                                             │                                                              │                                      │
│  PRs                                                  13    │  Agent Sessions                            6 · 2 working     │  Errors                          8   │
│  ─────────────────────────────────────────────────────      │  ──────────────────────────────────────────────────────      │  ────────────────────────────────    │
│                                                             │                                                              │                                      │
│   ●  no reviews    gmail TLS in STARTTLS         10d        │   ●  working    /implement-issue · hub #66       12m         │   ⚠ CRITICAL                         │
│   ●  conflict      Add project overview           9d        │   ●  working    /github-ci-investigate · 1284     4m         │     Dependabot error                 │
│   ●  no reviews    Fix variable name in docs      9d        │   ◐  paused     /review-pr-comments · hub #159    1h         │     gatsbytutorials.com   23h ↑      │
│   ●  no reviews    cli: broken import & doc       8d        │   ✓  done       /media-investigate              22m         │                                      │
│   ●  no reviews    workflows: filter stderr       8d        │   ✓  done       /refactor media.rs               1d         │   ⚠ WARNING                          │
│   ●  no reviews    docs: cargo-nextest            4d        │   ✗  failed     /review-code · hub #160           2d         │     Media · '.scr' invalid  ×3 now  │
│   ●  no reviews    pilots: SENDGRID validation    2d        │                                                              │     Media · '.exe' invalid  ×2 now  │
│   …+6 more                                                  │                                                              │     Media · matched by ID   ×1 now  │
│                                                             │                                                              │     Loki · error rate up    ×12 3h   │
│   press  p  to enter                                        │   press  a  to enter                                         │                                      │
│                                                             │                                                              │   press  e  to enter                 │
├─────────────────────────────────────────────────────────────┼──────────────────────────────────────────────────────────────┼──────────────────────────────────────┤
│                                                             │                                                              │                                      │
│  Tasks                                              276     │  Recent Activity                                             │  Today                               │
│  ─────────────────────────────────────────────────────      │  ──────────────────────────────────────────────────────      │  ────────────────────────────────    │
│                                                             │                                                              │                                      │
│   ◉  security    GitHub token in clone error    11d         │   14:18  ✓  finished  /implement-issue · #66                 │    ✓  3 agent runs completed         │
│   ◉  security    Validate MEDIA_URL scheme     11d         │   14:11  ▴  PR #248 opened by claude                         │    ✓  1 PR merged · #247             │
│   ◉  security    GraphQL injection in slugs     11d         │   13:58  ●  started   /github-ci-investigate                 │    ✗  1 failed · /review-code 160    │
│   ◉  privacy     Linear error body leak         11d         │   13:47  ⚠  Media × 3                                       │    ⚠  4 new critical signals         │
│   ◉  config      env vars scattered in code     11d         │   13:31  ⚠  CI fail · gatsbytutorials                        │                                      │
│   ◉  config      .env.example missing keys      11d         │   13:14  ✓  finished  /media-investigate                    │   pulse  ▂▃▅▇▇▆▅▃▂                   │
│   ◉  agent       document observability signals 12d         │   13:02  ▾  PR #247 merged · gmail TLS                       │   today  47 runs · 41 ✓  6 ✗         │
│   …+269 more                                                │   12:48  ●  started   /media-investigate                    │                                      │
│                                                             │                                                              │   yesterday  8 runs · 6 ✓  2 ✗       │
│   press  t  to enter                                        │                                                              │                                      │
│                                                             │                                                              │   press  T  to enter                 │
╰─────────────────────────────────────────────────────────────┴──────────────────────────────────────────────────────────────┴──────────────────────────────────────╯
  p  PRs    a  Agents    e  Errors    t  Tasks    T  Today    /  search    r  refresh    ?  help    q  quit
```

**What it shows:**
6 distinct panels in a 3×2 grid. Each panel is a mini-list with type-appropriate
endering. Counts and pulses give a "weather report" feel. Sparkline + run
stats in Today panel demonstrate visual data that web does easily but TUIs
often skip. Each panel labels its keybinding so the user learns by exposure.

**What the eye gets:** category boundaries are crisp, type icons act as
visual anchors per row, urgency colors lead the eye to red+yellow rows first.

---

## Layout B · PR CATEGORY · drilled into one section

After pressing `p` on home. List on left, full detail on right. The detail
pane shows everything you'd want before deciding to act, without leaving the
TUI.

```
╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  hub  ›  PRs                                                                                   13 total  ·  All  Mine 13  Drafts 0  Awaiting me 0  ·  filter:─    │
├──────────────────────────────────────────────────────────────────────┬────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                      │                                                                                            │
│   ●  no reviews · 10d                                                │   PR  #159  ·  ooloth/hub                                                                  │
│      gmail: enforce TLS cert verification in STARTTLS                │                                                                                            │
│      ooloth/media-tools · #42                                        │   workflows/implement: filter claude stderr before forwarding to caller                    │
│   ─────────────────────────────────────────────────────────────      │                                                                                            │
│   ●  conflict · 9d                                                   │    status     ◌  no reviews                                                                │
│      Add project overview, integrations, and dev commands            │    checks     ✓  6 / 6 passing      ▇▇▇▇▇▇                                                 │
│      ooloth/michaeluloth.com · #83                                   │    diff       +24 −3 in 3 files                                                            │
│   ─────────────────────────────────────────────────────────────      │    branch     fix/stderr-filter                                                            │
│   ●  no reviews · 9d                                                 │    author     ooloth                                                                       │
│      Fix wrong variable name in invariant docs example               │    updated    8d ago                                                                       │
│      ooloth/michaeluloth.com · #84                                   │                                                                                            │
│   ─────────────────────────────────────────────────────────────      │   ────────────────────────────────────────────────────────────────────────────             │
│   ●  no reviews · 8d                                                 │   Description                                                                              │
│      cli: fix broken import and stale docstring                      │                                                                                            │
│      ooloth/scripts · #55                                            │      The agent invocation path forwards stderr verbatim, which has leaked                  │
│   ─────────────────────────────────────────────────────────────      │      token-bearing lines into committed transcripts (see issue #78). This                  │
│ ▸ ●  no reviews · 8d                                                 │      change adds strip_secrets() and routes stderr through it before                       │
│      workflows/implement: filter claude stderr                       │      forwarding to the caller.                                                             │
│      ooloth/hub · #159                                               │                                                                                            │
│   ─────────────────────────────────────────────────────────────      │   Files changed                                                                            │
│   ●  no reviews · 8d                                                 │      workflows/src/agent.rs                              +24 −3                            │
│      claude: remove external rust.md reference                       │      workflows/src/lib.rs                                +1  −0                            │
│      ooloth/hub · #160                                               │      tests/agent_test.rs                                 +18 −0                            │
│   ─────────────────────────────────────────────────────────────      │                                                                                            │
│   ●  no reviews · 4d                                                 │   Related signals                                                                          │
│      docs: add cargo-nextest to CONTRIBUTING                         │      ◉ Task #78    Claude agent stderr relay must filter credential-bearing                │
│      ooloth/hub · #210                                               │      ◉ Task #82    Enforce retention limit on agent transcripts                            │
│   ─────────────────────────────────────────────────────────────      │      ◉ Task #85    Remove PR author's GitHub login from TUI display                        │
│   ●  no reviews · 4d                                                 │                                                                                            │
│      docs: deploy playbook and CLAUDE.md ref                         │   Agent sessions                                                                           │
│      ooloth/media-tools · #49                                        │      ◐ /review-pr-comments-converge · paused 1h ago                                        │
│      …5 more                                                         │                                                                                            │
│                                                                      │                                                                                            │
├──────────────────────────────────────────────────────────────────────┴────────────────────────────────────────────────────────────────────────────────────────────┤
│  5 of 13   ↑↓ navigate   [↩] review   [i] investigate   [l] lazygit   [d] diff   [o] browser   [c] resume comment session   [esc] back                            │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**What it shows:**
Same width, two panes. Left list uses two-line cards with a row separator —
fits 7 PRs comfortably with full titles. Right detail pane shows everything
about the selected PR: status with a check-pass sparkline, file-level diff
stats, description, related tasks, and any active agent session for this PR.

**Action shelf in the footer** changes per item type — this PR has a paused
comment-resolution session, so `[c] resume comment session` appears.

---

## Layout C · AGENTS · multi-session live supervision

The colleague's killer view, in TUI form. List on left, multiple live stream
panels on right showing what each active session is doing _right now_.

```
╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  hub  ›  Agents                                                  6 sessions  ·  ● 2 working  ·  ◐ 1 paused  ·  ✓ 2 done  ·  ✗ 1 failed  ·  today: 47 runs        │
├───────────────────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                   │ ╭ ● working ─ /implement-issue · hub #66 ── 12m ── 14¢ ─ 117 tools ─────────────────────────────────────────╮│
│   Sessions                                        │ │  ✓ cloned    ✓ tests written    ▸ implementing    ○ open PR                                                ││
│                                                   │ │                                                                                                              ││
│ ▸ ●  working   implement #66        12m   14¢     │ │  14:22:03  write   added test for strip_secrets() — expected None for token line                          ││
│                                                   │ │  14:22:11  write   added test for strip_secrets() — pass through plain line                                ││
│   ●  working   investigate CI        4m    6¢     │ │  14:22:14  shell   cargo test --package workflows                                                          ││
│                                                   │ │  14:22:17    test result: FAILED · 3 passed · 3 failed · 0 ignored                                          ││
│   ◐  paused    review #159           1h   22¢     │ │  14:22:18  write   implementing strip_secrets() using TOKEN_PATTERN regex                                  ││
│                                                   │ │  14:22:24  ⠋       running cargo test...                                                                    ││
│   ✓  done      media investigate   22m   18¢     │ ╰────────────────────────────────────────────────────────────────────────────────────────────────────────────╯│
│                                                   │ ╭ ● working ─ /github-ci-investigate · gatsbytutorials #1284 ── 4m ── 6¢ ──────────────────────────────────╮│
│   ✓  done      refactor media      1d    31¢     │ │  ✓ fetched workflow    ▸ analyzing logs    ○ root cause    ○ propose fix                                  ││
│                                                   │ │                                                                                                              ││
│   ✗  failed    review #160          2d   47¢      │ │  14:21:34  shell   gh run view 1284 --log                                                                   ││
│                                                   │ │  14:21:48  read    extracted 1,247 log lines                                                                ││
│   ──────────────────────────────────              │ │  14:21:55  shell   grep -i "error\|fail" → 23 matches                                                       ││
│                                                   │ │  14:22:01  ⠋       analyzing dependency resolution failures...                                              ││
│   today  ▇▇▇▇▇▆▅▃▂  47 runs                       │ ╰────────────────────────────────────────────────────────────────────────────────────────────────────────────╯│
│         41 ✓  ·  6 ✗  ·  cost $4.18               │ ╭ ◐ paused ── /review-pr-comments-converge · hub #159 ── 1h ago ── 22¢ ─────────────────────────────────────╮│
│                                                   │ │  ✓ comments fetched    ✓ 2/3 addressed    ▸ awaiting confirmation                                          ││
│   this week                                       │ │                                                                                                              ││
│   ▇▇▇▆▅▃ 312 runs · 287 ✓ · 25 ✗                  │ │  13:18:22  ask     "Should this also handle the case where stderr is empty?                                ││
│                                                   │ │                     See @ooloth's comment on line 47."                                                       ││
│                                                   │ │  ─ waiting for human ─                                                                                       ││
│                                                   │ ╰────────────────────────────────────────────────────────────────────────────────────────────────────────────╯│
│                                                   │                                                                                                              │
├───────────────────────────────────────────────────┴───────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│  1 of 6   ↑↓ session   [↩] focus selected   [j/k] scroll stream   [r] resume   [k] kill   [n] new session   [esc] home                                          │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**What it shows:**
3 live stream panes stacked on the right, one per active or paused session.
Each pane has its own progress bar (`✓ ✓ ▸ ○`) plus the most recent stream
output. The paused session shows the agent's pending question with a "waiting
for human" indicator — you know at a glance what's blocked on you. Left rail
shows all sessions with cost (`14¢`) and duration, plus daily/weekly run
totals as sparklines.

This is the screen the colleague's web app has. The TUI version is here.

---

## Layout D · SESSION FOCUS · zoomed into one agent

After pressing Enter on a session row. Everything about one run on one screen:
task, progress checklist, live stream, workspace state, cost, related runs.

```
╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  hub  ›  Agents  ›  implement-issue #66                                                       ●  working  ·  12m elapsed  ·  14¢  ·  ws: implement-issue-66       │
├───────────────────────────────────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                   │                                                                                               │
│   Task                                                            │   Stream  ·  follow on  ·  ⠋ live                                                              │
│                                                                   │                                                                                               │
│   Document observability signals so agents can diagnose           │   14:10:08  start   workspace cloned · branch agent/implement-66                              │
│   failures without reading source code                            │   14:10:14  read    CLAUDE.md · workflows/src/agent.rs · workflows/src/lib.rs                  │
│                                                                   │   14:11:02  plan    write tests first for strip_secrets()                                      │
│   ooloth/hub  ·  Issue #66                                        │   14:12:18  write   tests/agent_test.rs  +18 lines                                             │
│                                                                   │   14:14:50  shell   cargo test                                                                 │
│   ─────────────────────────────────────────────────────           │   14:14:53     test failed (expected — no impl yet)                                            │
│                                                                   │   14:15:20  write   workflows/src/agent.rs strip_secrets() helper                              │
│   Progress                                                        │   14:15:31  write   workflows/src/lib.rs routing stderr through helper                         │
│                                                                   │   14:21:48  read    tests/agent_test.rs                                                         │
│   ✓  cloned ooloth/hub into workspace            ↗  2m            │   14:21:52  read    workflows/src/agent.rs                                                      │
│   ✓  identified affected modules                 ↗  4m            │   14:22:03  write   added test for strip_secrets() · None for token line                       │
│   ✓  wrote 3 failing tests for strip_secrets()   ↗  7m            │   14:22:11  write   added test for strip_secrets() · pass through plain line                   │
│   ▸  implementing strip_secrets()                                 │   14:22:14  shell   cargo test --package workflows                                              │
│   ○  open PR                                                      │   14:22:17     test result: FAILED · 3 passed · 3 failed · 0 ignored                            │
│                                                                   │   14:22:18  write   implementing strip_secrets() using TOKEN_PATTERN regex                     │
│   ─────────────────────────────────────────────────────           │   14:22:24  ⠋       running cargo test...                                                       │
│                                                                   │                                                                                               │
│   Workspace                                                       │   ────────────────────────────────────────────────────────────────────                         │
│     ~/.hub/workspaces/implement-issue-66                          │                                                                                               │
│     branch     agent/implement-66                                 │   Artifacts                                                                                   │
│     diff       +42 −3 in 3 files                                  │                                                                                               │
│     staged     3 changes                                          │     report.md           ⌛ will write at completion                                            │
│                                                                   │     diff.patch          ⌛ will write at completion                                            │
│   Cost                                                            │     pr-description.md   ⌛ will write at completion                                            │
│     14¢  ·  117 tool calls  ·  88K input · 12K output             │                                                                                               │
│                                                                   │   Related runs                                                                                │
│   Skill                                                           │                                                                                               │
│     /implement-issue  ·  v3                                       │     ✗ /review-code · hub #160       2d   same author, related changes                          │
│     ~/.claude/skills/implement-issue.md                           │     ✓ /implement-issue · hub #58    4d   similar pattern (stdout filtering)                    │
│                                                                   │     ✓ /media-investigate           22m  finished, no overlap                                  │
│                                                                   │                                                                                               │
├───────────────────────────────────────────────────────────────────┴───────────────────────────────────────────────────────────────────────────────────────────────┤
│  [t] open transcript    [w] open workspace    [d] diff in lazygit    [k] kill session    [s] silence notifications    [esc] back                                 │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**What it shows:**
Two panes again, but both are "detail" panes now. Left has structured fields
(task, progress, workspace, cost, skill). Right has the live stream + artifact
status + related runs. This is what reviewing-an-agent-run looks like inline —
no leaving for `tmux attach`, `ls`, `cat report.md`.

The `↗ 2m / 4m / 7m` next to progress steps shows how long each step took.
Cost breakdown is web-app-grade detail.

---

## Layout E · TODAY · cross-domain priority view

The view that argues hub deserves to exist. Critical/High items from all
domains ranked together, with context that makes it clear _why_ each one is
ranked here. Trending indicators show which problems are growing.

```
╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  hub  ›  Today                                            43 attention items  ·  4 critical  ·  18 high  ·  21 medium  ·  pulse ▂▃▅▇▆▅▃▂ trending down 12%        │
├───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                                                                                   │
│   CRITICAL  ·  4  ·  needs action now                                                                                                                            │
│   ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────       │
│                                                                                                                                                                   │
│   ⚠  prod error    gatsbytutorials.com   Dependabot encountered error                          23h   ↑ growing  ·  3 occurrences  ·  blocks weekly deploy        │
│   ⚠  home server   media drive            Media — import blocked × 6                            now   ↑ new      ·  6 in 12 min  ·  possible worm                │
│   ⚠  security      ooloth/hub             GitHub token in clone error output                    11d   ⏸ stable  ·  agent task open #74  ·  needs human review     │
│   ✗  agent failed  ooloth/hub             /review-code · #160 errored                            2d    ⏸ stale   ·  not retried since failure                     │
│                                                                                                                                                                   │
│   HIGH  ·  18  ·  needs action this week                                                                                                                         │
│   ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────       │
│                                                                                                                                                                   │
│   ●  PR review     ooloth/media-tools     gmail: enforce TLS cert in STARTTLS                  10d   ⏸ stale   ·  no reviewers responded                          │
│   ●  PR review     ooloth/hub             workflows: filter claude stderr                       8d   ⏸ stale   ·  conflict-free  ·  related to task #78           │
│   ◉  security      ooloth/hub             Validate repo slugs before GraphQL embedding         11d   ↑ similar ·  same root cause as #77                          │
│   ◉  security      hub-private            Validate MEDIA_URL scheme                            11d   ⏸ stable                                                    │
│   ◉  security      ooloth/hub             Path traversal in repo slug REST URL                 11d   ↑ similar ·  same root cause as #76                          │
│   ◉  privacy       ooloth/hub             Linear error body propagates to caller                11d                                                                │
│   ◉  privacy       ooloth/hub             Strip raw API response body from Linear errors        11d                                                                │
│   ●  PR review     michaeluloth.com       Add project overview, integrations                     9d   ⚠ conflict ·  3 merge conflicts                            │
│   ⚠  media        media drive            Found series via grab history but matched by ID      now   ⏸ stable                                                    │
│   ◉  agent-harness ooloth/scripts         Document observability signals for agents             12d                                                                │
│   ◉  config        ooloth/hub             env vars scattered in workflow code                   11d                                                                │
│   …+7 more                                                                                                                                                        │
│                                                                                                                                                                   │
│   MEDIUM  ·  21  ·  press  m  to expand                                                                                                                          │
│                                                                                                                                                                   │
├───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│  cross-domain ranking · workflows classify · hub aggregates    [↩] enter item   [g] group   [s] sort   [/] search   [esc] home                                  │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

**What it shows:**
The differentiator. Sentry, Linear, GitHub, Media — each only shows their
own world. This view ranks across all of them with a trend indicator
(`↑ growing`, `⏸ stable`, `↑ similar` for tasks that share a root cause).
The "why this is ranked here" microcopy (`blocks weekly deploy`, `possible
worm`, `same root cause as #77`) is the editorial layer no source tool can
provide because no source tool sees the others.

The pulse sparkline at top right shows urgency trending day-over-day.

---

## What these layouts argue

**The visual ceiling is much higher than the current TUI suggests.** Layouts
A and C in particular are dense, hierarchical, multi-section views that
deliver web-dashboard at-a-glance scanability — without typography
variation, just through disciplined color, spacing, sectioning, and icons.

**Where the TUI still loses to a web app:** rendering of agent reports,
markdown notes, diffs — anything that's "long-form content with formatting."
For hub's current scope (agent reports read by future agents, diffs reviewed
in lazygit), this gap doesn't bite.

**Where the TUI matches or beats a web app for hub:**

- Cross-domain ranked list (Layout E) — TUIs render uniform-row tables better
  than web actually, because uniform width forces visual comparability
- Live multi-stream supervision (Layout C) — same content density as web,
  faster keyboard nav
- Detail-rich item inspection (Layouts B and D) — fully achievable; the
  detail pane has plenty of room for structured data

**What's required to get here from today:**

- Two-line item cards as default (vs current single-line)
- Section headers and grouping
- Detail pane on the right of category screens
- Multi-stream live tail in Agents view
- Nerd font assumed for the runtime
- Consistent color palette tied to type and urgency
- Trend indicators harvested from time-series in the cache

None of this is structurally new. It's a redesign of the existing TUI, not
a new technology.

## Verdict against the web-app question

If layouts A, C, and E feel close to "web-app good" for your use, the TUI
path is viable for the personal-IDE vision. If layouts B and D feel insufficient
when you imagine actually reading agent reports or reviewing detail all day,
the web case re-emerges — but the bite is specifically on long-form content,
not on dashboard density.
