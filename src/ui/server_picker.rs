use ratatui::{
    layout::{self, Constraint, Direction, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use crate::ui::action_popup::{Action, Popup};

pub fn render(f: &mut Frame, area: Rect, popup: &Popup, selected: usize) {
    let Popup::ServerPicker {
        selected: checks,
        for_action,
        title,
        server_names,
        ..
    } = popup
    else {
        return;
    };

    // Items: N checkboxes + [Continue] + [Cancel]. Plus 2 rows for the block's top/bottom borders.
    let height = (checks.len() + 2 + 2) as u16;
    let popup_area = centered_rect(height, 44, area);

    let mut items: Vec<ListItem> = checks
        .iter()
        .enumerate()
        .map(|(i, checked)| {
            let marker = if *checked { "[x]" } else { "[ ]" };
            let already = match for_action {
                Action::Deploy if !*checked => "  (already present)",
                _ => "",
            };
            let name = server_names.get(i).map(|s| s.as_str()).unwrap_or("?");
            let label = format!("{} {}{}", marker, name, already);
            let style = if i == selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };
            ListItem::new(Span::styled(label, style))
        })
        .collect();

    let on_confirm = selected == checks.len();
    let confirm_label = if on_confirm {
        Span::styled(
            "> [Continue]",
            Style::default().fg(Color::Black).bg(Color::White),
        )
    } else {
        Span::styled("  [Continue]", Style::default())
    };
    let cancel_label = if selected == checks.len() + 1 {
        Span::styled(
            "> [Cancel]",
            Style::default().fg(Color::Black).bg(Color::White),
        )
    } else {
        Span::styled("  [Cancel]", Style::default())
    };
    items.push(ListItem::new(confirm_label));
    items.push(ListItem::new(cancel_label));

    let block = Block::default().borders(Borders::ALL).title(title.as_str());
    let list = List::new(items).block(block);

    f.render_widget(Clear, popup_area);
    f.render_widget(list, popup_area);
}

fn centered_rect(height: u16, width: u16, r: Rect) -> Rect {
    let popup_layout = layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Length((r.height.saturating_sub(height)) / 2),
        ])
        .split(r);

    layout::Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Length((r.width.saturating_sub(width)) / 2),
        ])
        .split(popup_layout[1])[1]
}
