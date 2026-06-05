use crate::display::flatten;
use crate::state::{Action, App, Effect, PrPrevScreen, Screen};

impl App {
    pub(super) fn handle_merging_pr(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::CancelMerge => {
                let Screen::MergingPr { prev, .. } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let PrPrevScreen::UnifiedList { snapshot } = prev;
                self.ui.screen = Screen::UnifiedList {
                    flat_rows: flatten(&snapshot.items, &snapshot.expanded_groups),
                    items: snapshot.items,
                    selected: snapshot.selected,
                    filter: snapshot.filter,
                    expanded_groups: snapshot.expanded_groups,
                    detail_mode: snapshot.detail_mode,
                };
                vec![]
            }
            Action::CommitMerge => {
                let Screen::MergingPr { pr, prev, .. } = std::mem::take(&mut self.ui.screen) else {
                    return vec![];
                };
                let repo = pr.repo.to_string();
                let number = pr.number;
                let PrPrevScreen::UnifiedList { snapshot } = prev;
                self.ui.screen = Screen::UnifiedList {
                    flat_rows: flatten(&snapshot.items, &snapshot.expanded_groups),
                    items: snapshot.items,
                    selected: snapshot.selected,
                    filter: snapshot.filter,
                    expanded_groups: snapshot.expanded_groups,
                    detail_mode: snapshot.detail_mode,
                };
                vec![Effect::MergePullRequest { repo, number }]
            }
            _ => unreachable!(),
        }
    }
}
