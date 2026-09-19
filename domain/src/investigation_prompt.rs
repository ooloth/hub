//! The prompt an investigation hands its agent, assembled from parts that say
//! where each part came from.
//!
//! A prompt used to be one `format!` mixing hub's instructions with a log
//! line, and an agent reading it had no way to tell which was which. Here the
//! two are different variants, and the only variant that accepts an
//! [`UntrustedText`] fences it.

use std::path::Path;

use crate::untrusted::UntrustedText;

/// The tag fencing externally-authored text, and the substring that must not
/// survive inside a fenced body.
///
/// One string does both jobs on purpose: if the body cannot contain
/// `untrusted-input` at all, it can form neither `<untrusted-input …>` nor
/// `</untrusted-input>`, so the fence cannot be opened or closed from inside.
const MARKER: &str = "untrusted-input";

/// One piece of an investigation prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptSegment {
    /// Text hub wrote.
    Instruction(String),
    /// Text hub did not write. Fenced on render, and labelled with where it
    /// came from so the agent knows what it is looking at.
    Untrusted {
        /// Human-readable provenance, e.g. "loki log message".
        source: String,
        /// The text itself.
        body: UntrustedText,
    },
    /// Stands in for the file `launch` writes supporting data to. The path
    /// does not exist when the prompt is built, so it arrives at render time.
    SupportingDataPath,
}

/// An investigation prompt, built one segment at a time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InvestigationPrompt(Vec<PromptSegment>);

impl InvestigationPrompt {
    /// An empty prompt. Rendering one produces an empty string, which is how
    /// an investigation opens with no task.
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// Appends text hub wrote.
    #[must_use]
    pub fn instruction(mut self, text: impl Into<String>) -> Self {
        self.0.push(PromptSegment::Instruction(text.into()));
        self
    }

    /// Appends text hub did not write, to be fenced on render.
    ///
    /// # Panics
    /// Panics if `source` contains a double quote, which would break out of
    /// the attribute it is rendered into. Sources are literals in hub's own
    /// code, so this is a contract on the caller, not on the log.
    #[must_use]
    pub fn untrusted(mut self, source: &str, body: &UntrustedText) -> Self {
        assert!(
            !source.contains('"'),
            "untrusted source label must not contain a double quote, got {source}"
        );
        self.0.push(PromptSegment::Untrusted {
            source: source.to_string(),
            body: body.clone(),
        });
        self
    }

    /// Appends the path of the supporting-data file.
    #[must_use]
    pub fn supporting_data_path(mut self) -> Self {
        self.0.push(PromptSegment::SupportingDataPath);
        self
    }

    /// Whether this prompt has no segments at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether any segment needs the supporting-data path at render time.
    #[must_use]
    pub fn references_supporting_data(&self) -> bool {
        self.0
            .iter()
            .any(|segment| matches!(segment, PromptSegment::SupportingDataPath))
    }

