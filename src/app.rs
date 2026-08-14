use anyhow::Result;
use ratatui::layout::Rect;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::dockerfile::discover_image_families;
use crate::inventory;
use crate::models::{Config, ImageFamily, ImageTableRow, ScanStatus, UnknownTarball};
use crate::remote::{self, tarball_filename, RemoteEvent};
use crate::ui::action_popup::{Action, Popup, PopupEntry};
use crate::ui::log_viewer::{LogSelection, LogViewer};
use crate::build;

const MAX_LOG_LINES: usize = 10_000;

pub struct Searcher {
    nucleo: nucleo::Nucleo<RowInfo>,
    pattern: String,
    pub visible_indices: Vec<usize>,
}

impl Searcher {
    pub fn new(rows: &[RowInfo]) -> Self {
        let mut s = Self::empty_matcher();
        s.replace_items(rows);
        s
    }

    fn empty_matcher() -> Self {
        let nucleo = nucleo::Nucleo::new(
            nucleo::Config::DEFAULT,
            Arc::new(|| {}),
            None,
            1,
        );
        Self {
            nucleo,
            pattern: String::new(),
            visible_indices: Vec::new(),
        }
    }

    fn replace_items(&mut self, rows: &[RowInfo]) {
        let nucleo = nucleo::Nucleo::new(
            nucleo::Config::DEFAULT,
            Arc::new(|| {}),
            None,
            1,
        );
        let injector = nucleo.injector();
        for info in rows {
            injector.push(info.clone(), |_row, dst| {
                dst[0] = nucleo::Utf32String::from(
                    format!("{}_{}", info.name, info.version).as_str(),
                );
            });
        }
        self.nucleo = nucleo;
        self.recompute_visible();
    }

    fn recompute_visible(&mut self) {
        if self.pattern.is_empty() {
            let count = self.nucleo.injector().injected_items() as usize;
            self.visible_indices = (0..count).collect();
            return;
        }
        self.nucleo.pattern.reparse(
            0,
            &self.pattern,
            nucleo::pattern::CaseMatching::Ignore,
            nucleo::pattern::Normalization::Smart,
            false,
        );
        let _ = self.nucleo.tick(1_000_000);
        let snapshot = self.nucleo.snapshot();
        self.visible_indices = snapshot
            .matched_items(..)
            .map(|item| item.data.index)
            .collect();
    }

    pub fn set_pattern(&mut self, pattern: &str) {
        self.pattern = pattern.to_lowercase();
        self.recompute_visible();
    }

    pub fn update_items(&mut self, rows: &[RowInfo]) {
        self.replace_items(rows);
    }
}

#[derive(Clone)]
pub struct RowInfo {
    pub index: usize,
    pub name: String,
    pub version: String,
}

pub struct App {
    pub running: bool,
    pub config: Config,
    pub families: BTreeMap<String, ImageFamily>,
    pub unknowns: Vec<UnknownTarball>,
    pub rows: Vec<ImageTableRow>,
    pub server_names: Vec<String>,
    pub server_scan_status: BTreeMap<String, ScanStatus>,
    pub selected_row: usize,
    pub remote_rx: Option<mpsc::UnboundedReceiver<RemoteEvent>>,
    pub status_message: String,
    pub search_query: String,
    pub search_active: bool,
    pub filtered_indices: Vec<usize>,
    pub scanned_servers: usize,
    pub total_servers: usize,
    pub searcher: Searcher,
    pub popup: Popup,
    pub popup_selection: usize,
    pub viewing_logs: bool,
    pub log_viewer: LogViewer,
    pub log_selection: LogSelection,
    pub log_area: Rect,
    pub current_log_lines: Vec<String>,
    pub current_log_title: String,
    pub build_rx: Option<mpsc::UnboundedReceiver<build::BuildEvent>>,
    pub build_handle: Option<build::BuildHandle>,
    pub current_build_name: Option<String>,
    pub operation_rx: Option<mpsc::UnboundedReceiver<RemoteEvent>>,
    pub operation_in_flight: bool,
    pub operation_expected: usize,
}

