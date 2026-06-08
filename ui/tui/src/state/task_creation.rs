use domain::{RepoSlug, TaskKind, TaskOrigin};
use tui_textarea::TextArea;
use workflows::status::StatusItem;

/// Fuzzy repo picker: type to filter, Up/Down to select.
#[derive(Debug)]
pub(crate) struct RepoPicker {
    input: String,
    options: Vec<RepoSlug>,
    filtered: Vec<usize>,
    cursor: usize,
}

impl RepoPicker {
    pub(crate) fn new(options: Vec<RepoSlug>, preselect: Option<&RepoSlug>) -> Self {
        let input = preselect
            .map(|r| r.repo_name().to_string())
            .unwrap_or_default();
        let mut picker = Self {
            input,
            options,
            filtered: vec![],
            cursor: 0,
        };
        picker.refilter();
        if let Some(pre) = preselect {
            let target = pre.to_string();
            if let Some(pos) = picker
                .filtered
                .iter()
                .position(|&i| picker.options[i].to_string() == target)
            {
                picker.cursor = pos;
            }
        }
        picker
    }

    pub(crate) fn type_char(&mut self, c: char) {
        self.input.push(c);
        self.refilter();
    }

    pub(crate) fn backspace(&mut self) {
        self.input.pop();
        self.refilter();
    }

    fn refilter(&mut self) {
        let q = self.input.to_lowercase();
        self.filtered = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| q.is_empty() || o.to_string().to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        if self.cursor >= self.filtered.len() {
            self.cursor = self.filtered.len().saturating_sub(1);
        }
    }

    pub(crate) fn move_down(&mut self) {
        if !self.filtered.is_empty() {
            self.cursor = (self.cursor + 1).min(self.filtered.len() - 1);
        }
    }

    pub(crate) fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(crate) fn selected_value(&self) -> Option<&RepoSlug> {
        self.filtered
            .get(self.cursor)
            .and_then(|&i| self.options.get(i))
    }

    pub(crate) fn input(&self) -> &str {
        &self.input
    }

    #[cfg(test)]
    pub(crate) fn filtered_slugs(&self) -> impl Iterator<Item = &RepoSlug> {
        self.filtered.iter().filter_map(|&i| self.options.get(i))
    }

    #[cfg(test)]
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }
}

/// Proof that a task title is non-empty and trimmed.
/// Only constructable via `TaskTitle::parse()`.
pub(crate) struct TaskTitle(String);

impl TaskTitle {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(Self(t.to_owned()))
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated, ready-to-submit task creation payload.
/// Only reachable via `TaskCreationModal::try_into_request()`.
pub(crate) struct TaskCreationRequest {
    pub(crate) title: TaskTitle,
    pub(crate) description: Option<String>,
    pub(crate) kind: TaskKind,
    pub(crate) links: Vec<String>,
    pub(crate) repo: Option<RepoSlug>,
    pub(crate) origin: TaskOrigin,
}

/// Pre-population seed for the creation form.
pub(crate) struct TaskCreationSeed {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) kind: Option<TaskKind>,
    pub(crate) link: Option<String>,
    pub(crate) repo: Option<RepoSlug>,
    /// Provenance of the signal row this seed was derived from. Carried
    /// verbatim through the modal into the created task; not user-editable.
    pub(crate) origin: TaskOrigin,
}

/// Which field currently has focus in the creation modal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskFormField {
    Title,
    Description,
    Kind,
    Repo,
    Link,
    Submit,
}

impl TaskFormField {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Title => Self::Description,
            Self::Description => Self::Kind,
            Self::Kind => Self::Repo,
            Self::Repo => Self::Link,
            Self::Link => Self::Submit,
            Self::Submit => Self::Title,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Title => Self::Submit,
            Self::Description => Self::Title,
            Self::Kind => Self::Description,
            Self::Repo => Self::Kind,
            Self::Link => Self::Repo,
            Self::Submit => Self::Link,
        }
    }
}

