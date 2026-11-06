# domain

Types and pure business logic. The shared vocabulary of the system.

**Rules:**
- No I/O, no network calls, no file reads
- No imports from other hub crates
- Everything else imports from here; nothing here imports upward

**Lives here:** the shared types and the pure logic over them, one concept per file:
- pull requests and issues: `PullRequest`, `Issue`, `LinearIssue` and their states (`pr.rs`,
  `issue.rs`)
- CI, Loki and GCP signals: `CiFailure`, `LokiEntry`, `GcpEntry` and the query types (`ci.rs`,
  `loki.rs`, `gcp.rs`)
- `Urgency`, the ranking of a signal (`urgency.rs`)
- `Profile`, which set of hub's state a process uses (`profile.rs`)
- `RepoSlug`, `UntrustedText` and the investigation types `InvestigationPrompt` and
  `InvestigationWindow`
- `ProjectShortName`, what a project is called in tmux window names (`project_short_name.rs`)
- the window and tab types: `GitHubLogin`, `RexLabel`, `WorkersSession`, `RexSessionId`,
  `WindowProject`, `PrWindowActivity` and `ReviewSkill`, one file each, and
  `window_segment.rs`, which turns external text into the segments of a window name

`lib.rs` names every module with a line on what it holds, including those this list leaves out,
such as the daemon's health records, credential failures and whose turn a pull request is.

Config structs are in `config/`, not here.
