use std::io::Write;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

const FOOTER_HINTS: &str =
    " Drag to select  [Ctrl+C] Copy  [s] Scrollback  [Esc] Close  [d] Dump  [↑/↓/PgUp/PgDn] Scroll";

#[derive(Clone)]
pub struct LogViewer {
    pub scroll_offset: usize,
    pub total_lines: usize,
}

impl Default for LogViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogViewer {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            total_lines: 0,
        }
    }

    pub fn scroll(&mut self, delta: i32, visible_height: usize) {
        let max_offset = self.total_lines.saturating_sub(visible_height);
        let raw = (self.scroll_offset as i64) + (delta as i64);
        let new_offset = raw.clamp(0, max_offset as i64) as usize;
        self.scroll_offset = new_offset;
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
    }

    pub fn page_down(&mut self, page_size: usize, visible_height: usize) {
        let max_offset = self.total_lines.saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + page_size).min(max_offset);
    }

    pub fn home(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn end(&mut self, visible_height: usize) {
        self.scroll_offset = self.total_lines.saturating_sub(visible_height);
    }
}

/// A mouse-driven text selection in the log viewer.
///
/// Coordinates are (row, col) in the logical line space (i.e., indices into
/// the `lines` array and character indices within a line, not screen cells).
/// The `anchor` is where the mouse was pressed; the `cursor` follows the drag.
/// On release the selection stays put so the user can press Ctrl+C to copy.
#[derive(Clone, Debug, Default)]
pub struct LogSelection {
    pub anchor: Option<(usize, usize)>,
    pub cursor: Option<(usize, usize)>,
}

impl LogSelection {
    pub fn start(&mut self, row: usize, col: usize) {
        self.anchor = Some((row, col));
        self.cursor = Some((row, col));
    }

    pub fn update(&mut self, row: usize, col: usize) {
        if self.anchor.is_some() {
            self.cursor = Some((row, col));
        }
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.cursor = None;
    }

    pub fn is_active(&self) -> bool {
        match (self.anchor, self.cursor) {
            (Some(a), Some(c)) => a != c,
            _ => false,
        }
    }

    /// Returns the selection range as (start, end) in row-major order.
    pub fn range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (a, c) = (self.anchor?, self.cursor?);
        if a.0 < c.0 || (a.0 == c.0 && a.1 <= c.1) {
            Some((a, c))
        } else {
            Some((c, a))
        }
    }

    /// Column range to highlight on a given row, or `None` if the row is outside
    /// the selection. `usize::MAX` on either side means "to the end of the line"
    /// — the caller is expected to clamp to the actual line length.
    pub fn cols_for_row(&self, row: usize) -> Option<(usize, usize)> {
        let ((r1, c1), (r2, c2)) = self.range()?;
        if row < r1 || row > r2 {
            return None;
        }
        if r1 == r2 {
            Some((c1, c2))
        } else if row == r1 {
            Some((c1, usize::MAX))
        } else if row == r2 {
            Some((0, c2))
        } else {
            Some((0, usize::MAX))
        }
    }

    /// Extract the selected text from the given lines. Lines are joined with
    /// `\n`; trailing line halves are preserved (no trailing newline appended).
    pub fn extract(&self, lines: &[String]) -> String {
        let Some(((r1, c1), (r2, c2))) = self.range() else {
            return String::new();
        };
        let mut out = String::new();
        for row in r1..=r2 {
            let Some(line) = lines.get(row) else { continue };
            let chars: Vec<char> = line.chars().collect();
            let len = chars.len();
            let (cs, ce) = if r1 == r2 {
                (c1.min(len), c2.min(len))
            } else if row == r1 {
                (c1.min(len), len)
            } else if row == r2 {
                (0, c2.min(len))
            } else {
                (0, len)
            };
            if cs >= ce {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.extend(chars[cs..ce].iter());
        }
        out
    }
}

