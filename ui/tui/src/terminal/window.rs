//! Claiming a window by name or label: finding it if it is open, or the right to open it, and what
//! the status bar says about where it is.

use super::detect::Terminal;
use super::rex::RexSession;
use super::{rex, tmux};
use anyhow::{Context as _, Result};
use domain::{InvestigationWindow, RexSessionId, WorkersSession};
use std::path::Path;

/// The right to open one window, given out only for a name the terminal did
/// not already have.
///
/// `open` is a method on this type and nothing else constructs it, so "open a
/// window without having looked for it" is not expressible, and neither is
/// "open a window somewhere other than where it was looked for": the place
/// `claim` searched travels inside the claim.
///
/// Vacant *when checked*. tmux and Rex both permit duplicate window names and
/// nothing stops one appearing in between, so this type forces the check rather
/// than proving the result still holds. For a single-user TUI that race is not
/// worth closing.
pub(crate) struct VacantWindow {
    target: Target,
    window: InvestigationWindow,
}

/// Where a vacant window will open.
enum Target {
    /// A new tmux window in the TUI's tmux session.
    Tmux,
    /// A new tab in the window's existing session.
    RexSession(RexSessionId),
    /// A new session labelled as the window's session, with this tab as its first.
    NewRexSession,
}

/// What became of a request for a window.
pub(crate) enum WindowClaim {
    /// The terminal already had the window.
    Reused(WindowPlace),
    /// The terminal did not have the window. Only this can be opened.
    Vacant(VacantWindow),
}

/// Where a window is once hub has found or opened it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowPlace {
    /// tmux has brought the window into view.
    InView,
    /// A new Rex tab opened in the background, in an existing session.
    OpenedIn(WorkersSession),
    /// A new Rex tab opened in the background, in a session created for it.
    OpenedInNew(WorkersSession),
    /// The Rex tab was already open, and is left where it is.
    AlreadyOpenIn(WorkersSession),
}

impl WindowPlace {
    /// What the status bar says, when the user cannot see the window.
    ///
    /// A Rex tab never comes into view, so without this a launch that worked
    /// would look the same as a keypress that did nothing.
    pub(crate) fn notice(&self) -> Option<String> {
        match self {
            Self::InView => None,
            Self::OpenedIn(session) => Some(format!("Opened in {session}")),
            Self::OpenedInNew(session) => Some(format!("Opened in {session}, a new session")),
            Self::AlreadyOpenIn(session) => Some(format!("Already open in {session}")),
        }
    }
}

/// What the status bar says after a launch, if anything.
///
/// A failure shows its whole chain, because the cause, such as `rex`'s stderr, sits below the
/// context that names what failed, and the status bar is the only place a launch error goes.
pub(crate) fn launch_notice(result: Result<WindowPlace>) -> Option<String> {
    match result {
        Ok(place) => place.notice(),
        Err(err) => Some(format!("{err:#}")),
    }
}

/// Finds `window` if the enclosing terminal already has it, or hands back the
/// right to open it.
///
/// In tmux a found window is brought into view. In Rex it is left as it is,
/// because hub can't bring a Rex tab into view without Remote Control
/// (decision 032), and it is looked for only in the window's session.
///
/// A `Reused` claim means the caller is done, and every step the caller would
/// otherwise take (fetching, creating a worktree, writing a prompt) is skipped.
///
/// `purpose` names what the caller was trying to do, so a missing terminal
/// stays actionable.
pub(crate) fn claim(window: &InvestigationWindow, purpose: &str) -> Result<WindowClaim> {
    let terminal = Terminal::detect(
        std::env::var("TMUX").ok().as_deref(),
        std::env::var("REX_SESSION").ok().as_deref(),
        purpose,
    )?;

    match terminal {
        Terminal::Tmux => match find_window(&tmux::open_windows()?, &window.to_string()) {
            Some(id) => {
                tmux::select_window(&id)?;
                Ok(WindowClaim::Reused(WindowPlace::InView))
            }
            None => Ok(WindowClaim::Vacant(VacantWindow {
                target: Target::Tmux,
                window: window.clone(),
            })),
        },
        Terminal::Rex => {
            let sessions = rex::sessions()
                .with_context(|| format!("couldn't look for {}", window.session()))?;
            claim_in_rex(window, &sessions, rex::open_windows)
        }
    }
}

