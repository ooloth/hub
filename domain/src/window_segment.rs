//! Turning external text into the segments of a window name, and the short
//! hashes that keep shortened segments apart.

use uuid::Uuid;

/// Length of the hash that tells two shortened segments apart. It separates a
/// small population — segments that share a prefix and are both over budget —
/// so four characters buys back legibility without risking a clash.
const TRUNCATION_HASH_LEN: usize = 4;

/// Substituted for a segment whose input sanitises to nothing, so that
/// legitimately punctuation-only external data cannot produce an empty name.
const EMPTY_SEGMENT: &str = "unnamed";

/// Namespace for the v5 UUIDs the short hashes are taken from. Any fixed UUID
/// works; this one is arbitrary and must never change, because changing it
/// renames every alert window and breaks the match in #331.
const HASH_NAMESPACE: Uuid = Uuid::from_bytes([
    0x1f, 0x9c, 0x4a, 0x2e, 0x7b, 0x60, 0x5d, 0x48, 0x9e, 0x11, 0xc3, 0x07, 0x5a, 0x8f, 0x2d, 0x64,
]);

/// Reduces external text to `[a-z0-9-]`, collapsing runs of separators.
///
/// Workflow names, project names and environment names come from GitHub, Loki
/// and `hub.toml`, where spaces, slashes and colons are all legitimate. They
/// are rewritten rather than rejected, and text that reduces to nothing gets a
/// placeholder, so no valid input can produce an empty name.
pub(crate) fn segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }

    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        EMPTY_SEGMENT.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Shortens a sanitised segment to `max`, ending it with a hash of the full
/// segment so that two long inputs sharing a prefix stay distinguishable.
///
/// Segments are ASCII by the time they reach here, so slicing by byte index
/// cannot split a character.
pub(crate) fn clamp(seg: &str, max: usize) -> String {
    assert!(
        max > TRUNCATION_HASH_LEN + 1,
        "segment budget {max} leaves no room for a hash"
    );
    if seg.len() <= max {
        return seg.to_string();
    }
    let keep = max - TRUNCATION_HASH_LEN - 1;
    // Trimmed so the kept text cannot end on the separator the hash adds.
    let prefix = seg[..keep].trim_end_matches('-');
    format!("{prefix}-{}", short_hash(seg, TRUNCATION_HASH_LEN))
}

/// Short, stable hash of arbitrary text, for disambiguating names that would
/// otherwise collide.
pub(crate) fn short_hash(raw: &str, len: usize) -> String {
    Uuid::new_v5(&HASH_NAMESPACE, raw.as_bytes())
        .simple()
        .to_string()
        .chars()
        .take(len)
        .collect()
}
