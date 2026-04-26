# workflows

Orchestrated operations — the "what this tool does."

**Rules:**
- Each file is one end-to-end operation (e.g. morning_briefing, prioritize_prs)
- Composes clients and store calls; contains no I/O of its own
- Imported by ui/; never imports ui/

**Lives here:** the named things hub can do, expressed as sequences of client fetches, store reads/writes, and domain logic.