/// State for the task creation modal overlay.
#[derive(Debug)]
pub(crate) struct TaskCreationModal {
    pub(crate) focused_field: TaskFormField,
    pub(crate) title: TextArea<'static>,
    pub(crate) description: TextArea<'static>,
    pub(crate) kind: TaskKind,
    pub(crate) repo: RepoPicker,
    pub(crate) link: TextArea<'static>,
    /// Provenance carried from the seed. `Idea` for a blank form. Not editable
    /// through any field — it rides alongside the form into the created task.
    pub(crate) origin: TaskOrigin,
}

impl TaskCreationModal {
    #[cfg(test)]
    pub(crate) fn blank() -> Self {
        Self::with_seed(None, vec![])
    }

    pub(crate) fn with_seed(
        seed: Option<TaskCreationSeed>,
        available_repos: Vec<RepoSlug>,
    ) -> Self {
        let mut title_ta = TextArea::default();
        let mut desc_ta = TextArea::default();
        let mut link_ta = TextArea::default();
        let mut kind = TaskKind::Implement;
        let mut seed_repo: Option<RepoSlug> = None;
        let mut origin = TaskOrigin::Idea;
        if let Some(s) = seed {
            if let Some(t) = s.title {
                title_ta = TextArea::new(vec![t]);
            }
            if let Some(d) = s.description {
                desc_ta = TextArea::new(d.lines().map(str::to_string).collect());
            }
            if let Some(k) = s.kind {
                kind = k;
            }
            if let Some(l) = s.link {
                link_ta = TextArea::new(vec![l]);
            }
            seed_repo = s.repo;
            origin = s.origin;
        }
        Self {
            focused_field: TaskFormField::Title,
            title: title_ta,
            description: desc_ta,
            kind,
            repo: RepoPicker::new(available_repos, seed_repo.as_ref()),
            link: link_ta,
            origin,
        }
    }

    /// Extract and validate modal fields. Returns None if title is blank.
    pub(crate) fn try_into_request(&self) -> Option<TaskCreationRequest> {
        let raw_title = self.title.lines().join("\n");
        let title = TaskTitle::parse(&raw_title)?;
        let raw_desc = self.description.lines().join("\n");
        let description = {
            let s = raw_desc.trim().to_owned();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };
        let raw_link = self.link.lines().join("\n");
        let links = {
            let s = raw_link.trim().to_owned();
            if s.is_empty() {
                vec![]
            } else {
                vec![s]
            }
        };
        Some(TaskCreationRequest {
            title,
            description,
            kind: self.kind,
            links,
            repo: self.repo.selected_value().cloned(),
            origin: self.origin.clone(),
        })
    }
}

