use crate::display::{display_item_line, display_item_urgency, item_hint, DisplayItem};
use crate::state::{CategoryView, DataState};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders},
};

pub(super) fn hint_for_category_item(item: &DisplayItem) -> Option<String> {
    match item {
        DisplayItem::Group { .. } => Some("↩ to expand".to_string()),
        DisplayItem::Single(s) => item_hint(s),
    }
}

pub(super) fn render_category(
    frame: &mut ratatui::Frame,
    view: &mut CategoryView,
    data: &DataState,
    content_area: Rect,
) {
    let cat_items = data
        .cats
        .iter()
        .find(|c| c.cat == view.cat)
        .map(|c| c.items.as_slice())
        .unwrap_or(&[]);

    let title = Span::styled(
        format!(" {} ", view.cat.label()),
        Style::default()
            .fg(super::FOCUS_COLOR)
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::new()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(super::FOCUS_COLOR));
    let inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    super::render_list_view(
        frame,
        inner,
        cat_items,
        &mut view.list_state,
        |item| {
            let (line_text, dim_suffix) = match item {
                DisplayItem::Group { label, items } => {
                    (label.clone(), Some(format!(" ({})", items.len())))
                }
                DisplayItem::Single(_) => (display_item_line(item), None),
            };
            (line_text, dim_suffix, display_item_urgency(item))
        },
        hint_for_category_item,
    );
}

#[cfg(test)]
mod tests {
    use super::hint_for_category_item;
    use crate::display::DisplayItem;
    use workflows::status::StatusItem;

    fn ci() -> DisplayItem {
        DisplayItem::Single(StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            conclusion: "failure".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        }))
    }

    fn pr() -> DisplayItem {
        DisplayItem::Single(StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Fix".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
        }))
    }

    #[test]
    fn ci_item_hint_includes_investigate() {
        assert_eq!(
            hint_for_category_item(&ci()),
            Some("↩ to open · i to investigate".to_string())
        );
    }

    #[test]
    fn pr_item_hint_is_open_only() {
        assert_eq!(hint_for_category_item(&pr()), Some("↩ to open".to_string()));
    }

    #[test]
    fn group_item_hint_is_expand() {
        let group = DisplayItem::Group {
            label: "group".to_string(),
            items: vec![],
        };
        assert_eq!(
            hint_for_category_item(&group),
            Some("↩ to expand".to_string())
        );
    }
}
