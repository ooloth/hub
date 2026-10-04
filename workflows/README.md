# workflows

Orchestrated operations — the "what this tool does."

**Rules:**
- Each file is one end-to-end operation (e.g. `status`, `github_prs`)
- Composes clients and store calls. Most files do no I/O of their own; `fetch.rs` and `git.rs` run
  `git` subprocesses and touch the filesystem for investigation worktrees
- Imported by ui/; never imports ui/

**Lives here:** the named things hub can do, expressed as sequences of client fetches, store reads/writes, and domain logic.
`sources.rs` gathers every source at once, each within `SOURCE_TIMEOUT` (`status.rs`), so one
source that never answers cannot hold back the rest.

To add a workflow: [docs/playbooks/add-a-workflow.md](../docs/playbooks/add-a-workflow.md)