/// Derives a creation seed from a selected signal row.
/// Returns `None` for task rows and media-backlog rows (opens a blank form).
pub(crate) fn seed_from_item(item: &StatusItem) -> Option<TaskCreationSeed> {
    match item {
        StatusItem::Pr(pr) => {
            let review_decision = match pr.review_decision {
                Some(domain::ReviewDecision::Approved) => match pr.approval_count {
                    1 => "approved (1)".to_string(),
                    n => format!("approved ({n})"),
                },
                Some(domain::ReviewDecision::ChangesRequested) => "changes requested".to_string(),
                None => "no reviews".to_string(),
            };
            Some(TaskCreationSeed {
                title: Some(pr.title.clone()),
                description: Some(format!(
                    "PR #{} · {}\nauthor: {}\nbranch: {} → {}\nstatus: {}",
                    pr.number, pr.repo, pr.author, pr.head_branch, pr.base_branch, review_decision
                )),
                kind: Some(TaskKind::Review),
                link: Some(pr.url.clone()),
                repo: Some(pr.repo.clone()),
                origin: TaskOrigin::from_pr(pr),
            })
        }
        StatusItem::Issue(issue) => Some(TaskCreationSeed {
            title: Some(issue.title.clone()),
            description: Some(format!(
                "Issue #{} · {}\nauthor: {}",
                issue.number, issue.repo, issue.author
            )),
            kind: Some(TaskKind::Implement),
            link: Some(issue.url.clone()),
            repo: Some(issue.repo.clone()),
            origin: TaskOrigin::from_issue(issue),
        }),
        StatusItem::Linear(l) => Some(TaskCreationSeed {
            title: Some(l.title.clone()),
            description: Some(format!("Linear {} · {}", l.identifier, l.state)),
            kind: Some(TaskKind::Implement),
            link: Some(l.url.clone()),
            repo: None,
            origin: TaskOrigin::from_linear(l),
        }),
        StatusItem::Ci(ci) => {
            let title = match (&ci.job_name, &ci.step_name) {
                (Some(job), Some(step)) => format!("{}: {} / {}", ci.repo, job, step),
                (Some(job), None) => format!("{}: {}", ci.repo, job),
                _ => format!("{}: CI failed", ci.repo),
            };
            let mut desc = format!("CI failure · {}\nworkflow: {}", ci.repo, ci.workflow_name);
            if let (Some(job), Some(step)) = (&ci.job_name, &ci.step_name) {
                desc.push_str(&format!("\njob: {} / {}", job, step));
            } else if let Some(job) = &ci.job_name {
                desc.push_str(&format!("\njob: {}", job));
            }
            if let Some(err) = &ci.error {
                desc.push_str(&format!("\nerror: {}", err));
            }
            Some(TaskCreationSeed {
                title: Some(title),
                description: Some(desc),
                kind: Some(TaskKind::Debug),
                link: Some(ci.url.clone()),
                repo: Some(ci.repo.clone()),
                origin: TaskOrigin::from_ci(ci),
            })
        }
        StatusItem::Loki(l) => Some(TaskCreationSeed {
            title: Some(format!("{}:{} — {}", l.project, l.env, l.title)),
            description: Some(format!(
                "Loki alert · {} ({})\nalert: {}\nmessage: {}\nline: {}\nlookback: {}",
                l.project, l.env, l.title, l.message, l.line, l.lookback
            )),
            kind: Some(TaskKind::Debug),
            link: (!l.url.is_empty()).then(|| l.url.clone()),
            repo: None,
            origin: TaskOrigin::from_loki(l),
        }),
        StatusItem::Gcp(g) => Some(TaskCreationSeed {
            title: Some(format!("{}:{} — {}", g.project, g.env, g.title)),
            description: Some(format!(
                "GCP alert · {} ({})\nalert: {}\nmessage: {}\nline: {}\nlookback: {}",
                g.project, g.env, g.title, g.message, g.line, g.lookback
            )),
            kind: Some(TaskKind::Debug),
            link: (!g.url.is_empty()).then(|| g.url.clone()),
            repo: None,
            origin: TaskOrigin::from_gcp(g),
        }),
        // Task rows open a blank form (no pre-population).
        StatusItem::AgentSession(_) => None,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => Some(TaskCreationSeed {
            title: Some(b.title.clone()),
            description: Some(format!("Import blocked · {}\nerror: {}", b.source, b.error)),
            kind: Some(TaskKind::Debug),
            link: Some(b.url.clone()),
            repo: None,
            // Media origins are constructed here (private-gated) but the
            // `TaskOrigin::Alert` type itself is feature-independent. `source`
            // is baked into the key so different media apps never collide.
            origin: TaskOrigin::Alert {
                source: domain::AlertSource::Media,
                key: format!("media/blocked/{}/{}", b.source, b.title),
                label: b.title.clone(),
            },
        }),
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => Some(TaskCreationSeed {
            title: Some(m.title.clone()),
            description: Some(format!("Missing · {}\naired: {}", m.source, m.air_date)),
            kind: Some(TaskKind::Debug),
            link: Some(m.url.clone()),
            repo: None,
            origin: TaskOrigin::Alert {
                source: domain::AlertSource::Media,
                key: format!("media/missing/{}/{}/{}", m.source, m.title, m.air_date),
                label: m.title.clone(),
            },
        }),
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => Some(TaskCreationSeed {
            title: Some(h.message.clone()),
            description: Some(format!("Health · {}", h.source)),
            kind: Some(TaskKind::Debug),
            link: (!h.url.is_empty()).then(|| h.url.clone()),
            repo: None,
            origin: TaskOrigin::Alert {
                source: domain::AlertSource::Media,
                key: format!("media/health/{}/{}", h.source, h.message),
                label: h.message.clone(),
            },
        }),
        // Backlog rows carry no actionable identity — blank form.
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => None,
    }
}

