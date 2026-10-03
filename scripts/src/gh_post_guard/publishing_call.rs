use super::shell_words::{ShellWords, Word};

/// The kinds of `gh` call that publish text. Each reads its body from different flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishKind {
    /// `issue` or `pr`: create, comment, edit, review, close, merge, reopen.
    IssueOrPr,
    /// `release create` or `release edit`.
    Release,
    /// `gist create`. The files it is given are the content.
    Gist,
    /// `gist edit`. Its positionals name the gist, so only files added with `--add` are content.
    GistEdit,
    /// `repo create` or `repo edit`.
    Repo,
    /// `label create` or `label edit`.
    Label,
    /// `api` with a method other than GET, or with fields or an input body.
    Api,
}

/// One `gh` invocation that would publish text, with the words that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishingCall {
    pub(crate) kind: PublishKind,
    /// Every word after the subcommand, up to the next shell operator.
    pub(crate) args: Vec<Word>,
}

impl PublishingCall {
    /// Every publishing `gh` call in the command. Reads are never returned.
    pub(crate) fn find_all(words: &ShellWords) -> Vec<Self> {
        let mut calls = Vec::new();
        let mut rest = words.words.as_slice();
        while let Some(start) = rest.iter().position(is_gh) {
            let after_gh = rest.get(start + 1..).unwrap_or_default();
            let end = after_gh
                .iter()
                .position(is_operator)
                .unwrap_or(after_gh.len());
            let (invocation, remaining) = after_gh.split_at(end);
            calls.extend(classify(invocation));
            rest = remaining;
        }
        calls
    }
}

fn is_gh(word: &Word) -> bool {
    word.text == "gh" || word.text.ends_with("/gh")
}

fn is_operator(word: &Word) -> bool {
    !word.quoted
        && matches!(
            word.text.as_str(),
            "&&" | "||" | "|" | ";" | "&" | "(" | ")"
        )
}

/// The publishing call `gh <invocation>` makes, if it makes one.
fn classify(invocation: &[Word]) -> Option<PublishingCall> {
    let group = invocation.first()?.text.as_str();
    if group == "api" {
        let args = invocation.get(1..).unwrap_or_default().to_vec();
        return api_publishes(&args).then_some(PublishingCall {
            kind: PublishKind::Api,
            args,
        });
    }
    let action = invocation.get(1)?.text.as_str();
    let kind = match (group, action) {
        (
            "issue" | "pr",
            "create" | "comment" | "edit" | "review" | "close" | "merge" | "reopen",
        ) => PublishKind::IssueOrPr,
        ("release", "create" | "edit") => PublishKind::Release,
        ("gist", "create") => PublishKind::Gist,
        ("gist", "edit") => PublishKind::GistEdit,
        ("repo", "create" | "edit") => PublishKind::Repo,
        ("label", "create" | "edit") => PublishKind::Label,
        _ => return None,
    };
    Some(PublishingCall {
        kind,
        args: invocation.get(2..).unwrap_or_default().to_vec(),
    })
}

