use std::io::IsTerminal;

/// Resolved output behavior for a CLI invocation, derived from the global
/// `--json`/`--quiet`/`--no-color` flags plus the environment.
pub struct OutputOptions {
    pub json: bool,
    pub quiet: bool,
    pub color: bool,
}

impl OutputOptions {
    pub fn new(json: bool, quiet: bool, no_color: bool) -> Self {
        let color = !json && !no_color && !env_no_color() && std::io::stdout().is_terminal();
        Self { json, quiet, color }
    }
}

fn env_no_color() -> bool {
    std::env::var("NO_COLOR")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Column-aligned table rendering: each column is padded to the widest
/// cell (header included), columns separated by two spaces, trailing
/// whitespace trimmed per line.
pub fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
    }

    let mut out = String::new();
    let header_cells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    out.push_str(format_row(&header_cells, &widths).trim_end());
    out.push('\n');
    for row in rows {
        out.push_str(format_row(row, &widths).trim_end());
        out.push('\n');
    }
    out
}

fn format_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let w = widths.get(i).copied().unwrap_or(c.len());
            format!("{:<width$}", c, width = w)
        })
        .collect::<Vec<_>>()
        .join("  ")
}

pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_table_aligns_columns() {
        let headers = ["NAME", "VERSION"];
        let rows = vec![
            vec!["adguardhome".to_string(), "v1".to_string()],
            vec!["a".to_string(), "v0.107.68-1.0.2".to_string()],
        ];
        let table = render_table(&headers, &rows);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines[0], "NAME         VERSION");
        assert_eq!(lines[1], "adguardhome  v1");
        assert_eq!(lines[2], "a            v0.107.68-1.0.2");
    }

    #[test]
    fn test_render_table_empty_rows() {
        let table = render_table(&["NAME"], &[]);
        assert_eq!(table, "NAME\n");
    }
}
