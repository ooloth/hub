use crossterm::event::KeyCode;

use super::super::{
    task_creation::{cycle_task_kind, seed_from_item, TaskFormField},
    Action, App, Effect, TaskCreationModal,
};

pub(super) fn update(app: &mut App, action: Action) -> Vec<Effect> {
    match action {
        Action::OpenTaskCreationForm => {
            let seed = app
                .ui
                .screen
                .selected_item_for_seeding()
                .and_then(|item| seed_from_item(&item));
            let repos = app.ui.available_repos.clone();
            app.ui.modal = Some(TaskCreationModal::with_seed(seed, repos));
            vec![]
        }
        Action::OpenBlankTaskCreationForm => {
            let repos = app.ui.available_repos.clone();
            app.ui.modal = Some(TaskCreationModal::with_seed(None, repos));
            vec![]
        }
        Action::CancelTaskCreation => {
            app.ui.modal = None;
            vec![]
        }
        Action::FocusNextField => {
            if let Some(modal) = &mut app.ui.modal {
                modal.focused_field = modal.focused_field.next();
            }
            vec![]
        }
        Action::FocusPrevField => {
            if let Some(modal) = &mut app.ui.modal {
                modal.focused_field = modal.focused_field.prev();
            }
            vec![]
        }
        Action::CycleTaskKind => {
            if let Some(modal) = &mut app.ui.modal {
                modal.kind = cycle_task_kind(modal.kind);
            }
            vec![]
        }
        Action::ModalTextInput(key) => {
            if let Some(modal) = &mut app.ui.modal {
                match modal.focused_field {
                    TaskFormField::Title => {
                        let _ = modal.title.input(key);
                    }
                    TaskFormField::Description => {
                        let _ = modal.description.input(key);
                    }
                    TaskFormField::Link => {
                        let _ = modal.link.input(key);
                    }
                    TaskFormField::Repo => match key.code {
                        KeyCode::Char(c) => modal.repo.type_char(c),
                        KeyCode::Backspace => modal.repo.backspace(),
                        KeyCode::Up => modal.repo.move_up(),
                        KeyCode::Down => modal.repo.move_down(),
                        _ => {}
                    },
                    TaskFormField::Kind | TaskFormField::Submit => {}
                }
            }
            vec![]
        }
        Action::CommitTaskCreation => {
            let result = app
                .ui
                .modal
                .as_ref()
                .and_then(super::super::task_creation::TaskCreationModal::try_into_request);
            match result {
                None => {
                    if app.ui.modal.is_some() {
                        app.ui.flash = Some("Title required".to_string());
                    }
                    vec![]
                }
                Some(req) => {
                    app.ui.modal = None;
                    vec![Effect::CreateTask {
                        title: req.title.as_str().to_owned(),
                        description: req.description,
                        kind: req.kind,
                        links: req.links,
                        repo: req.repo,
                        origin: req.origin,
                    }]
                }
            }
        }
        _ => unreachable!("task_modal::update called with non-modal action"),
    }
}