/// Whether `gh api` with these arguments sends anything. An explicit method decides;
/// otherwise `gh api` sends a POST exactly when it is given fields or an input body.
fn api_publishes(args: &[Word]) -> bool {
    let mut method: Option<String> = None;
    let mut has_body = false;
    let mut words = args.iter().map(|word| word.text.as_str());
    while let Some(arg) = words.next() {
        if arg == "-X" || arg == "--method" {
            method = words.next().map(str::to_string);
        } else if let Some(value) = arg.strip_prefix("--method=") {
            method = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("-X") {
            method = Some(value.to_string());
        } else if is_api_body_flag(arg) {
            has_body = true;
        }
    }
    method.map_or(has_body, |method| !method.eq_ignore_ascii_case("GET"))
}

fn is_api_body_flag(arg: &str) -> bool {
    matches!(arg, "-f" | "-F" | "--field" | "--raw-field" | "--input")
        || ["--field=", "--raw-field=", "--input="]
            .iter()
            .any(|prefix| arg.starts_with(prefix))
        || (arg.len() > 2 && (arg.starts_with("-f") || arg.starts_with("-F")))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn kinds(command: &str) -> Vec<PublishKind> {
        PublishingCall::find_all(&ShellWords::split(command).unwrap())
            .into_iter()
            .map(|call| call.kind)
            .collect()
    }

    #[rstest]
    #[case::issue_comment("gh issue comment 1 --body-file body.md", PublishKind::IssueOrPr)]
    #[case::pr_create("gh pr create -t title -b body", PublishKind::IssueOrPr)]
    #[case::issue_edit("gh issue edit 3 -b body", PublishKind::IssueOrPr)]
    #[case::pr_review("gh pr review 1 --comment -b body", PublishKind::IssueOrPr)]
    #[case::issue_close("gh issue close 1 --comment done", PublishKind::IssueOrPr)]
    #[case::pr_merge("gh pr merge 1 --body text", PublishKind::IssueOrPr)]
    #[case::release_create("gh release create v1 -n notes", PublishKind::Release)]
    #[case::gist_create("gh gist create notes.md", PublishKind::Gist)]
    #[case::repo_edit("gh repo edit --description text", PublishKind::Repo)]
    #[case::label_create("gh label create name", PublishKind::Label)]
    #[case::api_field("gh api repos/o/r/issues/1/comments -f body=text", PublishKind::Api)]
    #[case::api_typed_field("gh api repos/o/r/issues/1/comments -F body=@b.md", PublishKind::Api)]
    #[case::api_patch("gh api -X PATCH repos/o/r/issues/comments/1", PublishKind::Api)]
    #[case::api_joined_method("gh api -XPOST repos/o/r/issues", PublishKind::Api)]
    #[case::api_long_method("gh api --method=POST repos/o/r/issues", PublishKind::Api)]
    #[case::api_input("gh api repos/o/r/issues --input body.json", PublishKind::Api)]
    #[case::after_an_operator("cd x && gh issue comment 1 -b body", PublishKind::IssueOrPr)]
    #[case::by_path("/opt/homebrew/bin/gh issue comment 1 -b body", PublishKind::IssueOrPr)]
    fn a_publishing_call_is_found(#[case] command: &str, #[case] kind: PublishKind) {
        assert_eq!(kinds(command), vec![kind]);
    }

    #[rstest]
    #[case::issue_view("gh issue view 1 --comments")]
    #[case::issue_list("gh issue list --state all")]
    #[case::search("gh search issues alpha --repo o/r")]
    #[case::pr_diff("gh pr diff 1")]
    #[case::api_get("gh api repos/o/r")]
    #[case::api_explicit_get("gh api -X GET search/issues -f q=alpha")]
    #[case::gh_inside_a_quoted_message("git commit -m 'gh issue comment'")]
    #[case::not_gh("cargo build")]
    #[case::in_a_comment("echo hi # gh issue comment 1 -b x")]
    fn a_read_is_not_a_publishing_call(#[case] command: &str) {
        assert!(kinds(command).is_empty());
    }

    #[test]
    fn a_call_stops_at_the_next_shell_operator() {
        let calls = PublishingCall::find_all(
            &ShellWords::split("gh issue comment 1 -b body && echo done").unwrap(),
        );

        assert_eq!(calls.len(), 1);
        let texts: Vec<&str> = calls
            .first()
            .unwrap()
            .args
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        assert_eq!(texts, vec!["1", "-b", "body"]);
    }

    #[test]
    fn a_quoted_operator_is_text_not_the_end_of_the_call() {
        let calls = PublishingCall::find_all(
            &ShellWords::split("gh issue comment 1 -b ';' --body-file b.md").unwrap(),
        );

        let texts: Vec<&str> = calls
            .first()
            .unwrap()
            .args
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        assert_eq!(texts, vec!["1", "-b", ";", "--body-file", "b.md"]);
    }

    #[test]
    fn two_calls_in_one_command_are_both_found() {
        assert_eq!(
            kinds("gh issue comment 1 -b a; gh pr comment 2 -b b"),
            vec![PublishKind::IssueOrPr, PublishKind::IssueOrPr]
        );
    }
}
