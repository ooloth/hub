use domain::TaskKind;
use tui_textarea::TextArea;

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
    pub(crate) issue_links: Vec<String>,
}

/// Pre-population seed for the creation form (S6 wired later).
pub(crate) struct TaskCreationSeed {
    pub(crate) title: Option<String>,
    pub(crate) issue_link: Option<String>,
}

/// Which field currently has focus in the creation modal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskFormField {
    Title,
    Description,
    Kind,
    IssueLink,
    Submit,
}

impl TaskFormField {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Title => Self::Description,
            Self::Description => Self::Kind,
            Self::Kind => Self::IssueLink,
            Self::IssueLink => Self::Submit,
            Self::Submit => Self::Title,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Title => Self::Submit,
            Self::Description => Self::Title,
            Self::Kind => Self::Description,
            Self::IssueLink => Self::Kind,
            Self::Submit => Self::IssueLink,
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
    pub(crate) issue_link: TextArea<'static>,
}

impl TaskCreationModal {
    pub(crate) fn blank() -> Self {
        Self::with_seed(None)
    }

    pub(crate) fn with_seed(seed: Option<TaskCreationSeed>) -> Self {
        let mut title_ta = TextArea::default();
        let mut issue_link_ta = TextArea::default();
        if let Some(s) = seed {
            if let Some(t) = s.title {
                title_ta = TextArea::new(vec![t]);
            }
            if let Some(l) = s.issue_link {
                issue_link_ta = TextArea::new(vec![l]);
            }
        }
        Self {
            focused_field: TaskFormField::Title,
            title: title_ta,
            description: TextArea::default(),
            kind: TaskKind::General,
            issue_link: issue_link_ta,
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
        let raw_link = self.issue_link.lines().join("\n");
        let issue_links = {
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
            issue_links,
        })
    }
}

pub(crate) fn cycle_task_kind(kind: TaskKind) -> TaskKind {
    match kind {
        TaskKind::General => TaskKind::Implement,
        TaskKind::Implement => TaskKind::Debug,
        TaskKind::Debug => TaskKind::General,
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
    fn tab_advances_through_all_five_fields_and_wraps() {
        let sequence = [
            TaskFormField::Description,
            TaskFormField::Kind,
            TaskFormField::IssueLink,
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
    fn shift_tab_retreats_through_all_five_fields_and_wraps() {
        let sequence = [
            TaskFormField::Submit,
            TaskFormField::IssueLink,
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
        assert_eq!(req.kind, TaskKind::General);
        assert!(req.issue_links.is_empty());
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
    fn try_into_request_issue_link_empty_yields_no_links() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert!(req.issue_links.is_empty());
    }

    #[test]
    fn try_into_request_issue_link_non_empty_yields_one_element_vec() {
        let mut modal = TaskCreationModal::blank();
        modal.title = TextArea::new(vec!["Fix bug".to_string()]);
        modal.issue_link = TextArea::new(vec!["https://github.com/org/repo/issues/1".to_string()]);
        let req = modal.try_into_request().unwrap();
        assert_eq!(
            req.issue_links,
            vec!["https://github.com/org/repo/issues/1"]
        );
    }

    // ── cycle_task_kind ──────────────────────────────────────────────────────

    #[test]
    fn cycle_task_kind_general_becomes_implement() {
        assert_eq!(cycle_task_kind(TaskKind::General), TaskKind::Implement);
    }

    #[test]
    fn cycle_task_kind_implement_becomes_debug() {
        assert_eq!(cycle_task_kind(TaskKind::Implement), TaskKind::Debug);
    }

    #[test]
    fn cycle_task_kind_debug_becomes_general() {
        assert_eq!(cycle_task_kind(TaskKind::Debug), TaskKind::General);
    }
}
