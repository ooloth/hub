## Purpose

Diagnoses a Loki application error by finding where it originates in the codebase, correlating with recent changes, and recommending a fix.

## Prerequisites

- Running in the project's local worktree (hub TUI ensures this)

## Context

Launched from the hub TUI with:
- `project` — project name (e.g. "mapapp")
- `env` — environment (e.g. "prod", "internal")
- `title` — error category (e.g. "MemoryError")
- `message` — stable error label (e.g. "OOM killed")
- `line` — raw JSON log line with full context

## Investigation pattern

### 1. Parse the log line

Read the raw JSON `line` for the full log context: stack trace, request details, or custom fields. Extract the most specific signal — an exception type, a function name, a message prefix.

### 2. Trace to source

Using `rg` and `Read`, find where in the codebase this error is raised or logged. Look for the exception class, the log message string, or the surrounding context. Read the relevant function and its callers.

### 3. Check recent history

```bash
git log --oneline -20 -- <affected-files>
git diff HEAD~5 -- <affected-files>
```

Look for changes in the last few commits that could explain a new or changed error pattern.

### 4. Form a hypothesis

Based on the error signal, the source location, and the recent history, name a likely cause. If multiple causes are plausible, rank them.

### 5. Validate

If the hypothesis is testable from the local codebase (a recent change, a clear logic bug, a missing guard), confirm it. If validation requires runtime access, name what you'd need to verify.

### 6. Stop when

You can name the error's origin, its likely cause, and at least one concrete next step. Three iterations is usually enough; if not, surface what you know and flag what needs a human look.

## Output format

**Error:** `<title>` — `<message>` in `<project>:<env>`

**Origin:** File and function where the error is raised or logged.

**Cause:** One or two sentences explaining why it's happening.

**Options:**
1. First fix — when to use it
2. Second fix — if applicable

If the cause is genuinely ambiguous, say so and list what additional signals would resolve it.
