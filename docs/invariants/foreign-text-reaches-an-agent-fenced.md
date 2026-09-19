# Text hub did not write reaches an agent fenced

## The invariant

Every piece of text hub hands to an investigation agent that hub did not author arrives inside an
`<untrusted-input source="…">` fence, and no such text can produce the fence marker itself.

This covers what hub puts in front of the agent: the task prompt, and any file hub writes and then
tells the agent to read. It does not cover what the agent goes and fetches with its own tools.
Hub cannot fence a `gh pr view` the agent runs for itself, and claiming otherwise would make this
unenforceable on the day it was written.

## Why it must hold

An investigation prompt is assembled from two kinds of text and delivered as one string. Hub's
instructions and a stranger's words arrive in the same channel, and the agent's only way to tell
them apart is how they are marked.

The everyday failure is not an attack. Log lines, CI output and vendor error strings are full of
imperatives: run this migration, clear that cache, try this flag. An agent that cannot separate
those from its own instructions acts on them, and acting on them is usually wrong even when nobody
intended anything.

The marker rule is what makes the fence mean something. A fence a log line can close is a fence
that fails precisely when someone tries it, which is worse than no fence, because the surrounding
code and [Decision 023](../decisions/023-investigation-prompts-fence-foreign-text.md) both read as
though the boundary holds.

What this does **not** promise: that the agent obeys the framing. The boundary is enforced;
compliance is requested. 023's Risk section is the honest statement of that limit, and it is a
limit of the protection rather than an exception to this invariant.

## What it forbids

- Interpolating a value that came from outside hub into an `Instruction` segment, or into any
  string that becomes one.
- Writing foreign text to a file the prompt points the agent at without fencing the file's
  contents. The file is the same channel as the prompt, one indirection later, and it usually
  carries more foreign text than the prompt does.
- Adding a signal type whose prompt carries foreign text without giving that text its own
  `Untrusted` segment, which is the case a new investigation type walks into.
- Reaching for `UntrustedText::expose` to build prompt text. Exposure has honest uses, listed
  below; producing prompt text is not one of them.

What it permits, which is easy to confuse with the above: exposing foreign text to render a list
row, to hash it into a tmux window name, or to parse a value out of it such as a log's timestamp.
None of those put the text in front of an agent.

## How it is enforced

**The marker rule is enforced totally, in one place.** `domain::investigation_prompt` strips every
occurrence of `untrusted-input` from a body before wrapping it, in a loop bounded by the body
length, and asserts afterwards that none survived. A `proptest` case drives it with bodies built
from marker fragments, including the straddling form that defeats a single removal pass. Both the
prompt path and the file path call the same function, so there is one definition of what a fence is.

**Reaching the fence at all is enforced by the type.** Foreign text is `UntrustedText`, which has no
`Display`, no `Deref` and no `AsRef<str>`, so it cannot be interpolated. A prompt is an
`InvestigationPrompt` of segments, and `untrusted` is the only constructor that accepts an
`UntrustedText`. A new signal type cannot put foreign text in a prompt without choosing that
variant.

**The escape hatch is governed by a test, not a type.** `every_exposure_of_untrusted_text_says_why`
in `ui/tui/src/investigations/command.rs` walks the prompt-building modules and fails on any
`.expose()` without a `// expose:` justification in the comment block above it.

What that test misses: it reads source text, not meaning. It cannot tell a truthful justification
from a careless one, and a justification comment satisfies it whatever it says. It also exempts
`launch.rs` and `command.rs`, where exposure is the point, so a prompt assembled inside either of
those would not be caught. It is a tripwire against the reflexive `.expose()` rather than a proof.

What nothing checks: that a value arriving from a new external source is wrapped in `UntrustedText`
at its boundary in the first place. A field left as `String` in a client or workflow is invisible to
all of the above, because there is nothing to fence.
