use ratatui::{
    layout::{self, Constraint, Direction, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

#[derive(Clone, PartialEq, Eq)]
pub enum Action {
    Build,
    Deploy,
    Remove,
    ViewLogs,
    DumpLogs,
    Cancel,
}

#[derive(Clone)]
pub struct PopupEntry {
    pub action: Action,
    pub dimmed: bool,
    pub label: String,
}

#[derive(Clone)]
pub enum Popup {
    Hidden,
    Actions {
        entries: Vec<PopupEntry>,
        title: String,
    },
    ServerPicker {
        selected: Vec<bool>,
        for_action: Action,
        row_index: usize,
        title: String,
        server_names: Vec<String>,
    },
}

impl Popup {
    pub fn is_visible(&self) -> bool {
        !matches!(self, Popup::Hidden)
    }
}

pub fn render_action_popup(f: &mut Frame, area: Rect, popup: &Popup, selected: usize) {
    if let Popup::Actions { entries, title } = popup {
        let popup_area = centered_rect(entries.len() as u16 + 2, 40, area);

        let items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = if i == selected {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else if entry.dimmed {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                };
                ListItem::new(Span::styled(entry.label.clone(), style))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title.as_str()));

        f.render_widget(Clear, popup_area);
        f.render_widget(list, popup_area);
    }
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
