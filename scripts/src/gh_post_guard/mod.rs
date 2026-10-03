//! Refuses `gh` calls that would publish a term banned from hub.

/// Where the text a publishing call would post comes from.
mod body_source;
/// Reading the hook's request and phrasing its decision.
mod hook_io;
/// Which `gh` invocations publish text.
mod publishing_call;
/// The guard as a whole: request in, decision out.
mod respond;
/// Splitting a shell command into words.
mod shell_words;
/// Whether a call may run, and why not.
mod verdict;

pub(crate) use respond::run;