pub fn render(
    viewer: &mut LogViewer,
    f: &mut Frame,
    area: Rect,
    title: &str,
    lines: &[String],
    selection: &LogSelection,
) {
    viewer.total_lines = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize;

    if viewer.scroll_offset + visible_height > viewer.total_lines {
        if viewer.total_lines > visible_height {
            viewer.scroll_offset = viewer.total_lines - visible_height;
        } else {
            viewer.scroll_offset = 0;
        }
    }

    let display_lines: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(viewer.scroll_offset)
        .take(visible_height)
        .map(|(row_idx, line)| {
            let is_error = line.to_lowercase().contains("error");
            let base_style = if is_error {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            let highlight_style = base_style.add_modifier(Modifier::REVERSED);

            build_line_with_selection(line, row_idx, selection, base_style, highlight_style)
        })
        .collect();

    let paragraph = Paragraph::new(display_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_bottom(FOOTER_HINTS),
        )
        .scroll((viewer.scroll_offset as u16, 0));

    f.render_widget(paragraph, area);
}

fn build_line_with_selection(
    line: &str,
    row_idx: usize,
    selection: &LogSelection,
    base_style: Style,
    highlight_style: Style,
) -> Line<'static> {
    let sel_cols = match selection.cols_for_row(row_idx) {
        Some(c) => c,
        None => return Line::from(Span::styled(line.to_string(), base_style)),
    };

    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let (cs_raw, ce_raw) = sel_cols;
    let cs = cs_raw.min(len);
    let ce = ce_raw.min(len);

    if cs >= ce || len == 0 {
        return Line::from(Span::styled(line.to_string(), base_style));
    }

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(3);
    if cs > 0 {
        spans.push(Span::styled(chars[..cs].iter().collect::<String>(), base_style));
    }
    spans.push(Span::styled(
        chars[cs..ce].iter().collect::<String>(),
        highlight_style,
    ));
    if ce < len {
        spans.push(Span::styled(chars[ce..].iter().collect::<String>(), base_style));
    }
    Line::from(spans)
}

pub fn print_to_scrollback<W: Write>(
    writer: &mut W,
    title: &str,
    lines: &[String],
) -> std::io::Result<()> {
    writeln!(writer, "\n=== {} ===", title)?;
    for line in lines {
        writeln!(writer, "{}", line)?;
    }
    writeln!(
        writer,
        "\n--- Use your terminal's text selection to copy. Press any key to return ---"
    )?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<String> {
        vec![
            "STEP 1/5: FROM ubuntu:20.04".to_string(),
            "STEP 2/5: RUN apt-get update".to_string(),
            "STEP 3/5: COPY app.sh /".to_string(),
            "STEP 4/5: ENTRYPOINT [\"/app.sh\"]".to_string(),
            "STEP 5/5: CMD [\"start\"]".to_string(),
        ]
    }

    #[test]
    fn test_selection_inactive_until_drag() {
        let mut sel = LogSelection::default();
        assert!(!sel.is_active());
        sel.start(0, 0);
        assert!(!sel.is_active());
        sel.update(0, 5);
        assert!(sel.is_active());
    }

    #[test]
    fn test_selection_normalizes_reversed_drag() {
        let mut sel = LogSelection::default();
        sel.start(2, 10);
        sel.update(0, 3);
        let ((r1, c1), (r2, c2)) = sel.range().unwrap();
        assert_eq!((r1, c1), (0, 3));
        assert_eq!((r2, c2), (2, 10));
    }

    #[test]
    fn test_selection_cols_for_single_row() {
        let mut sel = LogSelection::default();
        sel.start(1, 5);
        sel.update(1, 10);
        assert_eq!(sel.cols_for_row(1), Some((5, 10)));
        assert_eq!(sel.cols_for_row(0), None);
        assert_eq!(sel.cols_for_row(2), None);
    }

    #[test]
    fn test_selection_cols_for_multi_row() {
        let mut sel = LogSelection::default();
        sel.start(0, 4);
        sel.update(3, 8);
        // First row: from col 4 to end of line
        assert_eq!(sel.cols_for_row(0), Some((4, usize::MAX)));
        // Middle row: full line
        assert_eq!(sel.cols_for_row(1), Some((0, usize::MAX)));
        assert_eq!(sel.cols_for_row(2), Some((0, usize::MAX)));
        // Last row: from col 0 to col 8
        assert_eq!(sel.cols_for_row(3), Some((0, 8)));
        // Outside
        assert_eq!(sel.cols_for_row(4), None);
    }

    #[test]
    fn test_selection_extract_single_line() {
        let mut sel = LogSelection::default();
        sel.start(0, 5);
        sel.update(0, 10);
        // Chars 5..10 of "STEP 1/5: FROM ubuntu:20.04" is "1/5: "
        let extracted = sel.extract(&lines());
        assert_eq!(extracted, "1/5: ");
    }

    #[test]
    fn test_selection_extract_multi_line() {
        let mut sel = LogSelection::default();
        sel.start(0, 13); // 'M' in "FROM"
        sel.update(2, 4); // just past 'P' in "STEP" of row 2
        let extracted = sel.extract(&lines());
        assert_eq!(extracted, "M ubuntu:20.04\nSTEP 2/5: RUN apt-get update\nSTEP");
    }

    #[test]
    fn test_selection_extract_clamps_to_line_length() {
        let mut sel = LogSelection::default();
        sel.start(0, 0);
        sel.update(0, 1000); // past end of line
        let extracted = sel.extract(&lines());
        assert_eq!(extracted, "STEP 1/5: FROM ubuntu:20.04");
    }

    #[test]
    fn test_selection_extract_empty_when_inactive() {
        let sel = LogSelection::default();
        assert_eq!(sel.extract(&lines()), "");
    }
}
