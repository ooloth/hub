use crate::repo_root::repo_root;

/// What a test recipe runs first, so that nothing a git hook exports reaches the tests.
///
/// A hook runs with git's own variables set, such as `GIT_DIR` when the push comes from a
/// worktree. Tests that build throwaway repositories run `git` there, and with `GIT_DIR`
/// inherited those commands write to the real repository instead: on 2026-10-28 a pre-push run
/// set `core.bare`, `core.hooksPath` and a test user in hub's shared config.
const CLEAR_GIT_ENV: &str = "unset $(git rev-parse --local-env-vars)";

/// The body of the recipe called `name` in `justfile`: its indented lines.
fn recipe_body(justfile: &str, name: &str) -> Option<String> {
    let mut lines = justfile.lines().skip_while(|line| {
        line.split(':')
            .next()
            .and_then(|head| head.split_whitespace().next())
            != Some(name)
    });
    let _recipe_line = lines.next()?;
    Some(
        lines
            .take_while(|line| line.starts_with(' ') || line.starts_with('\t'))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[test]
fn every_test_recipe_clears_the_git_environment_before_running_the_tests() {
    let justfile = std::fs::read_to_string(repo_root().join("justfile")).unwrap();

    for recipe in ["test", "test-update"] {
        let body = recipe_body(&justfile, recipe)
            .unwrap_or_else(|| panic!("the justfile has no `{recipe}` recipe"));
        assert!(
            body.trim_start().starts_with(CLEAR_GIT_ENV),
            "`just {recipe}` does not start with `{CLEAR_GIT_ENV}`, so a git hook's GIT_DIR \
             reaches the tests and their throwaway repositories write to this one:\n{body}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recipes_body_is_its_indented_lines() {
        let justfile =
            "build:\n    cargo build\n\ntest: _require-nextest\n    a\n    b\n\nother:\n    c\n";

        assert_eq!(
            recipe_body(justfile, "test"),
            Some("    a\n    b".to_string())
        );
        assert_eq!(recipe_body(justfile, "missing"), None);
    }
}