pub(crate) fn cycle_task_kind(kind: TaskKind) -> TaskKind {
    match kind {
        TaskKind::Review => TaskKind::Implement,
        TaskKind::Implement => TaskKind::Debug,
        TaskKind::Debug => TaskKind::Review,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TaskTitle ────────────────────────────────────────────────────────────

    #[test]
    fn task_title_parse_rejects_empty_string() {
        assert!(TaskTitle::parse("").is_none());
    }

    #[test]
    fn task_title_parse_rejects_whitespace_only() {
        assert!(TaskTitle::parse("   ").is_none());
    }

    #[test]
    fn task_title_parse_accepts_non_empty_string() {
        let t = TaskTitle::parse("Fix bug").unwrap();
        assert_eq!(t.as_str(), "Fix bug");
    }

    #[test]
    fn task_title_parse_trims_surrounding_whitespace() {
        let t = TaskTitle::parse("  Fix bug  ").unwrap();
        assert_eq!(t.as_str(), "Fix bug");
    }

    // ── TaskFormField cycling ────────────────────────────────────────────────

    #[test]
    fn tab_advances_through_all_six_fields_and_wraps() {
        let sequence = [
            TaskFormField::Description,
            TaskFormField::Kind,
            TaskFormField::Repo,
            TaskFormField::Link,
            TaskFormField::Submit,
            TaskFormField::Title,
        ];
        let mut field = TaskFormField::Title;
        for expected in sequence {
            field = field.next();
            assert_eq!(field, expected);
        }
    }

    #[test]
    fn shift_tab_retreats_through_all_six_fields_and_wraps() {
        let sequence = [
            TaskFormField::Submit,
            TaskFormField::Link,
            TaskFormField::Repo,
            TaskFormField::Kind,
            TaskFormField::Description,
            TaskFormField::Title,
        ];
        let mut field = TaskFormField::Title;
        for expected in sequence {
            field = field.prev();
            assert_eq!(field, expected);
        }
    }

    // ── RepoPicker ───────────────────────────────────────────────────────────

    fn slugs(ss: &[&str]) -> Vec<RepoSlug> {
        ss.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn repo_picker_shows_all_when_input_empty() {
        let picker = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), None);
        assert_eq!(picker.filtered_slugs().count(), 2);
    }

    #[test]
    fn repo_picker_filters_by_substring() {
        let picker = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), None);
        let mut p = picker;
        p.type_char('h');
        p.type_char('u');
        p.type_char('b');
        assert_eq!(p.filtered_slugs().count(), 1);
        assert_eq!(p.selected_value().unwrap().to_string(), "ooloth/hub");
    }

    #[test]
    fn repo_picker_backspace_restores_filter() {
        let mut p = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), None);
        p.type_char('u'); // 'u' only appears in "hub"
        assert_eq!(p.filtered_slugs().count(), 1);
        p.backspace();
        assert_eq!(p.filtered_slugs().count(), 2);
    }

    #[test]
    fn repo_picker_move_down_clamps_at_last() {
        let mut p = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), None);
        p.move_down();
        p.move_down();
        p.move_down();
        assert_eq!(p.cursor(), 1);
    }

    #[test]
    fn repo_picker_move_up_clamps_at_zero() {
        let mut p = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), None);
        p.move_up();
        assert_eq!(p.cursor(), 0);
    }

    #[test]
    fn repo_picker_preselect_positions_cursor() {
        let pre: RepoSlug = "org/other".parse().unwrap();
        let p = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), Some(&pre));
        assert_eq!(p.selected_value().unwrap().to_string(), "org/other");
    }

    #[test]
    fn repo_picker_selected_value_none_when_no_options() {
        let p = RepoPicker::new(vec![], None);
        assert!(p.selected_value().is_none());
    }

    #[test]
    fn repo_picker_cursor_resets_when_filter_shrinks_results() {
        let mut p = RepoPicker::new(slugs(&["ooloth/hub", "org/other"]), None);
        p.move_down(); // cursor=1
        p.type_char('u'); // 'u' only in "ooloth/hub" → filtered.len()=1, cursor clamped to 0
        assert_eq!(p.cursor(), 0);
    }

    // ── TaskCreationModal::try_into_request ──────────────────────────────────

    #[test]
    fn try_into_request_returns_none_when_title_is_empty() {
        let modal = TaskCreationModal::blank();
        assert!(modal.try_into_request().is_none());
    }

    #[test]
    fn try_into_request_returns_none_when_title_is_whitespace_only() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["   ".to_string()]);
        assert!(modal.try_into_request().is_none());
    }

    #[test]
    fn try_into_request_title_only_succeeds_with_correct_fields() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert_eq!(req.title.as_str(), "Fix bug");
        assert!(req.description.is_none());
        assert_eq!(req.kind, TaskKind::Implement);
        assert!(req.links.is_empty());
    }

    #[test]
    fn try_into_request_description_whitespace_becomes_none() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        modal.description = TextArea::new(vec!["   ".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert!(req.description.is_none());
    }

    #[test]
    fn try_into_request_includes_description_when_non_empty() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        modal.description = TextArea::new(vec!["Some details".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert_eq!(req.description.as_deref(), Some("Some details"));
    }

    #[test]
    fn try_into_request_link_empty_yields_no_links() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert!(req.links.is_empty());
    }

    #[test]
    fn try_into_request_link_non_empty_yields_one_element_vec() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        modal.link = TextArea::new(vec!["https://github.com/org/repo/issues/1".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert_eq!(req.links, vec!["https://github.com/org/repo/issues/1"]);
    }

    // ── cycle_task_kind ──────────────────────────────────────────────────────

    #[test]
    fn cycle_task_kind_review_becomes_implement() {
        assert_eq!(cycle_task_kind(TaskKind::Review), TaskKind::Implement);
    }

    #[test]
    fn cycle_task_kind_implement_becomes_debug() {
        assert_eq!(cycle_task_kind(TaskKind::Implement), TaskKind::Debug);
    }

    #[test]
    fn cycle_task_kind_debug_becomes_review() {
        assert_eq!(cycle_task_kind(TaskKind::Debug), TaskKind::Review);
    }

    // ── seed_from_item ───────────────────────────────────────────────────────

    fn make_pr(kind: domain::PrKind) -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 42,
            title: "Add dark mode".to_string(),
            repo: domain::RepoSlug::new("org", "hub"),
            url: "https://github.com/org/hub/pull/42".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            kind,
            author: "alice".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 0,
            head_branch: "feat/dark-mode".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        })
    }

    fn make_issue() -> StatusItem {
        StatusItem::Issue(domain::Issue {
            number: 7,
            title: "Button misaligned".to_string(),
            repo: domain::RepoSlug::new("org", "hub"),
            url: "https://github.com/org/hub/issues/7".to_string(),
            author: "bob".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec![],
            body: None,
        })
    }

    fn make_ci(job: Option<&str>, step: Option<&str>, error: Option<&str>) -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("org", "hub"),
            workflow_name: "CI".to_string(),
            job_name: job.map(str::to_string),
            step_name: step.map(str::to_string),
            error: error.map(str::to_string),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/org/hub/actions/runs/99".to_string(),
        })
    }

    fn make_loki(url: &str) -> StatusItem {
        StatusItem::Loki(domain::LokiEntry {
            title: "High error rate".to_string(),
            project: "myapp".to_string(),
            env: "prod".to_string(),
            message: "connection refused".to_string(),
            line: r#"{"level":"error"}"#.to_string(),
            lookback: "15m".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Critical,
            url: url.to_string(),
        })
    }

    #[test]
    fn seed_from_pr_to_review_gives_review_kind_and_pr_url() {
        let seed = seed_from_item(&make_pr(domain::PrKind::ToReview)).unwrap();
        assert_eq!(seed.kind, Some(TaskKind::Review));
        assert_eq!(seed.title.as_deref(), Some("Add dark mode"));
        assert_eq!(
            seed.link.as_deref(),
            Some("https://github.com/org/hub/pull/42")
        );
        assert!(seed.description.as_deref().unwrap().contains("org/hub"));
        assert!(seed.description.as_deref().unwrap().contains("alice"));
    }

    #[test]
    fn seed_from_own_pr_also_gives_review_kind() {
        let seed = seed_from_item(&make_pr(domain::PrKind::Mine)).unwrap();
        assert_eq!(seed.kind, Some(TaskKind::Review));
    }

    #[test]
    fn seed_from_issue_gives_implement_kind_and_issue_url() {
        let seed = seed_from_item(&make_issue()).unwrap();
        assert_eq!(seed.kind, Some(TaskKind::Implement));
        assert_eq!(seed.title.as_deref(), Some("Button misaligned"));
        assert_eq!(
            seed.link.as_deref(),
            Some("https://github.com/org/hub/issues/7")
        );
    }

    #[test]
    fn seed_from_ci_with_job_and_step_formats_title_and_debug_kind() {
        let seed =
            seed_from_item(&make_ci(Some("build"), Some("run tests"), Some("panicked"))).unwrap();
        assert_eq!(seed.kind, Some(TaskKind::Debug));
        assert_eq!(seed.title.as_deref(), Some("org/hub: build / run tests"));
        let desc = seed.description.unwrap();
        assert!(desc.contains("panicked"));
        assert_eq!(
            seed.link.as_deref(),
            Some("https://github.com/org/hub/actions/runs/99")
        );
    }

    #[test]
    fn seed_from_ci_without_job_falls_back_to_ci_failed_title() {
        let seed = seed_from_item(&make_ci(None, None, None)).unwrap();
        assert_eq!(seed.title.as_deref(), Some("org/hub: CI failed"));
    }

    #[test]
    fn seed_from_ci_without_error_omits_error_line_from_description() {
        let seed = seed_from_item(&make_ci(Some("build"), Some("test"), None)).unwrap();
        let desc = seed.description.unwrap();
        assert!(!desc.contains("error:"));
    }

    #[test]
    fn seed_from_loki_with_url_gives_link() {
        let seed = seed_from_item(&make_loki("https://grafana.example.com/d/123")).unwrap();
        assert_eq!(seed.kind, Some(TaskKind::Debug));
        assert_eq!(
            seed.link.as_deref(),
            Some("https://grafana.example.com/d/123")
        );
        let desc = seed.description.unwrap();
        assert!(desc.contains("connection refused"));
        assert!(desc.contains("15m"));
    }

    #[test]
    fn seed_from_loki_with_empty_url_gives_no_link() {
        let seed = seed_from_item(&make_loki("")).unwrap();
        assert!(seed.link.is_none());
    }

    #[test]
    fn seed_from_pr_populates_repo() {
        let seed = seed_from_item(&make_pr(domain::PrKind::ToReview)).unwrap();
        assert_eq!(seed.repo.unwrap().to_string(), "org/hub");
    }

    #[test]
    fn seed_from_issue_populates_repo() {
        let seed = seed_from_item(&make_issue()).unwrap();
        assert_eq!(seed.repo.unwrap().to_string(), "org/hub");
    }

    #[test]
    fn seed_from_ci_populates_repo() {
        let seed = seed_from_item(&make_ci(None, None, None)).unwrap();
        assert_eq!(seed.repo.unwrap().to_string(), "org/hub");
    }

    #[test]
    fn seed_from_loki_has_no_repo() {
        let seed = seed_from_item(&make_loki("https://example.com")).unwrap();
        assert!(seed.repo.is_none());
    }

    fn make_linear() -> StatusItem {
        StatusItem::Linear(domain::LinearIssue {
            identifier: "ENG-1".to_string(),
            title: "Investigate".to_string(),
            url: "https://linear.app/x".to_string(),
            state: "Todo".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
        })
    }

    fn make_gcp() -> StatusItem {
        StatusItem::Gcp(domain::GcpEntry {
            title: "High error rate".to_string(),
            project: "myapp".to_string(),
            env: "prod".to_string(),
            message: "oom killed".to_string(),
            line: "{}".to_string(),
            lookback: "15m".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            url: "https://console.cloud.google.com/x".to_string(),
            gcp_project: "rp006-prod-49a893d8".to_string(),
        })
    }

    // ── seed_from_item: origin ────────────────────────────────────────────────

    #[test]
    fn seed_from_pr_sets_pr_origin() {
        let seed = seed_from_item(&make_pr(domain::PrKind::ToReview)).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Pr {
                repo: domain::RepoSlug::new("org", "hub"),
                number: 42,
            }
        );
    }

    #[test]
    fn seed_from_issue_sets_github_issue_origin() {
        let seed = seed_from_item(&make_issue()).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Issue {
                system: domain::IssueSystem::GitHub,
                repo: Some(domain::RepoSlug::new("org", "hub")),
                id: "7".into(),
            }
        );
    }

    #[test]
    fn seed_from_linear_sets_linear_issue_origin() {
        let seed = seed_from_item(&make_linear()).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Issue {
                system: domain::IssueSystem::Linear,
                repo: None,
                id: "ENG-1".into(),
            }
        );
    }

    #[test]
    fn seed_from_ci_sets_ci_origin_with_identity_fields() {
        let seed = seed_from_item(&make_ci(Some("build"), Some("test"), None)).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Ci {
                repo: domain::RepoSlug::new("org", "hub"),
                workflow: "CI".into(),
                job: Some("build".into()),
                step: Some("test".into()),
                url: "https://github.com/org/hub/actions/runs/99".into(),
            }
        );
    }

    #[test]
    fn seed_from_loki_sets_loki_alert_origin() {
        let seed = seed_from_item(&make_loki("https://grafana/x")).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Alert {
                source: domain::AlertSource::Loki,
                key: "myapp/prod/connection refused".into(),
                label: "myapp:prod — High error rate".into(),
            }
        );
    }

    #[test]
    fn seed_from_gcp_sets_gcp_alert_origin() {
        let seed = seed_from_item(&make_gcp()).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Alert {
                source: domain::AlertSource::Gcp,
                key: "myapp/prod/oom killed".into(),
                label: "myapp:prod — High error rate".into(),
            }
        );
    }

    #[test]
    fn blank_form_produces_idea_origin() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["a title".to_string()]);
        assert_eq!(
            modal.try_into_request().unwrap().origin,
            domain::TaskOrigin::Idea
        );
    }

    #[test]
    fn seeded_origin_survives_the_form_boundary() {
        let seed = seed_from_item(&make_pr(domain::PrKind::ToReview)).unwrap();
        let expected = seed.origin.clone();
        let modal = TaskCreationModal::with_seed(Some(seed), vec![]);
        assert_eq!(modal.try_into_request().unwrap().origin, expected);
    }

    #[test]
    fn seed_from_agent_session_returns_none() {
        let task = StatusItem::AgentSession(domain::Task {
            id: "TASK-0001".parse().unwrap(),
            title: "some task".to_string(),
            description: None,
            status: domain::TaskStatus::InProgress,
            kind: TaskKind::Debug,
            session_id: None,
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            comments: vec![],
        });
        assert!(seed_from_item(&task).is_none());
    }

    #[cfg(feature = "private")]
    #[test]
    fn seed_from_media_blocked_sets_media_alert_origin_with_source_in_key() {
        let item = StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
            source: "tv".to_string(),
            urgency: domain::Urgency::Low,
            age: chrono::Duration::zero(),
            title: "Show — S01E01".to_string(),
            error: "unsupported extension".to_string(),
            url: "https://media/x".to_string(),
        });
        let seed = seed_from_item(&item).unwrap();
        assert_eq!(
            seed.origin,
            domain::TaskOrigin::Alert {
                source: domain::AlertSource::Media,
                key: "media/blocked/tv/Show — S01E01".into(),
                label: "Show — S01E01".into(),
            }
        );
    }
}
