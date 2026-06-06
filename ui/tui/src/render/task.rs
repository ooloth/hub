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

    let [title_area, desc_area, kind_area, repo_area, link_area, submit_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(popup);

    let focused = modal.focused_field;

    let border_style = |f: TaskFormField| {
        if focused == f {
            Style::default().fg(FOCUS_COLOR)
        } else {
            Style::default()
        }
    };

    // Apply cursor styling per-field: purple block on the focused field,
    // hidden on all others so only one cursor is visible at a time.
    let apply_cursor = |ta: &mut tui_textarea::TextArea<'static>, f: TaskFormField| {
        if focused == f {
            ta.set_cursor_style(Style::default().bg(FOCUS_COLOR));
        } else {
            ta.set_cursor_style(Style::default());
        }
        ta.set_cursor_line_style(Style::default());
    };

    apply_cursor(&mut modal.title, TaskFormField::Title);
    modal.title.set_block(
        Block::new()
            .title(" Title ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style(TaskFormField::Title)),
    );
    frame.render_widget(&modal.title, title_area);

    apply_cursor(&mut modal.description, TaskFormField::Description);
    modal.description.set_block(
        Block::new()
            .title(" Description ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style(TaskFormField::Description)),
    );
    frame.render_widget(&modal.description, desc_area);

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
                .border_style(border_style(TaskFormField::Kind)),
        ),
        kind_area,
    );

    let repo_display = modal
        .repo
        .selected_value()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "(none)".to_string());
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
                .border_style(border_style(TaskFormField::Repo)),
        ),
        repo_area,
    );

    apply_cursor(&mut modal.link, TaskFormField::Link);
    modal.link.set_block(
        Block::new()
            .title(" Link ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style(TaskFormField::Link)),
    );
    frame.render_widget(&modal.link, link_area);

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
