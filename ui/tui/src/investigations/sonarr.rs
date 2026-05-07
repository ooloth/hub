use super::LaunchConfig;

pub(crate) fn config(title: &str, error: &str) -> LaunchConfig {
    let safe_title = title.replace('\'', "'\\''");
    let safe_error = error.replace('\'', "'\\''");
    LaunchConfig {
        command: format!(
            "claude  --dangerously-skip-permissions '/sonarr-investigate {safe_title} -- {safe_error}'"
        ),
        env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn sonarr_investigation_command_passes_title_and_error() {
        assert_eq!(
            config("Show — S01E01", "Invalid video file").command,
            "claude  --dangerously-skip-permissions '/sonarr-investigate Show — S01E01 -- Invalid video file'"
        );
    }

    #[test]
    fn sonarr_investigation_command_escapes_single_quotes() {
        assert_eq!(
            config("It's Always Sunny", "Can't import").command,
            "claude  --dangerously-skip-permissions '/sonarr-investigate It'\\''s Always Sunny -- Can'\\''t import'"
        );
    }
}
