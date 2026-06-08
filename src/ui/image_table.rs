use std::collections::BTreeMap;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Cell, Row, Table, TableState},
    Frame,
};

use crate::models::{BuildState, ImageTableRow, ScanStatus};

pub struct ImageTable {
    pub state: TableState,
}

impl Default for ImageTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageTable {
    pub fn new() -> Self {
        Self {
            state: TableState::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        f: &mut Frame,
        area: Rect,
        rows: &[ImageTableRow],
        server_names: &[String],
        scan_status: &BTreeMap<String, ScanStatus>,
        selected_row: usize,
        search_active: bool,
        filtered_indices: &[usize],
    ) {
        let visible_rows: Vec<&ImageTableRow> = if search_active && !filtered_indices.is_empty() {
            filtered_indices
                .iter()
                .filter_map(|&i| rows.get(i))
                .collect()
        } else {
            rows.iter().collect()
        };

        let header_cells: Vec<Cell> = std::iter::once(Cell::from(Span::styled(
            "Image",
            Style::default().fg(Color::Cyan),
        )))
        .chain(std::iter::once(Cell::from(Span::styled(
            "Local",
            Style::default().fg(Color::Cyan),
        ))))
        .chain(server_names.iter().map(|name| {
            Cell::from(Span::styled(
                name.clone(),
                Style::default().fg(Color::Cyan),
            ))
        }))
        .collect();

        let header = Row::new(header_cells).height(1);

        let table_rows: Vec<Row> = visible_rows
            .iter()
            .map(|row| self.build_row(row, server_names, scan_status))
            .collect();

        let table = Table::new(table_rows, constraints(server_names.len()))
            .header(header)
            .block(Block::default())
            .column_spacing(2)
            .row_highlight_style(Style::default().bg(Color::Rgb(70, 70, 90)).fg(Color::White));

        self.state.select(Some(selected_row));
        f.render_stateful_widget(table, area, &mut self.state);
    }

    fn build_row<'a>(
        &self,
        row: &'a ImageTableRow,
        server_names: &[String],
        scan_status: &BTreeMap<String, ScanStatus>,
    ) -> Row<'a> {
        match row {
            ImageTableRow::Current(image) => {
                let label = format!("{}  {}", image.name, image.version);
                let label = if matches!(image.build_state, BuildState::Building) {
                    Span::styled(format!("{}  ⏳", label), Style::default().fg(Color::Yellow))
                } else {
                    Span::styled(label, Style::default())
                };

                let mut cells: Vec<Cell> = vec![Cell::from(label)];
                let local_symbol = local_cell(image);
                cells.push(Cell::from(local_symbol));
                self.append_server_cells(&mut cells, &image.server_presence, server_names, scan_status);
                Row::new(cells)
            }
            ImageTableRow::Stale {
                family_name,
                version,
                servers,
            } => {
                let label = format!("  {}  {}", family_name, version);
                let label = Span::styled(label, Style::default().fg(Color::DarkGray));

                let mut cells: Vec<Cell> = vec![Cell::from(label)];
                cells.push(Cell::from(Span::styled("  —", Style::default().fg(Color::DarkGray))));
                self.append_server_cells_for_unknown(&mut cells, servers, server_names, scan_status);
                Row::new(cells)
            }
            ImageTableRow::Unknown(unknown) => {
                let label = format!("? {}", unknown.filename);
                let label = Span::styled(label, Style::default().fg(Color::DarkGray));

                let mut cells: Vec<Cell> = vec![Cell::from(label)];
                cells.push(Cell::from(Span::styled("  —", Style::default().fg(Color::DarkGray))));
                self.append_server_cells_for_unknown(&mut cells, &unknown.servers, server_names, scan_status);
                Row::new(cells)
            }
        }
    }

    fn append_server_cells(
        &self,
        cells: &mut Vec<Cell>,
        presence: &std::collections::BTreeMap<String, bool>,
        server_names: &[String],
        scan_status: &BTreeMap<String, ScanStatus>,
    ) {
        for name in server_names {
            let symbol = match presence.get(name).copied() {
                Some(true) => Span::styled("  ✓", Style::default().fg(Color::Green)),
                Some(false) => Span::styled("  ✗", Style::default().fg(Color::Red)),
                None => match scan_status.get(name) {
                    Some(ScanStatus::Ok) => Span::styled("  ✗", Style::default().fg(Color::Red)),
                    _ => Span::styled("  ?", Style::default().fg(Color::DarkGray)),
                },
            };
            cells.push(Cell::from(symbol));
        }
    }

    fn append_server_cells_for_unknown(
        &self,
        cells: &mut Vec<Cell>,
        servers_with_tarball: &std::collections::BTreeSet<String>,
        server_names: &[String],
        scan_status: &BTreeMap<String, ScanStatus>,
    ) {
        for name in server_names {
            if servers_with_tarball.contains(name) {
                cells.push(Cell::from(Span::styled(
                    "  ✓",
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                let symbol = match scan_status.get(name) {
                    Some(ScanStatus::Ok) => Span::styled("  ✗", Style::default().fg(Color::Red)),
                    _ => Span::styled("  ?", Style::default().fg(Color::DarkGray)),
                };
                cells.push(Cell::from(symbol));
            }
        }
    }
}

fn local_cell(image: &crate::models::ManagedImage) -> Span<'static> {
    if matches!(image.build_state, BuildState::Building) {
        Span::styled("  ⏳", Style::default().fg(Color::Yellow))
    } else if image.local_tarball {
        Span::styled("  ✓", Style::default().fg(Color::Green))
    } else {
        Span::styled("  ✗", Style::default().fg(Color::Red))
    }
}

fn constraints(server_count: usize) -> Vec<Constraint> {
    let mut v = vec![Constraint::Min(30), Constraint::Length(6)];
    for _ in 0..server_count {
        v.push(Constraint::Length(5));
    }
    v
}
