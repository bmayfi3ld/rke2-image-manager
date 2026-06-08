use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_search_bar(f: &mut Frame, area: Rect, query: &str, active: bool) {
    let style = if active {
        Style::default().fg(Color::White).bg(Color::Rgb(30, 30, 40))
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let display = if query.is_empty() {
        "Search: ".to_string()
    } else {
        format!("Search: {}", query)
    };

    let block = if active {
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan))
    } else {
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray))
    };

    let paragraph = Paragraph::new(display).block(block).style(style);
    f.render_widget(paragraph, area);
}