impl App {
    pub async fn new(config: Config) -> Result<Self> {
        let server_images_dir = Path::new(&config.paths.server_images_dir);
        let discovery = discover_image_families(server_images_dir)?;
        let mut families = discovery.families;
        let warnings = discovery.warnings;

        let staging_dir = Path::new(&config.paths.staging_dir).to_path_buf();
        remote::refresh_local_tarballs(&mut families, &staging_dir);

        let server_names: Vec<String> = config.servers.iter().map(|s| s.name.clone()).collect();
        let mut server_scan_status: BTreeMap<String, ScanStatus> = BTreeMap::new();
        for name in &server_names {
            server_scan_status.insert(name.clone(), ScanStatus::Pending);
        }
        let total_servers = server_names.len();

        let rx = remote::start_remote_scan(
            config.servers.clone(),
            config.paths.rke2_images_dir.clone(),
        );

        let unknowns: Vec<UnknownTarball> = Vec::new();
        let rows = inventory::rows_from(&families, &unknowns);
        let searcher = Searcher::new(&rows_to_info(&rows));

        let initial_status = if warnings.is_empty() {
            format!("Scanning {} servers...", total_servers)
        } else {
            format!(
                "Scanning {} servers... ({} startup warnings)",
                total_servers,
                warnings.len()
            )
        };

        Ok(Self {
            running: true,
            config,
            families,
            unknowns,
            rows,
            server_names,
            server_scan_status,
            selected_row: 0,
            remote_rx: Some(rx),
            status_message: initial_status,
            search_query: String::new(),
            search_active: false,
            filtered_indices: Vec::new(),
            scanned_servers: 0,
            total_servers,
            searcher,
            popup: Popup::Hidden,
            popup_selection: 0,
            viewing_logs: false,
            log_viewer: LogViewer::new(),
            log_selection: LogSelection::default(),
            log_area: Rect::default(),
            current_log_lines: Vec::new(),
            current_log_title: String::new(),
            build_rx: None,
            build_handle: None,
            current_build_name: None,
            operation_rx: None,
            operation_in_flight: false,
            operation_expected: 0,
        })
    }

    pub fn quit(&mut self) {
        if let Some(handle) = self.build_handle.take() {
            handle.cancel();
        }
        self.running = false;
    }

