use crate::state::{Action, App, Effect, Screen};

impl App {
    pub(super) fn handle_merging_pr(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelMerge => {
                let Screen::MergingPr { parent, .. } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                self.ui.screen = Screen::from_snapshot(parent);
                vec![]
            }
            Action::CommitMerge => {
                let Screen::MergingPr { pr, parent } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let repo = pr.repo.to_string();
                let number = pr.number;
                self.ui.screen = Screen::from_snapshot(parent);
                vec![Effect::MergePullRequest { repo, number }]
            }
            _ => unreachable!(),
        }
    }
}
