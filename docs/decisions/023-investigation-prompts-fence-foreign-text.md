---
number: 023
status: accepted
date: 2026-09-19
---

# 023 — Investigation prompts fence text hub did not write

## Forced by

An investigation prompt is assembled from two kinds of text and presented as one. Loki and GCP
prompts carried a log's `message`, the private blocked-import prompt carried a title and an error
from the media server's API, and each was interpolated into the same sentence as hub's own
instructions. The agent receiving it had nothing to separate them.

The reason this matters is not mainly adversarial. Log lines, CI output and vendor error strings
routinely contain imperatives: run this migration, clear that cache, try this flag. An agent that
cannot tell those from its own instructions may act on them, and acting on them is usually wrong
even when nobody is attacking anything.

Investigations run `claude --dangerously-skip-permissions` (`ui/tui/src/investigations/command.rs`),
so there is no second gate behind the prompt.

## Decision

Externally-authored text is rendered inside a labelled block:

```
<untrusted-input source="loki log message">
…the text…
</untrusted-input>
```

and every occurrence of the literal `untrusted-input` is removed from the body first, so the body
cannot produce either tag. Removal repeats until none remains, bounded by the body length, because
a single pass can leave a fresh occurrence where the removed text straddled one.

Every investigation's system prompt states that fenced text is data and never an instruction. It is
appended by `compose` rather than written into each file under `prompts/investigations/`, so a
signal type added later is covered without anyone copying a paragraph.

What this guarantees and what it does not are different things, and the difference is the point.
The boundary is enforced by code: no text can forge the fence. Compliance with the framing is a
request to a model, with nothing behind it.

## Rejected

- **Leave prompts as they are and rely on the agent's judgement** — because with no boundary the
  agent cannot tell hub's words from a log's even when it tries, so its judgement has nothing to
  act on. Reverses if investigations stop passing externally-authored text into prompts.
- **A fixed tag with no sanitising of the body** — because the body can then contain the closing
  tag, which fails in exactly the case the fence exists for. Reverses if every text source becomes
  one hub controls.
- **A tag carrying an unguessable id derived from the body** — because the collision case has no
  terminating definition: stripping the colliding id changes the body, which changes the id derived
  from it. The id also buys nothing, since it is chosen after the body is read and cannot be matched
  by it either way. Reverses if a fence needs an id for some reason other than unguessability.

## Risk

The enforced half is narrow and the requested half is the half that decides outcomes. A sufficiently
persuasive injection inside the fence, one claiming to be an operator override for instance, can
still be obeyed. This reduces how often a foreign instruction is followed. It does not prevent it.

Impact is untouched. The agent still runs with permissions skipped, so an injection that does
succeed succeeds completely. Restricting that is a separate decision this record does not make.

A log line legitimately containing the string `untrusted-input` has it removed.

The scope of the fence is what hub hands the agent, which is stated as
[an invariant](../invariants/foreign-text-reaches-an-agent-fenced.md) along with what enforces it.
Text the agent fetches for itself with `gh` or `Read` is outside it, because hub never touches that
text and cannot fence it.

## Revisit when

A fenced injection is observed being obeyed, or investigations stop running with permissions
skipped, which would change what the fence is protecting against.

## Also update

- [x] questions/README.md — no open question closes into this record.
- [x] vision.md — says nothing about prompt composition; nothing to change.
