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
- session transcript parsing: `StreamBlock` (`session.rs`)

Config structs are in `config/`, not here.
