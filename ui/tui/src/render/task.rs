use super::FOCUS_COLOR;
use crate::render::shared::popup_area;
use crate::state::{task_creation::TaskFormField, TaskCreationModal};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

pub(crate) fn render_task_creation_modal(
    frame: &mut ratatui::Frame,
    modal: &mut TaskCreationModal,
) {
    let popup = popup_area(frame.area(), 18, 62);
    frame.render_widget(Clear, popup);

    let [title_area, desc_area, kind_area, repo_area, link_area, submit_area] = modal_layout(popup);

    let focused = modal.focused_field;

    render_text_fields(frame, modal, focused, title_area, desc_area, link_area);
    render_kind_field(frame, modal, focused, kind_area);
    render_repo_field(frame, modal, focused, repo_area);
    render_submit_field(frame, modal, submit_area);
}

fn modal_layout(popup: ratatui::layout::Rect) -> [ratatui::layout::Rect; 6] {
    Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(popup)
}

fn border_style(focused: TaskFormField, field: TaskFormField) -> Style {
    if focused == field {
        Style::default().fg(FOCUS_COLOR)
    } else {
        Style::default()
    }
}

fn apply_cursor(
    ta: &mut tui_textarea::TextArea<'static>,
    focused: TaskFormField,
    field: TaskFormField,
) {
    if focused == field {
        ta.set_cursor_style(Style::default().bg(FOCUS_COLOR));
    } else {
        ta.set_cursor_style(Style::default());
    }
    ta.set_cursor_line_style(Style::default());
}

fn render_text_fields(
    frame: &mut ratatui::Frame,
    modal: &mut TaskCreationModal,
    focused: TaskFormField,
    title_area: ratatui::layout::Rect,
    desc_area: ratatui::layout::Rect,
    link_area: ratatui::layout::Rect,
) {
    apply_cursor(&mut modal.title, focused, TaskFormField::Title);
    modal.title.set_block(
        Block::new()
            .title(" Title ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style(focused, TaskFormField::Title)),
    );
    frame.render_widget(&modal.title, title_area);

    apply_cursor(&mut modal.description, focused, TaskFormField::Description);
    modal.description.set_block(
        Block::new()
            .title(" Description ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style(focused, TaskFormField::Description)),
    );
    frame.render_widget(&modal.description, desc_area);

    apply_cursor(&mut modal.link, focused, TaskFormField::Link);
    modal.link.set_block(
        Block::new()
            .title(" Link ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style(focused, TaskFormField::Link)),
    );
    frame.render_widget(&modal.link, link_area);
}

fn render_kind_field(
    frame: &mut ratatui::Frame,
    modal: &TaskCreationModal,
    focused: TaskFormField,
    kind_area: ratatui::layout::Rect,
) {
    let kind_name = match modal.kind {
        domain::TaskKind::Review => "Review",
        domain::TaskKind::Implement => "Implement",
        domain::TaskKind::Debug => "Debug",
    };
    frame.render_widget(
        Paragraph::new(kind_name).block(
            Block::new()
                .title(" Kind  [Space] cycle ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style(focused, TaskFormField::Kind)),
        ),
        kind_area,
    );
}

fn render_repo_field(
    frame: &mut ratatui::Frame,
    modal: &TaskCreationModal,
    focused: TaskFormField,
    repo_area: ratatui::layout::Rect,
) {
    let repo_display = modal
        .repo
        .selected_value()
        .map_or_else(|| "(none)".to_string(), std::string::ToString::to_string);
    let repo_title = if focused == TaskFormField::Repo && !modal.repo.input().is_empty() {
        format!(" Repo  {} ↑↓ ", modal.repo.input())
    } else {
        " Repo  [↑↓] type to filter ".to_string()
    };
    frame.render_widget(
        Paragraph::new(repo_display).block(
            Block::new()
                .title(repo_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style(focused, TaskFormField::Repo)),
        ),
        repo_area,
    );
}

fn render_submit_field(
    frame: &mut ratatui::Frame,
    modal: &TaskCreationModal,
    submit_area: ratatui::layout::Rect,
) {
    let focused_submit = modal.focused_field == TaskFormField::Submit;
    let submit_text_color = if focused_submit {
        FOCUS_COLOR
    } else {
        Color::White
    };
    let submit_block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(submit_text_color));
    let submit_inner = submit_block.inner(submit_area);
    frame.render_widget(submit_block, submit_area);
    frame.render_widget(
        Paragraph::new("SUBMIT")
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(submit_text_color)),
        submit_inner,
    );
}
