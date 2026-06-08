mod app;
mod build;
mod config;
mod dockerfile;
mod models;
mod remote;
mod ui;

use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;

use base64::Engine;
use crossterm::{
    cursor::Show,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

use crate::app::App;
use crate::config::load_config;
use crate::ui::{action_popup, log_viewer, search_bar as search_ui, server_picker, status_bar};

const INPUT_POLL_MS: u64 = 50;

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture, Show);
    }
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;

    let config_path = find_config()?;
    let config = load_config(&config_path)?;

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let _guard = TerminalGuard;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
        original_hook(info);
    }));

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = runtime.block_on(async {
        let mut app = App::new(config).await?;
        run(&mut terminal, &mut app).await
    });

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    )?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    while app.running {
        terminal.draw(|f| draw(f, app))?;

        if event::poll(std::time::Duration::from_millis(INPUT_POLL_MS))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.viewing_logs
                        && matches!(key.code, KeyCode::Char('s'))
                        && key.modifiers.is_empty()
                    {
                        enter_log_scrollback(
                            terminal,
                            &app.current_log_title,
                            &app.current_log_lines,
                        )?;
                        continue;
                    }
                    handle_key(app, key);
                }
                Event::Mouse(mouse) if app.viewing_logs => {
                    handle_log_mouse(app, mouse);
                }
                _ => {}
            }
        }

        app.handle_events();
    }

    Ok(())
}

fn handle_log_mouse(app: &mut App, mouse: MouseEvent) {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left)
        && mouse.kind != MouseEventKind::Drag(MouseButton::Left)
        && mouse.kind != MouseEventKind::Up(MouseButton::Left)
    {
        return;
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.handle_log_mouse_down(mouse.column, mouse.row);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.handle_log_mouse_drag(mouse.column, mouse.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.handle_log_mouse_up(mouse.column, mouse.row);
        }
        _ => {}
    }
}

fn enter_log_scrollback(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title: &str,
    lines: &[String],
) -> Result<()> {
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    )?;
    terminal.show_cursor()?;

    {
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();
        log_viewer::print_to_scrollback(&mut writer, title, lines)?;
    }

    let _ = event::read();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    Ok(())
}

fn draw(f: &mut Frame, app: &mut App) {
    if app.viewing_logs {
        app.log_area = f.area();
        let mut viewer = app.log_viewer.clone();
        log_viewer::render(
            &mut viewer,
            f,
            f.area(),
            &app.current_log_title,
            &app.current_log_lines,
            &app.log_selection,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    let title_line = format!("RKE2 Image Manager  [{} servers]", app.server_names.len());
    f.render_widget(
        ratatui::widgets::Paragraph::new(title_line)
            .style(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan)),
        chunks[0],
    );

    search_ui::render_search_bar(f, chunks[1], &app.search_query, app.search_active);

    let mut image_table = ui::image_table::ImageTable::new();
    image_table.render(
        f,
        chunks[2],
        &app.rows,
        &app.server_names,
        &app.server_scan_status,
        app.selected_row,
        app.search_active,
        &app.filtered_indices,
    );

    status_bar::render(f, chunks[3], &app.status_message, app.build_handle.is_some());

    if app.popup.is_visible() {
        match &app.popup {
            ui::action_popup::Popup::ServerPicker { .. } => {
                server_picker::render(f, f.area(), &app.popup, app.popup_selection);
            }
            _ => {
                action_popup::render_action_popup(f, f.area(), &app.popup, app.popup_selection);
            }
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if app.viewing_logs {
        // Ctrl+C copies the selection (or the whole log if nothing selected).
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            copy_log_to_clipboard(app);
            return;
        }
        match key.code {
            KeyCode::Esc => app.close_log_viewer(),
            KeyCode::Char('j') | KeyCode::Down => app.log_viewer.scroll(1, 20),
            KeyCode::Char('k') | KeyCode::Up => app.log_viewer.scroll(-1, 20),
            KeyCode::PageUp => app.log_viewer.page_up(20),
            KeyCode::PageDown => app.log_viewer.page_down(20, 20),
            KeyCode::Home => app.log_viewer.home(),
            KeyCode::End => app.log_viewer.end(20),
            KeyCode::Char('d') => app.dump_current_log(),
            _ => {}
        }
        return;
    }

    if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
        app.quit();
        return;
    }

    if app.search_active {
        match key.code {
            KeyCode::Esc => {
                app.search_active = false;
                app.search_clear();
            }
            KeyCode::Backspace => {
                app.search_pop();
            }
            KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
                app.search_push(c);
            }
            KeyCode::Enter => {
                app.search_active = false;
            }
            _ => {}
        }
        return;
    }

    if app.popup.is_visible() {
        match key.code {
            KeyCode::Esc => app.popup_cancel(),
            KeyCode::Enter => app.popup_confirm(),
            KeyCode::Char(' ') => app.popup_toggle(),
            KeyCode::Char('j') | KeyCode::Down => app.popup_move(1),
            KeyCode::Char('k') | KeyCode::Up => app.popup_move(-1),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') => app.quit(),
        KeyCode::Esc => app.quit(),
        KeyCode::Char('/') => {
            app.search_active = true;
            app.search_clear();
        }
        KeyCode::Enter => app.open_action_popup(),
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        _ => {}
    }
}

fn find_config() -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![PathBuf::from("config.toml")];
    if let Some(path) = dirs_lookup("config.toml") {
        candidates.push(path);
    }

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!(
        "config.toml not found. Copy config.example.toml to config.toml and edit it."
    ))
}

fn dirs_lookup(filename: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?;
    let exe_parent = exe_dir.parent()?;
    let path = exe_parent.join(filename);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Copy the log viewer's current selection (or the whole log if nothing is
/// selected) to the terminal's clipboard via the OSC 52 escape sequence.
/// Supported by kitty, WezTerm, iTerm2, recent gnome-terminal, and most
/// other modern terminal emulators.
fn copy_log_to_clipboard(app: &mut App) {
    let text = if app.log_selection.is_active() {
        app.log_selection.extract(&app.current_log_lines)
    } else {
        app.current_log_lines.join("\n")
    };

    if text.is_empty() {
        app.status_message = "Nothing to copy".to_string();
        return;
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let sequence = format!("\x1b]52;c;{}\x1b\\", encoded);

    match write_sequence(&sequence) {
        Ok(()) => {
            let kind = if app.log_selection.is_active() {
                "selection"
            } else {
                "log"
            };
            app.status_message = format!("Copied {} to clipboard ({} bytes)", kind, text.len());
        }
        Err(e) => {
            app.status_message = format!("Clipboard copy failed: {}", e);
        }
    }
}

fn write_sequence(sequence: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    stdout.write_all(sequence.as_bytes())?;
    stdout.flush()
}