    pub fn selected_row_index(&self) -> Option<usize> {
        if self.search_active && !self.filtered_indices.is_empty() {
            self.filtered_indices.get(self.selected_row).copied()
        } else if self.rows.is_empty() {
            None
        } else {
            Some(self.selected_row.min(self.rows.len().saturating_sub(1)))
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        let len = if self.search_active && !self.filtered_indices.is_empty() {
            self.filtered_indices.len()
        } else {
            self.rows.len()
        };

        if len == 0 {
            return;
        }

        let current = self.selected_row as i32;
        let new = (current + delta).rem_euclid(len as i32);
        self.selected_row = new as usize;
    }

    pub fn refresh_local_tarballs(&mut self) {
        let staging = Path::new(&self.config.paths.staging_dir).to_path_buf();
        remote::refresh_local_tarballs(&mut self.families, &staging);
    }

    pub fn rebuild_rows(&mut self) {
        self.rows = inventory::rows_from(&self.families, &self.unknowns);
        self.searcher.update_items(&rows_to_info(&self.rows));
    }

    pub fn open_action_popup(&mut self) {
        let row = self.selected_row_index();
        if let Some(idx) = row {
            if let Some(row) = self.rows.get(idx) {
                let (entries, title) = match row {
                    ImageTableRow::Current(image) => {
                        let title = format!("{} {}", image.name, image.version);
                        let entries = build_current_actions(image, self);
                        (entries, title)
                    }
                    ImageTableRow::Stale { family_name, version, .. } => {
                        let title = format!("{} {} (stale)", family_name, version);
                        let entries = build_cleanup_actions();
                        (entries, title)
                    }
                    ImageTableRow::Unknown(unknown) => {
                        let title = format!("{} (unknown)", unknown.filename);
                        let entries = build_cleanup_actions();
                        (entries, title)
                    }
                };
                self.popup = Popup::Actions { entries, title };
                self.popup_selection = 0;
                return;
            }
        }
        self.popup = Popup::Hidden;
    }

    pub fn open_server_picker(
        &mut self,
        for_action: Action,
        row_index: usize,
    ) {
        let (selected, title) = match &self.rows.get(row_index) {
            Some(ImageTableRow::Current(image)) => {
                let pre = match for_action {
                    Action::Deploy => self.servers_without_tarball(image),
                    Action::Remove => self.servers_with_tarball_current(image),
                    _ => vec![true; self.server_names.len()],
                };
                let title = match for_action {
                    Action::Deploy => format!("Deploy {} to...", image.name),
                    Action::Remove => format!("Remove {} from...", image.name),
                    _ => "Select servers".to_string(),
                };
                (pre, title)
            }
            Some(ImageTableRow::Stale { family_name, version, .. }) => {
                let pre = self.servers_with_tarball_stale(family_name, version);
                let title = format!("Remove {} {} from...", family_name, version);
                (pre, title)
            }
            Some(ImageTableRow::Unknown(unknown)) => {
                let pre = unknown.servers.iter().map(|n| self.server_names.contains(n)).collect();
                let title = format!("Remove {} from...", unknown.filename);
                (pre, title)
            }
            None => (vec![true; self.server_names.len()], "Select servers".to_string()),
        };

        self.popup = Popup::ServerPicker {
            selected,
            for_action,
            row_index,
            title,
            server_names: self.server_names.clone(),
        };
        self.popup_selection = 0;
    }

    fn servers_without_tarball(&self, image: &crate::models::ManagedImage) -> Vec<bool> {
        self.server_names
            .iter()
            .map(|name| !image.server_presence.get(name).copied().unwrap_or(false))
            .collect()
    }

    fn servers_with_tarball_current(&self, image: &crate::models::ManagedImage) -> Vec<bool> {
        self.server_names
            .iter()
            .map(|name| image.server_presence.get(name).copied().unwrap_or(false))
            .collect()
    }

    fn servers_with_tarball_stale(&self, family_name: &str, version: &str) -> Vec<bool> {
        let family = match self.families.get(family_name) {
            Some(f) => f,
            None => return vec![true; self.server_names.len()],
        };
        let servers = family.remote_presence.get(version).cloned().unwrap_or_default();
        self.server_names
            .iter()
            .map(|name| servers.contains(name))
            .collect()
    }

    pub fn popup_move(&mut self, delta: i32) {
        let max = match &self.popup {
            Popup::Actions { entries, .. } => entries.len(),
            // checkboxes + [Continue] + [Cancel]
            Popup::ServerPicker { selected, .. } => selected.len() + 2,
            Popup::Hidden => return,
        };

        if max == 0 {
            return;
        }

        let current = self.popup_selection as i32;
        let new = (current + delta).rem_euclid(max as i32);
        self.popup_selection = new as usize;
    }

    pub fn popup_toggle(&mut self) {
        if let Popup::ServerPicker { ref mut selected, .. } = &mut self.popup {
            if let Some(checked) = selected.get_mut(self.popup_selection) {
                *checked = !*checked;
            }
        }
    }

    pub fn popup_confirm(&mut self) {
        let popup = self.popup.clone();
        match popup {
            Popup::Actions { entries, .. } => {
                if let Some(entry) = entries.get(self.popup_selection) {
                    if entry.dimmed {
                        return;
                    }
                    let action = entry.action.clone();
                    self.handle_action(action);
                }
            }
            Popup::ServerPicker {
                selected,
                for_action,
                row_index,
                ..
            } => {
                let continue_idx = selected.len();
                let cancel_idx = selected.len() + 1;
                match self.popup_selection {
                    i if i == continue_idx => {
                        self.execute_picker(for_action, row_index, selected);
                    }
                    i if i == cancel_idx => {
                        self.popup = Popup::Hidden;
                        self.popup_selection = 0;
                    }
                    _ => {
                        self.popup_toggle();
                    }
                }
            }
            Popup::Hidden => {}
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::Build => {
                self.popup = Popup::Hidden;
                self.start_build_for_row();
            }
            Action::Deploy => {
                self.popup = Popup::Hidden;
                if let Some(idx) = self.selected_row_index() {
                    self.open_server_picker(Action::Deploy, idx);
                }
            }
            Action::Remove => {
                self.popup = Popup::Hidden;
                if let Some(idx) = self.selected_row_index() {
                    self.open_server_picker(Action::Remove, idx);
                }
            }
            Action::ViewLogs => {
                self.popup = Popup::Hidden;
                if let Some(idx) = self.selected_row_index() {
                    if let Some(ImageTableRow::Current(image)) = self.rows.get(idx) {
                        let logs = image.build_log.clone();
                        let name = image.name.clone();
                        self.open_log_viewer(&name, logs);
                    }
                }
            }
            Action::DumpLogs => {
                self.popup = Popup::Hidden;
                if let Some(idx) = self.selected_row_index() {
                    if let Some(ImageTableRow::Current(image)) = self.rows.get(idx) {
                        let staging = self.config.paths.staging_dir.clone();
                        let name = image.name.clone();
                        let version = image.version.clone();
                        let log = image.build_log.clone();
                        match build::dump_log_to_file(&staging, &name, &version, &log) {
                            Ok(path) => {
                                self.status_message = format!("Dumped logs to {}", path.display());
                            }
                            Err(e) => {
                                self.status_message = format!("Failed to dump logs: {}", e);
                            }
                        }
                    }
                }
            }
            Action::Cancel => {
                self.popup = Popup::Hidden;
                self.popup_selection = 0;
            }
        }
    }

    fn execute_picker(&mut self, for_action: Action, row_index: usize, selected: Vec<bool>) {
        let row_clone = self.rows.get(row_index).cloned();
        let target_count: usize = selected.iter().filter(|x| **x).count();

        if target_count == 0 {
            self.popup = Popup::Hidden;
            self.status_message = "No servers selected".to_string();
            return;
        }

        match (for_action, row_clone) {
            (Action::Deploy, Some(ImageTableRow::Current(image))) => {
                self.popup = Popup::Hidden;
                self.execute_deploy(&image, selected, target_count);
            }
            (Action::Remove, Some(row @ (ImageTableRow::Current(_)
            | ImageTableRow::Stale { .. }
            | ImageTableRow::Unknown(_)))) => {
                self.popup = Popup::Hidden;
                self.execute_remove(&row, selected, target_count);
            }
            _ => {
                self.popup = Popup::Hidden;
                self.status_message = "Cannot perform that action on this row".to_string();
            }
        }
    }

    pub fn popup_cancel(&mut self) {
        self.popup = Popup::Hidden;
        self.popup_selection = 0;
    }

    pub fn start_build_for_row(&mut self) {
        if self.build_handle.is_some() {
            self.status_message = "A build is already in progress".to_string();
            return;
        }

        let Some(idx) = self.selected_row_index() else { return; };
        let Some(row) = self.rows.get(idx) else { return; };
        let ImageTableRow::Current(image) = row else { return; };

        if matches!(image.build_state, crate::models::BuildState::Building) {
            self.status_message = "Already building this image".to_string();
            return;
        }

        let image_for_build = image.clone();
        let name = image.name.clone();
        let (rx, handle) = build::start_build(
            image_for_build,
            self.config.paths.image_registry.clone(),
            self.config.paths.staging_dir.clone(),
            Path::new(&self.config.paths.server_images_dir).to_path_buf(),
        );
        self.build_rx = Some(rx);
        self.build_handle = Some(handle);
        self.current_build_name = Some(name.clone());

        if let Some(family) = self.families.get_mut(&name) {
            if let Some(ref mut current) = family.current {
                current.build_state = crate::models::BuildState::Building;
                current.build_log.clear();
            }
        }
        self.rebuild_rows();
        self.status_message = format!("Building {}...", name);
    }

    pub fn dump_current_log(&mut self) {
        if let Some(name) = &self.current_build_name {
            if let Some(family) = self.families.get(name) {
                if let Some(image) = &family.current {
                    let staging = self.config.paths.staging_dir.clone();
                    let name = image.name.clone();
                    let version = image.version.clone();
                    let log = image.build_log.clone();
                    match build::dump_log_to_file(&staging, &name, &version, &log) {
                        Ok(path) => {
                            self.status_message = format!("Dumped logs to {}", path.display());
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to dump logs: {}", e);
                        }
                    }
                    return;
                }
            }
        }
        self.status_message = "No build log to dump".to_string();
    }

    pub fn handle_events(&mut self) {
        self.drain_remote();
        self.drain_build();
    }

    fn drain_remote(&mut self) {
        if let Some(mut rx) = self.remote_rx.take() {
            while let Ok(event) = rx.try_recv() {
                self.process_remote_event(event);
            }
            self.remote_rx = Some(rx);
        }
        if let Some(mut rx) = self.operation_rx.take() {
            while let Ok(event) = rx.try_recv() {
                self.process_remote_event(event);
            }
            self.operation_rx = Some(rx);
        }
    }

    fn process_remote_event(&mut self, event: RemoteEvent) {
        match event {
            RemoteEvent::Scan {
                server_name,
                filenames,
            } => {
                self.scanned_servers += 1;
                match filenames {
                    Ok(files) => {
                        self.server_scan_status
                            .insert(server_name.clone(), ScanStatus::Ok);
                        remote::apply_scan_results(
                            &mut self.families,
                            &mut self.unknowns,
                            &server_name,
                            &files,
                        );
                    }
                    Err(err) => {
                        self.server_scan_status
                            .insert(server_name.clone(), ScanStatus::Error(err.clone()));
                        self.status_message =
                            format!("SSH error on {}: {}", server_name, err);
                    }
                }
                if self.scanned_servers >= self.total_servers {
                    self.status_message = count_status(self, false);
                } else {
                    self.status_message = format!(
                        "Scanning servers... {}/{}",
                        self.scanned_servers, self.total_servers
                    );
                }
                self.rebuild_rows();
            }
            RemoteEvent::Deploy {
                server_name,
                success,
                error,
            } => {
                if success {
                    self.status_message = format!("Deployed to {}", server_name);
                    if let Some(family) = self.families.get_mut(
                        self.current_build_name
                            .as_deref()
                            .unwrap_or(&server_name),
                    ) {
                        if let Some(ref mut current) = family.current {
                            current
                                .server_presence
                                .insert(server_name.clone(), true);
                        }
                    }
                } else if let Some(err) = error {
                    self.status_message =
                        format!("Deploy failed on {}: {}", server_name, err);
                }
                self.operation_expected = self.operation_expected.saturating_sub(1);
                if self.operation_expected == 0 {
                    self.operation_in_flight = false;
                    self.rebuild_rows();
                    self.status_message = count_status(self, false);
                }
            }
            RemoteEvent::Remove {
                server_name,
                tarball,
                success,
                error,
            } => {
                if success {
                    self.status_message = format!("Removed from {}", server_name);
                    self.apply_local_removal(&tarball, &server_name);
                } else if let Some(err) = error {
                    self.status_message =
                        format!("Remove failed on {}: {}", server_name, err);
                }
                self.operation_expected = self.operation_expected.saturating_sub(1);
                if self.operation_expected == 0 {
                    self.operation_in_flight = false;
                    self.rebuild_rows();
                    self.status_message = count_status(self, false);
                }
            }
        }
    }

    #[allow(clippy::collapsible_match)]
    fn apply_local_removal(&mut self, tarball: &str, server_name: &str) {
        for family in self.families.values_mut() {
            if let Some(ref mut current) = family.current {
                if tarball_filename(&current.name, &current.version) == tarball {
                    current.server_presence.insert(server_name.to_string(), false);
                    return;
                }
            }
            let version_to_remove = family
                .remote_presence
                .iter()
                .find(|(v, _)| tarball_filename(&family.name, v) == tarball)
                .map(|(v, _)| v.clone());
            if let Some(v) = version_to_remove {
                if let Some(set) = family.remote_presence.get_mut(&v) {
                    set.remove(server_name);
                    // If no servers have this stale version anymore, drop it entirely
                    // so the stale row stops being re-created by build_rows.
                    if set.is_empty() {
                        family.remote_presence.remove(&v);
                        family.stale_versions.remove(&v);
                    }
                }
            }
        }
        for unknown in self.unknowns.iter_mut() {
            if unknown.filename == tarball {
                unknown.servers.remove(server_name);
            }
        }
        self.unknowns.retain(|u| !u.servers.is_empty());
    }

    fn drain_build(&mut self) {
        let mut events: Vec<build::BuildEvent> = if let Some(ref mut rx) = self.build_rx {
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        } else {
            return;
        };

        events.sort_by_key(|e| match e {
            build::BuildEvent::LogLine { seq, .. } => *seq,
            build::BuildEvent::BuildComplete { .. } => u64::MAX,
        });

        for event in events {
            match event {
                build::BuildEvent::LogLine { image_name, line, .. } => {
                    if let Some(family) = self.families.get_mut(&image_name) {
                        if let Some(ref mut current) = family.current {
                            current.build_log.push(line.clone());
                            if current.build_log.len() > MAX_LOG_LINES {
                                let excess = current.build_log.len() - MAX_LOG_LINES;
                                current.build_log.drain(0..excess);
                            }
                        }
                    }
                    if self.current_build_name.as_deref() == Some(image_name.as_str())
                        || self.viewing_logs
                    {
                        self.current_log_lines.push(line);
                        if self.current_log_lines.len() > MAX_LOG_LINES {
                            let excess = self.current_log_lines.len() - MAX_LOG_LINES;
                            self.current_log_lines.drain(0..excess);
                        }
                    }
                }
                build::BuildEvent::BuildComplete {
                    image_name,
                    result,
                    tarball_written,
                } => {
                    if let Some(family) = self.families.get_mut(&image_name) {
                        if let Some(ref mut current) = family.current {
                            current.build_state = match result {
                                Ok(()) => crate::models::BuildState::Success,
                                Err(e) => crate::models::BuildState::Failed(e),
                            };
                        }
                    }
                    self.build_rx = None;
                    self.build_handle = None;
                    self.current_build_name = None;
                    if tarball_written {
                        self.refresh_local_tarballs();
                    }
                    self.rebuild_rows();
                    self.status_message = count_status(self, true);
                }
            }
        }
    }

    pub fn open_log_viewer(&mut self, image_name: &str, lines: Vec<String>) {
        self.viewing_logs = true;
        self.current_log_lines = lines;
        self.current_log_title = format!("Build Log: {}", image_name);
        self.log_viewer = LogViewer::new();
        self.log_selection.clear();
    }

    pub fn close_log_viewer(&mut self) {
        self.viewing_logs = false;
        self.current_log_lines.clear();
        self.current_log_title.clear();
        self.log_selection.clear();
    }

    /// Convert a screen-space mouse coordinate to a (row, col) inside the
    /// log viewer's content area. Returns `None` if the coordinate falls on
    /// the border, outside the viewer, or on an empty line range.
    pub fn mouse_to_log_text(&self, screen_x: u16, screen_y: u16) -> Option<(usize, usize)> {
        let area = self.log_area;
        // Content area is inside the 1-cell border.
        let inner_x_start = area.x + 1;
        let inner_x_end = area.x + area.width.saturating_sub(1);
        let inner_y_start = area.y + 1;
        let inner_y_end = area.y + area.height.saturating_sub(1);
        if screen_x < inner_x_start || screen_x >= inner_x_end {
            return None;
        }
        if screen_y < inner_y_start || screen_y >= inner_y_end {
            return None;
        }
        let col = (screen_x - inner_x_start) as usize;
        let visible_row = (screen_y - inner_y_start) as usize;
        let row = visible_row + self.log_viewer.scroll_offset;
        // Don't accept clicks past the actual end of the log.
        if row >= self.current_log_lines.len() {
            return None;
        }
        Some((row, col))
    }

    pub fn handle_log_mouse_down(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((row, col)) = self.mouse_to_log_text(screen_x, screen_y) {
            self.log_selection.start(row, col);
        }
    }

    pub fn handle_log_mouse_drag(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((row, col)) = self.mouse_to_log_text(screen_x, screen_y) {
            self.log_selection.update(row, col);
        }
    }

    pub fn handle_log_mouse_up(&mut self, screen_x: u16, screen_y: u16) {
        if let Some((row, col)) = self.mouse_to_log_text(screen_x, screen_y) {
            self.log_selection.update(row, col);
        }
    }

    pub fn execute_deploy(
        &mut self,
        image: &crate::models::ManagedImage,
        selected: Vec<bool>,
        target_count: usize,
    ) {
        let tarball_name = tarball_filename(&image.name, &image.version);
        let local_path = Path::new(&self.config.paths.staging_dir).join(&tarball_name);

        if !local_path.is_file() {
            self.status_message = format!("No local tarball for {} (build first)", image.name);
            return;
        }

        let (tx, rx) = mpsc::unbounded_channel();
        self.operation_rx = Some(rx);
        self.operation_in_flight = true;
        self.operation_expected = target_count;
        self.current_build_name = Some(image.name.clone());

        let servers = self.config.servers.clone();
        let remote_dir = self.config.paths.rke2_images_dir.clone();

        tokio::task::spawn(async move {
            for (i, server) in servers.iter().enumerate() {
                if !selected.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let result = remote::deploy_tarball(server, &local_path, &remote_dir).await;
                let _ = tx.send(RemoteEvent::Deploy {
                    server_name: server.name.clone(),
                    success: result.is_ok(),
                    error: result.err(),
                });
            }
        });

        self.status_message = format!("Deploying {}...", image.name);
    }

    pub fn execute_remove(
        &mut self,
        row: &ImageTableRow,
        selected: Vec<bool>,
        target_count: usize,
    ) {
        let tarball_name: String = match row {
            ImageTableRow::Current(image) => tarball_filename(&image.name, &image.version),
            ImageTableRow::Stale { family_name, version, .. } => {
                tarball_filename(family_name, version)
            }
            ImageTableRow::Unknown(unknown) => unknown.filename.clone(),
        };

        let (tx, rx) = mpsc::unbounded_channel();
        self.operation_rx = Some(rx);
        self.operation_in_flight = true;
        self.operation_expected = target_count;

        let servers = self.config.servers.clone();
        let remote_dir = self.config.paths.rke2_images_dir.clone();
        let tarball_name_for_task = tarball_name.clone();

        tokio::task::spawn(async move {
            for (i, server) in servers.iter().enumerate() {
                if !selected.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let result = remote::remove_tarball(server, &tarball_name_for_task, &remote_dir).await;
                let _ = tx.send(RemoteEvent::Remove {
                    server_name: server.name.clone(),
                    tarball: tarball_name_for_task.clone(),
                    success: result.is_ok(),
                    error: result.err(),
                });
            }
        });

        self.status_message = format!("Removing {} from servers...", tarball_name);
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
        self.searcher.set_pattern(&self.search_query);
        self.filtered_indices = self.searcher.visible_indices.clone();
        self.selected_row = 0;
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
        self.searcher.set_pattern(&self.search_query);
        self.filtered_indices = self.searcher.visible_indices.clone();
        if self.selected_row >= self.filtered_indices.len().saturating_sub(1) {
            self.selected_row = 0;
        }
    }

    pub fn search_clear(&mut self) {
        self.search_query.clear();
        self.searcher.set_pattern("");
        self.filtered_indices = self.searcher.visible_indices.clone();
        self.selected_row = 0;
    }
}

fn rows_to_info(rows: &[ImageTableRow]) -> Vec<RowInfo> {
    rows.iter()
        .enumerate()
        .map(|(i, row)| match row {
            ImageTableRow::Current(img) => RowInfo {
                index: i,
                name: img.name.clone(),
                version: img.version.clone(),
            },
            ImageTableRow::Stale {
                family_name,
                version,
                ..
            } => RowInfo {
                index: i,
                name: family_name.clone(),
                version: version.clone(),
            },
            ImageTableRow::Unknown(unknown) => RowInfo {
                index: i,
                name: unknown.filename.clone(),
                version: String::new(),
            },
        })
        .collect()
}

fn build_current_actions(image: &crate::models::ManagedImage, app: &App) -> Vec<PopupEntry> {
    let building = matches!(image.build_state, crate::models::BuildState::Building);
    let entries = vec![
        PopupEntry {
            action: Action::Build,
            dimmed: building,
            label: if building {
                "  [Building...]".to_string()
            } else {
                "  Build".to_string()
            },
        },
        PopupEntry {
            action: Action::Deploy,
            dimmed: !image.local_tarball,
            label: "  Deploy to servers...".to_string(),
        },
        PopupEntry {
            action: Action::Remove,
            dimmed: !image.present_on_any_server(),
            label: "  Remove from servers...".to_string(),
        },
        PopupEntry {
            action: Action::ViewLogs,
            dimmed: image.build_log.is_empty(),
            label: "  View build logs".to_string(),
        },
        PopupEntry {
            action: Action::DumpLogs,
            dimmed: image.build_log.is_empty(),
            label: "  Dump logs to file".to_string(),
        },
        PopupEntry {
            action: Action::Cancel,
            dimmed: false,
            label: "  Cancel".to_string(),
        },
    ];
    let _ = app; // reserved for future context
    entries
}

fn build_cleanup_actions() -> Vec<PopupEntry> {
    vec![
        PopupEntry {
            action: Action::Remove,
            dimmed: false,
            label: "  Remove from servers...".to_string(),
        },
        PopupEntry {
            action: Action::Cancel,
            dimmed: false,
            label: "  Cancel".to_string(),
        },
    ]
}

pub fn count_status(app: &App, include_build: bool) -> String {
    let managed_count = app.families.len();
    let stale_count: usize = app.families.values().map(|f| f.stale_versions.len()).sum();
    let unknown_count = app.unknowns.len();
    let building_count = if include_build && app.build_handle.is_some() { 1 } else { 0 };
    format!(
        "{} managed ({} stale) | {} unknown | {} building...",
        managed_count, stale_count, unknown_count, building_count
    )
}