    /// Renders the prompt, one segment per line.
    ///
    /// # Panics
    /// Panics if a segment needs the supporting-data path and none was given.
    /// The caller checks the same contract before calling, so a disagreement
    /// about it surfaces as well as a violation of it.
    #[must_use]
    pub fn render(&self, supporting_data_path: Option<&Path>) -> String {
        if self.references_supporting_data() {
            assert!(
                supporting_data_path.is_some(),
                "prompt references the supporting-data path but none was given"
            );
        }

        self.0
            .iter()
            .map(|segment| match segment {
                PromptSegment::Instruction(text) => text.clone(),
                PromptSegment::Untrusted { source, body } => fence(source, body.expose()),
                // Unreachable when it matters: the assertion above has already
                // run for any prompt that reaches this arm with `None`.
                PromptSegment::SupportingDataPath => supporting_data_path
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Wraps externally-authored text so it cannot be mistaken for an instruction.
///
/// A prompt fences through [`InvestigationPrompt::untrusted`]. This is for the
/// other way hub hands an agent foreign text: a file it writes and then tells
/// the agent to read. Both go through the same fence so there is one definition
/// of what a fence is.
///
/// # Panics
/// Panics if `source` contains a double quote, for the reason given on
/// [`InvestigationPrompt::untrusted`].
#[must_use]
pub fn fenced(source: &str, body: &UntrustedText) -> String {
    assert!(
        !source.contains('"'),
        "untrusted source label must not contain a double quote, got {source}"
    );
    fence(source, body.expose())
}

/// Wraps externally-authored text so it cannot be mistaken for an instruction.
fn fence(source: &str, body: &str) -> String {
    let body = without_marker(body);
    assert!(
        !body.contains(MARKER),
        "fenced body still contains {MARKER}"
    );
    format!("<{MARKER} source=\"{source}\">\n{body}\n</{MARKER}>")
}

/// Removes every occurrence of [`MARKER`].
///
/// One pass is not enough. `String::replace` scans left to right and can leave
/// a fresh occurrence behind where the removed text straddled one, as in
/// `untruntrusted-inputusted-input`, which becomes the marker itself after a
/// single pass.
///
/// The loop is bounded by the input: every pass removes at least one marker's
/// worth of bytes, so it cannot run more than `len / MARKER.len()` times, plus
/// the pass that observes nothing is left.
fn without_marker(body: &str) -> String {
    let max_passes = body.len() / MARKER.len() + 1;
    let mut cleaned = body.to_string();
    let mut passes = 0;
    while cleaned.contains(MARKER) {
        cleaned = cleaned.replace(MARKER, "");
        passes += 1;
        assert!(
            passes <= max_passes,
            "removing {MARKER} took more than {max_passes} passes"
        );
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::{fence, InvestigationPrompt, MARKER};
    use crate::untrusted::UntrustedText;
    use proptest::prelude::*;
    use std::path::Path;

    fn untrusted(body: &str) -> UntrustedText {
        UntrustedText::new(body)
    }

    /// The text between the opening and closing fence lines.
    fn fenced_body(rendered: &str) -> String {
        let after_open = rendered
            .split_once('\n')
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        after_open
            .rsplit_once('\n')
            .map(|(body, _)| body.to_string())
            .unwrap_or_default()
    }

    /// Bodies built from the pieces a fence-escape attempt is made of, plus
    /// the straddling case that defeats a single removal pass. A generator of
    /// arbitrary text would almost never produce a marker and would pass
    /// against code that does no removal at all.
    fn marker_dense_body() -> impl Strategy<Value = String> {
        let fragment = prop_oneof![
            Just(MARKER.to_string()),
            Just(format!("</{MARKER}>")),
            Just(format!("<{MARKER} source=\"hub instruction\">")),
            Just("untruntrusted-inputusted-input".to_string()),
            Just("untrusted-".to_string()),
            Just("input".to_string()),
            Just("-".to_string()),
            "[a-z ]{0,12}",
        ];
        prop::collection::vec(fragment, 0..12).prop_map(|parts| parts.concat())
    }

    proptest! {
        /// The security property: whatever the log says, it cannot close the
        /// fence it is sitting in. Each case also exercises the postcondition
        /// inside `fence` and the bound inside `without_marker`.
        #[test]
        fn a_fenced_body_can_never_form_the_marker(body in marker_dense_body()) {
            let rendered = InvestigationPrompt::new()
                .untrusted("log line", &untrusted(&body))
                .render(None);
            prop_assert!(!fenced_body(&rendered).contains(MARKER));
        }
    }

    #[test]
    fn a_body_without_the_marker_is_passed_through_unchanged() {
        let rendered = fence("log line", "OOM killed in worker 3");
        assert_eq!(fenced_body(&rendered), "OOM killed in worker 3");
    }

    #[test]
    fn an_empty_body_still_produces_a_well_formed_fence() {
        let rendered = fence("log line", "");
        assert_eq!(
            rendered,
            format!("<{MARKER} source=\"log line\">\n\n</{MARKER}>")
        );
    }

    /// A single removal pass turns this into the marker, which is the case
    /// that makes the loop necessary.
    #[test]
    fn a_straddling_marker_does_not_survive_removal() {
        let rendered = fence("log line", "untruntrusted-inputusted-input");
        assert!(!fenced_body(&rendered).contains(MARKER));
    }

    #[test]
    fn the_source_label_names_where_the_text_came_from() {
        let rendered = fence("loki log message", "boom");
        assert!(rendered.starts_with(&format!("<{MARKER} source=\"loki log message\">")));
    }

    #[test]
    fn instructions_and_untrusted_text_render_as_separate_lines() {
        let rendered = InvestigationPrompt::new()
            .instruction("Investigate the error below.")
            .untrusted("loki log message", &untrusted("boom"))
            .instruction("Then open the Grafana URL.")
            .render(None);
        assert_eq!(
            rendered,
            format!(
                "Investigate the error below.\n<{MARKER} source=\"loki log message\">\nboom\n</{MARKER}>\nThen open the Grafana URL."
            )
        );
    }

    #[test]
    fn an_empty_prompt_renders_to_nothing() {
        assert!(InvestigationPrompt::new().render(None).is_empty());
        assert!(InvestigationPrompt::new().is_empty());
    }

    #[test]
    fn the_supporting_data_segment_becomes_the_path_it_is_given() {
        let rendered = InvestigationPrompt::new()
            .instruction("Log lines:")
            .supporting_data_path()
            .render(Some(Path::new("/tmp/hub-supporting-data-000.json")));
        assert_eq!(rendered, "Log lines:\n/tmp/hub-supporting-data-000.json");
    }

    #[test]
    #[should_panic(expected = "references the supporting-data path but none was given")]
    fn rendering_a_supporting_data_segment_without_a_path_is_a_bug() {
        let _ = InvestigationPrompt::new()
            .supporting_data_path()
            .render(None);
    }

    #[test]
    #[should_panic(expected = "must not contain a double quote")]
    fn a_source_label_that_would_break_out_of_its_attribute_is_a_bug() {
        let _ = InvestigationPrompt::new().untrusted("a\" onload=\"x", &untrusted("boom"));
    }
}
