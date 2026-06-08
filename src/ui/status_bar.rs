use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render(f: &mut Frame, area: Rect, message: &str, building: bool) {
    let width = area.width as usize;
    let hint = "[Enter] Actions  [Esc/q/Ctrl-C] Quit  [/] Search";
    let build_suffix = if building { "  [b] Building..." } else { "" };
    let combined = format!("{}{}", message, build_suffix);

    let spans = vec![
        Span::styled(combined, Style::default()),
        Span::raw(" ".repeat(width.saturating_sub(message.len() + build_suffix.len() + hint.len()))),
        Span::styled(hint, Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)),
    ];

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, area);
}