/// The claim for `window` in Rex, given every session Rex has and a way to
/// list one session's windows.
///
/// Separate from `claim` so the choice between the three outcomes is tested
/// without a Rex server.
fn claim_in_rex(
    window: &InvestigationWindow,
    sessions: &[RexSession],
    windows_in: impl Fn(&RexSessionId) -> Result<Vec<OpenWindow>>,
) -> Result<WindowClaim> {
    let target = match find_session(sessions, window.session()) {
        None => Target::NewRexSession,
        Some(id) => match find_window(&windows_in(&id)?, window.label().as_str()) {
            Some(_) => {
                return Ok(WindowClaim::Reused(WindowPlace::AlreadyOpenIn(
                    window.session().clone(),
                )))
            }
            None => Target::RexSession(id),
        },
    };
    Ok(WindowClaim::Vacant(VacantWindow {
        target,
        window: window.clone(),
    }))
}

/// The first session labelled exactly as `wanted`.
///
/// The first, because the dotfiles launcher picks the first too, so both put
/// a repository's tabs in the same session if two ever share the label. Exact,
/// because `Workers (hub)` must not find `Workers (hub-private)`.
fn find_session(sessions: &[RexSession], wanted: &WorkersSession) -> Option<RexSessionId> {
    sessions
        .iter()
        .find(|session| session.label == wanted.label())
        .map(|session| session.id.clone())
}

impl VacantWindow {
    /// Opens the window and runs `command` in it, returning where it is.
    ///
    /// A tmux window becomes the one in view. A Rex tab opens in the
    /// background (decision 032). Taking `self` by value spends the claim, so
    /// one check cannot open two windows.
    pub(crate) fn open(
        self,
        cwd: Option<&Path>,
        env: &[(String, String)],
        command: &str,
    ) -> Result<WindowPlace> {
        assert!(!command.is_empty(), "window command must not be empty");

        let Self { target, window } = self;
        let label = window.label().as_str();
        let session = window.session().clone();
        match target {
            Target::Tmux => {
                tmux::new_window(&window.to_string(), cwd, env, command)?;
                Ok(WindowPlace::InView)
            }
            Target::RexSession(id) => {
                rex::new_window(&id, label, cwd, env, command)?;
                Ok(WindowPlace::OpenedIn(session))
            }
            Target::NewRexSession => {
                rex::new_session(session.label(), label, cwd, env, command)?;
                Ok(WindowPlace::OpenedInNew(session))
            }
        }
    }
}

/// A window the terminal currently has open.
pub(super) struct OpenWindow {
    /// The terminal's own handle for the window, like tmux's `@7` or Rex's
    /// `window:<uuid>`. Stable while the window lives and, unlike the name,
    /// safe to use as a target.
    pub(super) id: String,
    pub(super) name: String,
}

