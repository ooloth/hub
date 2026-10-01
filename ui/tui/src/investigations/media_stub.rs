//! Stands in for hub-private's `media.rs` on devices without it. The signature must
//! match the real module's, which `just lint` checks by compiling this file.

use anyhow::{bail, Result};
use domain::UntrustedText;
use secrecy::Secret;
use std::collections::HashMap;

use super::LaunchConfig;

pub(crate) fn config(
    _title: &UntrustedText,
    _error: &UntrustedText,
    _credentials: &HashMap<String, Secret<String>>,
) -> Result<LaunchConfig> {
    bail!("media investigation is not available on this device")
}

#[cfg(test)]
mod tests {
    use super::config;
    use domain::UntrustedText;
    use std::collections::HashMap;

    #[test]
    fn media_investigation_reports_it_is_unavailable_on_this_device() {
        let text = UntrustedText::new("any");
        let Err(err) = config(&text, &text, &HashMap::new()) else {
            panic!("the stub must never build a launch config");
        };
        assert_eq!(
            err.to_string(),
            "media investigation is not available on this device"
        );
    }
}
