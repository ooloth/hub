# Docs theme — heuristics for `/repo-scan docs`

Identifies documentation that is broken, drifted from reality, internally inconsistent, or missing where it would provide clear value. The goal is a doc surface that a cold reader (human or agent) can trust and act on.

## Doc surfaces to scan

- `README.md` / `README.*` at any level
- Any `.md` file under `docs/`, `doc/`, `.claude/`, or similar doc directories
- Inline module-level or file-level doc comments where these are the primary documentation for a module (e.g. a `//!` block at the top of `lib.rs`, a docstring at the top of `__init__.py`) — not scattered inline comments

## Heuristics

### Tier 1 — Broken references (file immediately)

A doc references something that no longer exists.

**What to look for:**
- File paths (`src/foo.rs`, `clients/github/mod.rs`) that are not in the current file tree
- Function names, struct names, or module names cited as "see X" or "defined in X" that do not appear in the codebase
- CLI commands or subcommands (`hub daemon`, `cargo run --bin foo`) that do not match the current `Justfile`, `Makefile`, `package.json` scripts, or binary definitions
- Link anchors to headings that no longer exist in the target file

**Verification:** cross-reference against the repo file tree and the `Justfile` / `Makefile` / equivalent.

**False positives to skip:**
- Paths written in clearly illustrative style: `path/to/your/config.toml`, `<owner>/<repo>`, `org/my-app`
- Commands in "planned" or "future" sections explicitly marked as not yet implemented
- External URLs (too noisy; out of scope for this theme)

---

### Tier 2 — Drift (file when confident)

A doc makes a factual claim about the codebase that the code contradicts.

**What to look for:**
- Architectural claims: "hub imports X via Y", "config/ is only imported by ui/" — verify against actual import structure
- Behavior claims: "`hub status` fetches live data on every call" — verify against the relevant source file
- Stack or dependency claims: "uses sqlx" when the code uses rusqlite, "written in Python" when it's Rust
- Command output examples that look wrong given the current code

**Verification:** read the referenced source file or `Cargo.toml` / `package.json` to confirm the claim.

**False positives to skip:**
- Claims in `## Out of scope` or `## Planned` sections — future intent, not current fact
- Examples prefixed with `# example` or `// example` where the illustrative nature is clear

---

### Tier 3 — Inconsistency (file when confident)

Two docs in the same repo make contradictory claims about the same thing.

**What to look for:**
- Two docs describing the same command, pattern, or convention differently
- A CLAUDE.md rule that contradicts a `docs/conventions/` file
- A playbook that prescribes a different approach than the architecture doc for the same concern

**Verification:** quote both the conflicting passages. Do not file if one doc is clearly older and superseded by the other (check file context for "supersedes" or "deprecated" language).

---

### Tier 4 — High-ROI gaps (surface for user judgment; do not auto-file)

A doc surface is absent where it would clearly help a cold reader or agent.

**What to look for:**
- A `clients/`, `workflows/`, or `store/` subdirectory with no `README.md` and no module-level doc comment explaining its purpose and usage pattern
- An obvious design decision (a non-trivial architectural choice visible in the code) with no ADR and no explanation in any doc
- A `README.md` that names the project but has no usage example — reader cannot tell how to run or invoke it
- A playbook that has no "Done when" section — no way to know when the task is complete

**Do not file gaps automatically.** Present them to the user and ask whether they want issues filed.

**False positives to skip:**
- Intentional stubs: files containing only `# TODO` or a single-sentence placeholder — the gap is acknowledged
- Test directories, vendor directories, generated files

## Ranking within a repo

When multiple findings exist in the same repo, order them:

1. Tier 1 (broken refs) — file immediately, high confidence
2. Tier 2 (drift) — file when confident after verification
3. Tier 3 (inconsistency) — file when confident after quoting both sides
4. Tier 4 (gaps) — present last, let user decide

## Issue granularity

One issue per doc file (or per module if the finding is in inline doc comments). A file with three broken references is one issue — the fix-agent reads the whole file and corrects it in one pass. Do not split findings within a single file into multiple issues.