/// The id of the window called `name`, if the terminal has one: the tmux name in tmux, and the
/// tab's label in Rex (decision 033).
///
/// Comparison is on the whole name in both directions: a substring match would
/// let `hub:pr:33` find `hub:pr:330`, or the reverse, and hijacking another
/// investigation's window is worse than opening a second one.
fn find_window(open: &[OpenWindow], name: &str) -> Option<String> {
    open.iter()
        .find(|candidate| candidate.name == name)
        .map(|candidate| candidate.id.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        claim_in_rex, find_session, find_window, launch_notice, OpenWindow, Target, WindowClaim,
        WindowPlace,
    };
    use crate::terminal::rex::RexSession;
    use anyhow::Context as _;
    use domain::{
        GitHubLogin, InvestigationWindow, PrWindowActivity, RepoSlug, ReviewSkill, RexSessionId,
        WindowProject, WorkersSession,
    };
    use proptest::prelude::*;
    use rstest::rstest;

    fn hub() -> domain::ProjectShortName {
        short("hub")
    }

    fn on_pr(
        short: &domain::ProjectShortName,
        number: u64,
        activity: PrWindowActivity,
    ) -> InvestigationWindow {
        InvestigationWindow::pr(
            &WindowProject::new(short.clone(), repo_slug()),
            number,
            &GitHubLogin::parse("alice").unwrap(),
            activity,
        )
    }

    /// The tmux name of `short`'s investigation of pull request `number`.
    fn pr_name(short: &domain::ProjectShortName, number: u64) -> String {
        on_pr(short, number, PrWindowActivity::Investigate).to_string()
    }

    fn short(name: &str) -> domain::ProjectShortName {
        domain::ProjectShortName::parse(name).unwrap()
    }

    fn repo_slug() -> RepoSlug {
        "ooloth/hub".parse().unwrap()
    }

    fn sessions(labels: &[&str]) -> Vec<RexSession> {
        labels
            .iter()
            .enumerate()
            .map(|(i, label)| RexSession {
                id: RexSessionId::parse(&format!("session:{i}")).unwrap(),
                label: (*label).to_string(),
            })
            .collect()
    }

    fn hub_session() -> WorkersSession {
        WorkersSession::for_repo(&repo_slug())
    }

    /// Window ids are irrelevant to every test that only asks whether a name
    /// matched, so they are generated rather than spelled out.
    fn open(names: &[&str]) -> Vec<OpenWindow> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| OpenWindow {
                id: format!("@{i}"),
                name: (*name).to_string(),
            })
            .collect()
    }

    #[test]
    fn a_window_already_open_is_found_by_its_id() {
        let windows = open(&["nvim", "hub:pr:330", "shell"]);
        assert_eq!(
            find_window(&windows, &pr_name(&hub(), 330)),
            Some("@1".to_string())
        );
    }

    #[test]
    fn an_empty_window_list_reports_not_open() {
        assert_eq!(find_window(&[], &pr_name(&hub(), 330)), None);
    }

    #[test]
    fn an_unrelated_window_list_reports_not_open() {
        let windows = open(&["nvim", "claude.exe", "just-", "scripts:ci:lint"]);
        assert_eq!(find_window(&windows, &pr_name(&hub(), 330)), None);
    }

    /// The open window is longer or shorter than the one being looked for, so
    /// this fails against a substring match in either direction.
    #[rstest]
    #[case("hub:pr:33")]
    #[case("hub:pr:3300")]
    #[case("hub:pr:330:extra")]
    #[case("x-hub:pr:330")]
    fn a_neighbouring_window_name_is_not_a_match(#[case] open_name: &str) {
        let windows = open(&[open_name]);
        assert_eq!(find_window(&windows, &pr_name(&hub(), 330)), None);
    }

    /// A pull request's lazygit window shares its project and number with the
    /// investigation, so only the kind segment stops a lookup switching to it.
    #[test]
    fn a_pull_requests_lazygit_window_is_not_its_investigation_window() {
        let windows = open(&[&on_pr(&hub(), 330, PrWindowActivity::Lazygit).to_string()]);
        assert_eq!(find_window(&windows, &pr_name(&hub(), 330)), None);
    }

    /// The id is what the terminal is told to switch to, so the wrong one switches to the
    /// wrong window.
    #[test]
    fn the_id_returned_belongs_to_the_matching_window() {
        let windows = vec![
            OpenWindow {
                id: "@4".to_string(),
                name: "hub:pr:12".to_string(),
            },
            OpenWindow {
                id: "@9".to_string(),
                name: "hub:pr:330".to_string(),
            },
        ];
        assert_eq!(
            find_window(&windows, &pr_name(&hub(), 330)),
            Some("@9".to_string())
        );
    }

    proptest! {
        /// Whatever else the terminal happens to have open, the window hub is looking
        /// for is found when it is there.
        #[test]
        fn a_window_is_found_among_any_other_windows(
            repo in "[a-z]{1,8}(-[a-z]{1,4}){0,2}",
            number in 1u64..=100_000,
            others in prop::collection::vec("[a-z:0-9-]{0,20}", 0..6),
        ) {
            let window = pr_name(&short(&repo), number);
            let mut names: Vec<String> = others;
            names.push(window.clone());
            let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
            prop_assert!(find_window(&open(&borrowed), &window).is_some());
        }

        /// The generator is biased toward one repository and adjacent numbers,
        /// because names sharing a prefix are where a substring bug hides.
        #[test]
        fn a_list_holding_one_window_does_not_report_another(
            repo in "hub(-[a-z]{1,4})?",
            a in 1u64..=400,
            b in 1u64..=400,
        ) {
            prop_assume!(a != b);
            let present = pr_name(&short(&repo), a);
            prop_assert!(find_window(&open(&[&present]), &pr_name(&short(&repo), b)).is_none());
        }
    }

    /// Only the exact label is the repository's session: a longer repository name, a shorter
    /// one and the plain session each belong to other tabs.
    #[rstest]
    #[case("Workers (hub-private)")]
    #[case("Workers (hu)")]
    #[case("Workers (hub) 2")]
    #[case("Workers")]
    #[case("workers (hub)")]
    fn a_neighbouring_session_label_is_not_the_repositorys_session(#[case] label: &str) {
        assert_eq!(find_session(&sessions(&[label]), &hub_session()), None);
    }

    #[test]
    fn the_repositorys_session_is_found_among_others() {
        let found = find_session(
            &sessions(&["Signals", "Workers", "Workers (hub)"]),
            &hub_session(),
        );
        assert_eq!(
            found.map(|id| id.to_string()),
            Some("session:2".to_string())
        );
    }

    /// The dotfiles launcher takes the first of two sessions sharing a label, so hub does too
    /// and both put the repository's tabs in one of them.
    #[test]
    fn of_two_sessions_sharing_the_label_the_first_is_used() {
        let found = find_session(
            &sessions(&["Workers (hub)", "Workers (hub)"]),
            &hub_session(),
        );
        assert_eq!(
            found.map(|id| id.to_string()),
            Some("session:0".to_string())
        );
    }

    /// What a claim in Rex became, in a form a test can compare.
    #[derive(Debug, Eq, PartialEq)]
    enum Outcome {
        Reused(WindowPlace),
        InSession(String),
        NewSession,
    }

    /// The claim for `window` when Rex has `tabs`, each session's label beside the labels of
    /// the tabs open in it. The listing answers only for the session it is asked about, so a
    /// claim that looked in the wrong session would see that session's tabs.
    fn rex_outcome_for(window: &InvestigationWindow, tabs: &[(&str, Vec<&str>)]) -> Outcome {
        let labels: Vec<&str> = tabs.iter().map(|(label, _)| *label).collect();
        let all = sessions(&labels);
        let claim = claim_in_rex(window, &all, |id| {
            let index = all
                .iter()
                .position(|session| &session.id == id)
                .expect("the claim listed a session Rex does not have");
            Ok(open(&tabs[index].1))
        })
        .unwrap();
        match claim {
            WindowClaim::Reused(place) => Outcome::Reused(place),
            WindowClaim::Vacant(vacant) => match vacant.target {
                Target::RexSession(id) => Outcome::InSession(id.to_string()),
                Target::NewRexSession => Outcome::NewSession,
                Target::Tmux => panic!("a Rex claim targeted tmux"),
            },
        }
    }

    /// The claim for the investigation of PR 330, `PR 330 (alice): investigate`.
    fn rex_outcome(tabs: &[(&str, Vec<&str>)]) -> Outcome {
        rex_outcome_for(&on_pr(&hub(), 330, PrWindowActivity::Investigate), tabs)
    }

    /// The window is looked for only in its repository's session. The same name open in any
    /// other session, such as the TUI's own or one with no repository, is another tab, and a
    /// new one opens.
    #[rstest]
    #[case::open_in_its_session(
        vec![("Signals", vec![]), ("Workers (hub)", vec!["PR 330 (alice): investigate"])],
        Outcome::Reused(WindowPlace::AlreadyOpenIn(hub_session()))
    )]
    #[case::not_open_in_its_session(
        vec![("Signals", vec![]), ("Workers (hub)", vec!["PR 330 (alice): lazygit"])],
        Outcome::InSession("session:1".to_string())
    )]
    #[case::open_only_in_another_session(
        vec![("Signals", vec!["PR 330 (alice): investigate"]), ("Workers (hub)", vec![])],
        Outcome::InSession("session:1".to_string())
    )]
    #[case::no_session_for_its_repository(
        vec![("Signals", vec![]), ("Workers", vec!["PR 330 (alice): investigate"])],
        Outcome::NewSession
    )]
    #[case::open_under_its_tmux_name(
        vec![("Workers (hub)", vec!["hub:pr:330"])],
        Outcome::InSession("session:0".to_string())
    )]
    fn a_rex_claim_looks_only_in_the_repositorys_session(
        #[case] tabs: Vec<(&str, Vec<&str>)>,
        #[case] expected: Outcome,
    ) {
        assert_eq!(rex_outcome(&tabs), expected);
    }

    /// A review the dotfiles launcher opened carries the label hub builds for the same review,
    /// so hub reuses that tab rather than starting a second review.
    #[test]
    fn a_review_tab_the_launcher_opened_is_reused() {
        let review = on_pr(
            &hub(),
            330,
            PrWindowActivity::Review(ReviewSkill::ReviewCode),
        );
        assert_eq!(
            rex_outcome_for(
                &review,
                &[("Workers (hub)", vec!["PR 330 (alice): review-code"])]
            ),
            Outcome::Reused(WindowPlace::AlreadyOpenIn(hub_session()))
        );
    }

    /// Each review skill has its own tab, so an open investigation does not swallow a review.
    #[test]
    fn a_review_opens_beside_the_prs_open_investigation() {
        let review = on_pr(
            &hub(),
            330,
            PrWindowActivity::Review(ReviewSkill::ReviewConverge),
        );
        assert_eq!(
            rex_outcome_for(
                &review,
                &[(
                    "Workers (hub)",
                    vec!["PR 330 (alice): investigate", "PR 330 (alice): review-code"]
                )]
            ),
            Outcome::InSession("session:0".to_string())
        );
    }

    /// A Rex tab never comes into view, so the status bar is the only sign a launch worked.
    #[rstest]
    #[case(WindowPlace::InView, None)]
    #[case(WindowPlace::OpenedIn(hub_session()), Some("Opened in Workers (hub)"))]
    #[case(
        WindowPlace::OpenedInNew(hub_session()),
        Some("Opened in Workers (hub), a new session")
    )]
    #[case(
        WindowPlace::AlreadyOpenIn(hub_session()),
        Some("Already open in Workers (hub)")
    )]
    fn each_place_says_where_the_window_is(
        #[case] place: WindowPlace,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(place.notice().as_deref(), expected);
    }

    /// The status bar is the only record of a failed launch, so it carries `rex`'s own words
    /// as well as what hub was doing.
    #[test]
    fn a_failed_launch_shows_the_cause_beneath_its_context() {
        let failure: anyhow::Result<WindowPlace> = Err(anyhow::anyhow!(
            "exit status: 1: Error: no Rex server is running"
        ))
        .context("rex session new failed for Workers (hub)");
        assert_eq!(
            launch_notice(failure).as_deref(),
            Some("rex session new failed for Workers (hub): exit status: 1: Error: no Rex server is running")
        );
    }

    #[test]
    fn a_launch_in_view_says_nothing() {
        assert_eq!(launch_notice(Ok(WindowPlace::InView)), None);
    }
}
