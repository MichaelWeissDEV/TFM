mod config;
mod core;
mod markers;
mod preview;
mod security;
mod ui;

use crate::config::Config;
use crate::core::FileEntry;
use crate::markers::MarkerStore;
use crate::preview::Preview;
use arboard::Clipboard;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, event, execute};
use regex::RegexBuilder;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::Resize;
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::future::Future;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_stream::StreamExt;

const DIR_BATCH_SIZE: usize = 512;

#[derive(Clone, Copy)]
enum DirTarget {
    Parent,
    Current,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InputAction {
    Search,
    MarkerSearch,
    AddFile,
    AddDir,
    Rename,
    MarkerSet,
    MarkerJump,
    MarkerRename { name: String },
    MarkerEditPath { name: String },
    MarkerCreateName,
    MarkerCreatePath { name: String },
    ConfirmDelete,
}

#[derive(Debug)]
struct InputState {
    action: InputAction,
    buffer: String,
}

impl InputState {
    fn new(action: InputAction, buffer: String) -> Self {
        Self { action, buffer }
    }

    fn title(&self) -> &'static str {
        match self.action.clone() {
            InputAction::Search => "Search (regex)",
            InputAction::MarkerSearch => "Search Markers (n:/p:)",
            InputAction::AddFile => "Add File",
            InputAction::AddDir => "Add Dir",
            InputAction::Rename => "Rename",
            InputAction::MarkerSet => "Set Marker",
            InputAction::MarkerJump => "Jump Marker",
            InputAction::MarkerRename { .. } => "Rename Marker",
            InputAction::MarkerEditPath { .. } => "Edit Marker Path",
            InputAction::MarkerCreateName => "New Marker Name",
            InputAction::MarkerCreatePath { .. } => "New Marker Path",
            InputAction::ConfirmDelete => "Delete",
        }
    }
}

#[derive(Debug)]
enum Mode {
    Normal,
    Input(InputState),
    MarkerList,
    ProgramList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingPrefix {
    Add,
    Settings,
    Copy,
    View,
    Delete,
    OpenWith,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardOp {
    Cut,
    Copy,
}

#[derive(Clone, Debug)]
struct ClipboardEntry {
    op: ClipboardOp,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct MarkerListEntry {
    name: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct ProgramEntry {
    name: String,
    path: PathBuf,
}

#[derive(Clone, Copy)]
enum MarkerFilterMode {
    Any,
    Name,
    Path,
}

#[derive(Debug)]
struct MarkerListState {
    entries: Vec<MarkerListEntry>,
    filtered_indices: Vec<usize>,
    selected: usize,
    filter: String,
}

#[derive(Debug)]
struct ProgramListState {
    entries: Vec<ProgramEntry>,
    filtered_indices: Vec<usize>,
    selected: usize,
    filter: String,
}

fn parse_marker_filter(query: &str) -> (MarkerFilterMode, String) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return (MarkerFilterMode::Any, String::new());
    }
    let lower = trimmed.to_ascii_lowercase();
    let (mode, rest) = if let Some(rest) = lower.strip_prefix("n:") {
        (MarkerFilterMode::Name, rest)
    } else if let Some(rest) = lower.strip_prefix("n/") {
        (MarkerFilterMode::Name, rest)
    } else if let Some(rest) = lower.strip_prefix("name:") {
        (MarkerFilterMode::Name, rest)
    } else if let Some(rest) = lower.strip_prefix("name/") {
        (MarkerFilterMode::Name, rest)
    } else if let Some(rest) = lower.strip_prefix("p:") {
        (MarkerFilterMode::Path, rest)
    } else if let Some(rest) = lower.strip_prefix("p/") {
        (MarkerFilterMode::Path, rest)
    } else if let Some(rest) = lower.strip_prefix("path:") {
        (MarkerFilterMode::Path, rest)
    } else if let Some(rest) = lower.strip_prefix("path/") {
        (MarkerFilterMode::Path, rest)
    } else {
        (MarkerFilterMode::Any, lower.as_str())
    };
    (mode, rest.trim().to_string())
}

impl MarkerListState {
    fn new(markers: &MarkerStore) -> Self {
        let mut entries: Vec<MarkerListEntry> = markers
            .entries()
            .map(|(name, path)| MarkerListEntry {
                name: name.clone(),
                path: path.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        let filtered_indices = (0..entries.len()).collect();
        Self {
            entries,
            filtered_indices,
            selected: 0,
            filter: String::new(),
        }
    }

    fn selected_entry(&self) -> Option<&MarkerListEntry> {
        let index = *self.filtered_indices.get(self.selected)?;
        self.entries.get(index)
    }

    fn sync(&mut self, markers: &MarkerStore, preferred: Option<&str>) {
        let current = preferred.map(|name| name.to_string()).or_else(|| {
            self.selected_entry()
                .map(|entry| entry.name.clone())
        });
        let mut entries: Vec<MarkerListEntry> = markers
            .entries()
            .map(|(name, path)| MarkerListEntry {
                name: name.clone(),
                path: path.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        self.entries = entries;
        self.apply_filter(current.as_deref());
    }

    fn update_filter(&mut self, value: String) {
        let preferred = self.selected_entry().map(|entry| entry.name.clone());
        self.filter = value;
        self.apply_filter(preferred.as_deref());
    }

    fn clear_filter(&mut self) {
        let preferred = self.selected_entry().map(|entry| entry.name.clone());
        self.filter.clear();
        self.apply_filter(preferred.as_deref());
    }

    fn apply_filter(&mut self, preferred: Option<&str>) {
        let (mode, query) = parse_marker_filter(&self.filter);
        self.filtered_indices = if query.is_empty() {
            (0..self.entries.len()).collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    let name = entry.name.to_ascii_lowercase();
                    let path = entry.path.to_string_lossy().to_ascii_lowercase();
                    match mode {
                        MarkerFilterMode::Any => name.contains(&query) || path.contains(&query),
                        MarkerFilterMode::Name => name.contains(&query),
                        MarkerFilterMode::Path => path.contains(&query),
                    }
                })
                .map(|(index, _)| index)
                .collect()
        };
        let mut selected = 0usize;
        if let Some(name) = preferred {
            if let Some(pos) = self
                .filtered_indices
                .iter()
                .position(|&index| self.entries[index].name == name)
            {
                selected = pos;
            }
        }
        if !self.filtered_indices.is_empty() {
            self.selected = selected.min(self.filtered_indices.len() - 1);
        } else {
            self.selected = 0;
        }
    }
}

impl ProgramListState {
    fn new(programs: &[ProgramEntry]) -> Self {
        let mut entries = programs.to_vec();
        entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        let filtered_indices = (0..entries.len()).collect();
        Self {
            entries,
            filtered_indices,
            selected: 0,
            filter: String::new(),
        }
    }

    fn selected_entry(&self) -> Option<&ProgramEntry> {
        let index = *self.filtered_indices.get(self.selected)?;
        self.entries.get(index)
    }

    fn update_filter(&mut self, value: String) {
        let preferred = self.selected_entry().map(|entry| entry.name.clone());
        self.filter = value;
        self.apply_filter(preferred.as_deref());
    }

    fn apply_filter(&mut self, preferred: Option<&str>) {
        let query = self.filter.trim().to_ascii_lowercase();
        self.filtered_indices = if query.is_empty() {
            (0..self.entries.len()).collect()
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    let name = entry.name.to_ascii_lowercase();
                    let path = entry.path.to_string_lossy().to_ascii_lowercase();
                    name.contains(&query) || path.contains(&query)
                })
                .map(|(index, _)| index)
                .collect()
        };
        let mut selected = 0usize;
        if let Some(name) = preferred {
            if let Some(pos) = self
                .filtered_indices
                .iter()
                .position(|&index| self.entries[index].name == name)
            {
                selected = pos;
            }
        }
        if !self.filtered_indices.is_empty() {
            self.selected = selected.min(self.filtered_indices.len() - 1);
        } else {
            self.selected = 0;
        }
    }
}

enum AppEvent {
    Input(Event),
    Preview {
        id: u64,
        result: Result<Preview, core::CoreError>,
    },
    DirEntries {
        id: u64,
        target: DirTarget,
        entries: Vec<FileEntry>,
        done: bool,
    },
    ImageReady {
        version: u64,
        protocol: Box<dyn StatefulProtocol>,
    },
    Action(ActionResult),
}

enum ActionResult {
    Refresh { select: Option<PathBuf> },
}

#[derive(Debug, Clone)]
enum SuspendAction {
    Shell(PathBuf),
    OpenWith {
        program: PathBuf,
        path: PathBuf,
        cwd: PathBuf,
    },
}

#[derive(Default)]
struct InputEffect {
    exit: bool,
    redraw: bool,
    request_preview: bool,
    suspend: Option<SuspendAction>,
}

struct App {
    config: Config,
    picker: Picker,
    current_dir: PathBuf,
    parent_entries: Vec<FileEntry>,
    current_entries: Vec<FileEntry>,
    filtered_indices: Vec<usize>,
    selected: usize,
    filter: String,
    show_hidden: bool,
    mode: Mode,
    pending_prefix: Option<PendingPrefix>,
    marker_list: Option<MarkerListState>,
    program_list: Option<ProgramListState>,
    programs: Vec<ProgramEntry>,
    preview: Option<Preview>,
    highlighted_preview: Option<ui::HighlightedText>,
    show_metadata: bool,
    show_permissions: bool,
    show_dates: bool,
    show_owner: bool,
    show_list_permissions: bool,
    show_list_owner: bool,
    preview_request_id: u64,
    preview_pending: bool,
    listing_id: u64,
    pending_selection: Option<PathBuf>,
    image_state: Option<ui::ThreadProtocol>,
    image_version: u64,
    image_worker_tx: Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>,
    clipboard: Option<ClipboardEntry>,
    markers: MarkerStore,
}

impl App {
    async fn new(
        config: Config,
        picker: Picker,
        image_worker_tx: Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self, core::CoreError> {
        let current_dir = env::current_dir()?;
        let markers = MarkerStore::load().await;
        let programs = match tokio::task::spawn_blocking(scan_programs).await {
            Ok(programs) => programs,
            Err(_) => Vec::new(),
        };
        let mut app = Self {
            show_metadata: config.metadata_bar.enabled,
            show_permissions: config.metadata_bar.show_permissions,
            show_dates: config.metadata_bar.show_dates,
            show_owner: config.metadata_bar.show_owner,
            show_list_permissions: false,
            show_list_owner: false,
            config,
            picker,
            current_dir,
            parent_entries: Vec::new(),
            current_entries: Vec::new(),
            filtered_indices: Vec::new(),
            selected: 0,
            filter: String::new(),
            show_hidden: true,
            mode: Mode::Normal,
            pending_prefix: None,
            marker_list: None,
            program_list: None,
            programs,
            preview: None,
            highlighted_preview: None,
            preview_request_id: 0,
            preview_pending: false,
            listing_id: 0,
            pending_selection: None,
            image_state: None,
            image_version: 0,
            image_worker_tx,
            clipboard: None,
            markers,
        };
        app.refresh_dirs(tx);
        Ok(app)
    }

    fn ui_state(&mut self) -> ui::UiState<'_> {
        let input = self.input_prompt();
        let image_state = self.image_state.as_mut();
        let marker_popup = self.marker_list.as_ref().map(|list| ui::MarkerPopup {
            items: list
                .filtered_indices
                .iter()
                .filter_map(|&index| list.entries.get(index))
                .map(|entry| ui::MarkerListItem {
                    name: entry.name.clone(),
                    path: entry.path.to_string_lossy().to_string(),
                })
                .collect(),
            selected: list.selected,
        });
        let program_popup = self.program_list.as_ref().map(|list| ui::ProgramPopup {
            items: list
                .filtered_indices
                .iter()
                .filter_map(|&index| list.entries.get(index))
                .map(|entry| ui::ProgramListItem {
                    name: entry.name.clone(),
                    path: entry.path.to_string_lossy().to_string(),
                })
                .collect(),
            selected: list.selected,
            filter: list.filter.clone(),
        });
        ui::UiState {
            config: &self.config,
            parent: &self.parent_entries,
            current: &self.current_entries,
            current_indices: &self.filtered_indices,
            selected: self.selected,
            preview: self.preview.as_ref(),
            highlighted_preview: self.highlighted_preview.as_ref(),
            show_metadata: self.show_metadata,
            show_permissions: self.show_permissions,
            show_dates: self.show_dates,
            show_owner: self.show_owner,
            show_list_permissions: self.show_list_permissions,
            show_list_owner: self.show_list_owner,
            metadata: self.preview.as_ref().and_then(|preview| preview.metadata.as_ref()),
            image_state,
            input,
            marker_popup,
            program_popup,
        }
    }

    fn input_prompt(&self) -> Option<ui::InputPrompt> {
        match &self.mode {
            Mode::Input(input) => {
                let value = if matches!(input.action.clone(), InputAction::ConfirmDelete) {
                    "y/n".to_string()
                } else {
                    format!("{}|", input.buffer)
                };
                Some(ui::InputPrompt {
                    title: input.title().to_string(),
                    value,
                })
            }
            Mode::MarkerList => None,
            Mode::ProgramList => None,
            Mode::Normal => None,
        }
    }

    fn clear_preview(&mut self) {
        self.preview = None;
        self.highlighted_preview = None;
        self.image_state = None;
        self.preview_pending = false;
    }

    fn select_up(&mut self) -> bool {
        if self.selected > 0 {
            self.selected -= 1;
            self.clear_preview();
            return true;
        }
        false
    }

    fn select_down(&mut self) -> bool {
        if self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
            self.clear_preview();
            return true;
        }
        false
    }

    fn activate_selected(&mut self, tx: &tokio_mpsc::UnboundedSender<AppEvent>) -> bool {
        let Some(entry) = self.selected_entry() else {
            return false;
        };
        if entry.is_dir {
            self.current_dir = entry.path.clone();
            self.selected = 0;
            self.pending_selection = None;
            self.clear_preview();
            self.refresh_dirs(tx);
            return true;
        }
        spawn_open(entry.path.clone());
        false
    }

    fn navigate_parent(&mut self, tx: &tokio_mpsc::UnboundedSender<AppEvent>) -> bool {
        let Some(parent) = self.current_dir.parent() else {
            return false;
        };
        let previous = self.current_dir.clone();
        self.current_dir = parent.to_path_buf();
        self.selected = 0;
        self.pending_selection = Some(previous);
        self.clear_preview();
        self.refresh_dirs(tx);
        true
    }

    fn request_preview(&mut self, tx: &tokio_mpsc::UnboundedSender<AppEvent>) {
        let Some(entry) = self.selected_entry() else {
            self.preview_pending = false;
            self.preview = None;
            return;
        };
        let path = entry.path.clone();
        self.preview_request_id = self.preview_request_id.wrapping_add(1);
        let request_id = self.preview_request_id;
        let config = self.config.clone();
        let tx = tx.clone();
        self.preview_pending = true;
        tokio::spawn(async move {
            let result = core::load_preview(&path, &config).await;
            let _ = tx.send(AppEvent::Preview {
                id: request_id,
                result,
            });
        });
    }

    fn apply_preview(&mut self, id: u64, result: Result<Preview, core::CoreError>) -> bool {
        if id != self.preview_request_id {
            return false;
        }
        self.preview_pending = false;
        match result {
            Ok(mut preview) => {
                self.image_state = None;
                self.highlighted_preview = ui::highlight_preview(&preview);
                if let Some(image) = preview.image.take() {
                    self.image_version = self.image_version.wrapping_add(1);
                    let version = self.image_version;
                    let protocol = self.picker.new_resize_protocol(image);
                    self.image_state = Some(ui::ThreadProtocol::new(
                        self.image_worker_tx.clone(),
                        protocol,
                        version,
                    ));
                }
                self.preview = Some(preview);
            }
            Err(_) => {
                self.preview = None;
                self.highlighted_preview = None;
                self.image_state = None;
            }
        }
        true
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        let index = *self.filtered_indices.get(self.selected)?;
        self.current_entries.get(index)
    }

    fn refresh_dirs(&mut self, tx: &tokio_mpsc::UnboundedSender<AppEvent>) {
        self.listing_id = self.listing_id.wrapping_add(1);
        let listing_id = self.listing_id;
        self.current_entries.clear();
        self.parent_entries.clear();
        self.filtered_indices.clear();
        self.clear_preview();
        spawn_dir_listing(tx.clone(), DirTarget::Current, listing_id, self.current_dir.clone());
        if let Some(parent) = self.current_dir.parent() {
            spawn_dir_listing(tx.clone(), DirTarget::Parent, listing_id, parent.to_path_buf());
        }
    }

    fn apply_filter(&mut self, preferred: Option<PathBuf>) -> bool {
        let had_entries = !self.filtered_indices.is_empty();
        let previous_selected = self.selected;
        let raw_query = self.filter.trim();
        let query_lower = raw_query.to_ascii_lowercase();
        let regex = if raw_query.is_empty() {
            None
        } else {
            RegexBuilder::new(raw_query)
                .case_insensitive(true)
                .build()
                .ok()
        };
        self.filtered_indices = if raw_query.is_empty() {
            (0..self.current_entries.len()).collect()
        } else {
            self.current_entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    if let Some(regex) = regex.as_ref() {
                        regex.is_match(entry.name.as_str())
                    } else {
                        entry.name.to_ascii_lowercase().contains(query_lower.as_str())
                    }
                })
                .map(|(index, _)| index)
                .collect()
        };
        let mut new_selected = 0usize;
        if let Some(preferred) = preferred {
            if let Some(pos) = self
                .filtered_indices
                .iter()
                .position(|&index| self.current_entries[index].path == preferred)
            {
                new_selected = pos;
            }
        }
        let changed = if self.filtered_indices.is_empty() {
            had_entries
        } else {
            previous_selected != new_selected
        };
        self.selected = new_selected;
        if self.filtered_indices.is_empty() {
            self.selected = 0;
        }
        changed
    }

    fn update_filter(&mut self, value: String) -> bool {
        let selected_path = self.selected_entry().map(|entry| entry.path.clone());
        self.filter = value;
        self.apply_filter(selected_path)
    }

    fn clear_filter(&mut self) -> bool {
        let selected_path = self.selected_entry().map(|entry| entry.path.clone());
        self.filter.clear();
        self.apply_filter(selected_path)
    }

    fn update_marker_filter(&mut self, value: String) {
        if let Some(list) = self.marker_list.as_mut() {
            list.update_filter(value);
        }
    }

    fn clear_marker_filter(&mut self) {
        if let Some(list) = self.marker_list.as_mut() {
            list.clear_filter();
        }
    }

    fn open_marker_list(&mut self) {
        self.marker_list = Some(MarkerListState::new(&self.markers));
        self.mode = Mode::MarkerList;
    }

    fn sync_marker_list(&mut self, preferred: Option<&str>) {
        if let Some(list) = self.marker_list.as_mut() {
            list.sync(&self.markers, preferred);
        }
    }

    fn open_program_list(&mut self) {
        self.pending_prefix = None;
        self.program_list = Some(ProgramListState::new(&self.programs));
        self.mode = Mode::ProgramList;
    }

    fn resolve_program_path(&self, name: &str) -> PathBuf {
        self.programs
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.path.clone())
            .unwrap_or_else(|| PathBuf::from(name))
    }

    fn open_with_quick(&self, key: char) -> Option<SuspendAction> {
        let digit = key.to_digit(10)?;
        let program = self
            .config
            .open_with
            .quick
            .get(&digit.to_string())?;
        let target = self.selected_entry()?;
        Some(SuspendAction::OpenWith {
            program: self.resolve_program_path(program),
            path: target.path.clone(),
            cwd: self.current_dir.clone(),
        })
    }
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

fn scan_programs() -> Vec<ProgramEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let Some(path_var) = env::var_os("PATH") else {
        return entries;
    };
    for dir in env::split_paths(&path_var) {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !is_executable(&path) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if seen.insert(name.clone()) {
                entries.push(ProgramEntry { name, path });
            }
        }
    }
    entries.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    entries
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return metadata.permissions().mode() & 0o111 != 0;
    }
    #[cfg(windows)]
    {
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        return matches!(ext.as_str(), "exe" | "cmd" | "bat" | "com");
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

struct InputHandler;

impl InputHandler {
    fn handle_key(
        app: &mut App,
        key: KeyEvent,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        match &mut app.mode {
            Mode::Input(_) => Self::handle_input(app, key, tx),
            Mode::MarkerList => Self::handle_marker_list(app, key, tx),
            Mode::ProgramList => Self::handle_program_list(app, key, tx),
            Mode::Normal => Self::handle_normal(app, key, tx),
        }
    }

    fn handle_normal(
        app: &mut App,
        key: KeyEvent,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        if let Some(prefix) = app.pending_prefix.take() {
            return Self::handle_prefix(app, prefix, key, tx);
        }
        Self::handle_normal_key(app, key, tx)
    }

    fn handle_prefix(
        app: &mut App,
        prefix: PendingPrefix,
        key: KeyEvent,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        let mut effect = InputEffect::default();
        if matches!(key.code, KeyCode::Esc) {
            return effect;
        }
        match prefix {
            PendingPrefix::Add => {
                if matches!(key.code, KeyCode::Char('d')) {
                    Self::start_input(app, InputAction::AddDir);
                    effect.redraw = true;
                    return effect;
                }
                Self::start_input(app, InputAction::AddFile);
                effect.redraw = true;
                let input_effect = Self::handle_input(app, key, tx);
                return InputEffect {
                    exit: input_effect.exit,
                    redraw: effect.redraw || input_effect.redraw,
                    request_preview: input_effect.request_preview,
                    suspend: input_effect.suspend,
                };
            }
            PendingPrefix::Settings => match key.code {
                KeyCode::Char('r') => {
                    app.show_permissions = !app.show_permissions;
                    app.show_metadata = true;
                    effect.redraw = true;
                    return effect;
                }
                KeyCode::Char('d') => {
                    app.show_dates = !app.show_dates;
                    app.show_metadata = true;
                    effect.redraw = true;
                    return effect;
                }
                KeyCode::Char('o') => {
                    app.show_owner = !app.show_owner;
                    app.show_metadata = true;
                    effect.redraw = true;
                    return effect;
                }
                KeyCode::Char('m') => {
                    app.show_metadata = !app.show_metadata;
                    effect.redraw = true;
                    return effect;
                }
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    app.show_hidden = !app.show_hidden;
                    app.pending_selection = app.selected_entry().map(|entry| entry.path.clone());
                    app.refresh_dirs(tx);
                    effect.redraw = true;
                    return effect;
                }
                KeyCode::Esc => return effect,
                _ => return Self::handle_normal_key(app, key, tx),
            },
            PendingPrefix::Copy => {
                if matches!(key.code, KeyCode::Char('p')) {
                    if let Some(entry) = app.selected_entry() {
                        spawn_copy_path(entry.path.clone());
                    }
                    return effect;
                }
                return Self::handle_normal_key(app, key, tx);
            }
            PendingPrefix::View => match key.code {
                KeyCode::Char('p') => {
                    app.show_list_permissions = !app.show_list_permissions;
                    effect.redraw = true;
                    return effect;
                }
                KeyCode::Char('o') => {
                    app.show_list_owner = !app.show_list_owner;
                    effect.redraw = true;
                    return effect;
                }
                _ => return Self::handle_normal_key(app, key, tx),
            },
            PendingPrefix::Delete => {
                if matches!(key.code, KeyCode::Char('d')) {
                    if app.selected_entry().is_some() {
                        Self::start_input(app, InputAction::ConfirmDelete);
                        effect.redraw = true;
                    }
                    return effect;
                }
                return Self::handle_normal_key(app, key, tx);
            }
            PendingPrefix::OpenWith => {
                if let KeyCode::Char(ch) = key.code {
                    if ch.is_ascii_digit() {
                        effect.suspend = app.open_with_quick(ch);
                        return effect;
                    }
                }
                return Self::handle_normal_key(app, key, tx);
            }
        }
    }

    fn handle_normal_key(
        app: &mut App,
        key: KeyEvent,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        let mut effect = InputEffect::default();
        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            app.open_program_list();
            effect.redraw = true;
            return effect;
        }
        if key.code == KeyCode::Char('O') {
            app.open_program_list();
            effect.redraw = true;
            return effect;
        }
        match key.code {
            KeyCode::Char('q') => effect.exit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.select_up() {
                    effect.redraw = true;
                    effect.request_preview = true;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.select_down() {
                    effect.redraw = true;
                    effect.request_preview = true;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if app.navigate_parent(tx) {
                    effect.redraw = true;
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                if app.activate_selected(tx) {
                    effect.redraw = true;
                }
            }
            KeyCode::Char('/') => {
                Self::start_input(app, InputAction::Search);
                effect.redraw = true;
            }
            KeyCode::Char('a') => {
                app.pending_prefix = Some(PendingPrefix::Add);
            }
            KeyCode::Char('r') => {
                if app.selected_entry().is_some() {
                    Self::start_input(app, InputAction::Rename);
                    effect.redraw = true;
                }
            }
            KeyCode::Char('d') => {
                app.pending_prefix = Some(PendingPrefix::Delete);
            }
            KeyCode::Char('m') => {
                Self::start_input(app, InputAction::MarkerSet);
                effect.redraw = true;
            }
            KeyCode::Char('M') => {
                app.open_marker_list();
                effect.redraw = true;
            }
            KeyCode::Char('g') => {
                Self::start_input(app, InputAction::MarkerJump);
                effect.redraw = true;
            }
            KeyCode::Char('s') => {
                app.pending_prefix = Some(PendingPrefix::Settings);
            }
            KeyCode::Char('v') => {
                app.pending_prefix = Some(PendingPrefix::View);
            }
            KeyCode::Char('c') => {
                Self::copy_selection(app, ClipboardOp::Copy);
                app.pending_prefix = Some(PendingPrefix::Copy);
            }
            KeyCode::Char('x') => {
                Self::copy_selection(app, ClipboardOp::Cut);
            }
            KeyCode::Char('p') => {
                Self::paste_selection(app, tx);
            }
            KeyCode::Char('o') => {
                app.pending_prefix = Some(PendingPrefix::OpenWith);
            }
            KeyCode::Char('t') => {
                effect.suspend = Some(SuspendAction::Shell(app.current_dir.clone()));
            }
            _ => {}
        }
        effect
    }

    fn handle_input(
        app: &mut App,
        key: KeyEvent,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        let mut effect = InputEffect::default();
        let mode = std::mem::replace(&mut app.mode, Mode::Normal);
        let mut input = match mode {
            Mode::Input(input) => input,
            other => {
                app.mode = other;
                return effect;
            }
        };

        let mut keep_input = true;
        match input.action.clone() {
            InputAction::Search => match key.code {
                KeyCode::Esc => {
                    let selection_changed = app.clear_filter();
                    keep_input = false;
                    effect.redraw = true;
                    if selection_changed {
                        app.clear_preview();
                        effect.request_preview = true;
                    }
                }
                KeyCode::Enter => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    let selection_changed = app.update_filter(input.buffer.clone());
                    effect.redraw = true;
                    if selection_changed {
                        app.clear_preview();
                        effect.request_preview = true;
                    }
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    let selection_changed = app.update_filter(input.buffer.clone());
                    effect.redraw = true;
                    if selection_changed {
                        app.clear_preview();
                        effect.request_preview = true;
                    }
                }
                _ => {}
            },
            InputAction::MarkerSearch => match key.code {
                KeyCode::Esc => {
                    app.clear_marker_filter();
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    app.update_marker_filter(input.buffer.clone());
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    app.update_marker_filter(input.buffer.clone());
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::AddFile | InputAction::AddDir => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    if !input.buffer.trim().is_empty() {
                        let name = input.buffer.trim().to_string();
                        let path = app.current_dir.join(&name);
                        let select = Some(path.clone());
                        let is_dir = matches!(input.action, InputAction::AddDir);
                        if is_dir {
                            let path = path.clone();
                            spawn_refresh(tx, select, async move { core::create_dir(&path).await });
                        } else {
                            let path = path.clone();
                            spawn_refresh(tx, select, async move { core::create_file(&path).await });
                        }
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::Rename => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let new_name = input.buffer.trim();
                    if !new_name.is_empty() {
                        if let Some(entry) = app.selected_entry() {
                            let src = entry.path.clone();
                            let dest = src.with_file_name(new_name);
                            if src != dest {
                                spawn_refresh(tx, Some(dest.clone()), async move {
                                    core::rename_path(&src, &dest).await
                                });
                            }
                        }
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::MarkerSet => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let name = input.buffer.trim();
                    if !name.is_empty() {
                        let name = name.to_string();
                        app.markers.set(name.clone(), app.current_dir.clone());
                        let save_task = app.markers.save_task();
                        tokio::spawn(save_task);
                        app.sync_marker_list(Some(&name));
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::MarkerJump => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let name = input.buffer.trim();
                    if let Some(path) = app.markers.get(name).cloned() {
                        app.current_dir = path;
                        app.pending_selection = None;
                        app.selected = 0;
                        app.clear_preview();
                        app.refresh_dirs(tx);
                        effect.redraw = true;
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::MarkerRename { name } => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let new_name = input.buffer.trim();
                    if !new_name.is_empty() {
                        let new_name = new_name.to_string();
                        if app.markers.rename(&name, new_name.clone()) {
                            let save_task = app.markers.save_task();
                            tokio::spawn(save_task);
                            app.sync_marker_list(Some(&new_name));
                        }
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::MarkerEditPath { name } => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let path = input.buffer.trim();
                    if !path.is_empty() {
                        app.markers.set(name.clone(), PathBuf::from(path));
                        let save_task = app.markers.save_task();
                        tokio::spawn(save_task);
                        app.sync_marker_list(Some(&name));
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::MarkerCreateName => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let name = input.buffer.trim();
                    if !name.is_empty() {
                        let buffer = app.current_dir.to_string_lossy().to_string();
                        input = InputState::new(
                            InputAction::MarkerCreatePath {
                                name: name.to_string(),
                            },
                            buffer,
                        );
                        effect.redraw = true;
                    } else {
                        keep_input = false;
                        effect.redraw = true;
                    }
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::MarkerCreatePath { name } => match key.code {
                KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Enter => {
                    let path = input.buffer.trim();
                    if !path.is_empty() {
                        app.markers.set(name.clone(), PathBuf::from(path));
                        let save_task = app.markers.save_task();
                        tokio::spawn(save_task);
                        app.sync_marker_list(Some(&name));
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Backspace => {
                    input.buffer.pop();
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.buffer.push(ch);
                    effect.redraw = true;
                }
                _ => {}
            },
            InputAction::ConfirmDelete => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(entry) = app.selected_entry() {
                        let path = entry.path.clone();
                        spawn_refresh(tx, None, async move { core::remove_path(&path).await });
                    }
                    keep_input = false;
                    effect.redraw = true;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    keep_input = false;
                    effect.redraw = true;
                }
                _ => {}
            },
        }

        if keep_input {
            app.mode = Mode::Input(input);
        } else if app.marker_list.is_some() {
            app.mode = Mode::MarkerList;
        } else if app.program_list.is_some() {
            app.mode = Mode::ProgramList;
        } else {
            app.mode = Mode::Normal;
        }
        effect
    }

    fn handle_marker_list(
        app: &mut App,
        key: KeyEvent,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        let mut effect = InputEffect::default();
        enum MarkerListAction {
            Jump(PathBuf),
            StartInput(InputAction),
            Delete(String),
        }

        let mut action: Option<MarkerListAction> = None;
        let mut close = false;
        {
            let Some(list) = app.marker_list.as_mut() else {
                app.mode = Mode::Normal;
                return effect;
            };
            match key.code {
                KeyCode::Esc => {
                    close = true;
                    effect.redraw = true;
                }
                KeyCode::Up => {
                    if list.selected > 0 {
                        list.selected -= 1;
                        effect.redraw = true;
                    }
                }
                KeyCode::Down => {
                    if list.selected + 1 < list.filtered_indices.len() {
                        list.selected += 1;
                        effect.redraw = true;
                    }
                }
                KeyCode::Enter => {
                    if let Some(entry) = list.selected_entry() {
                        action = Some(MarkerListAction::Jump(entry.path.clone()));
                    }
                    close = true;
                    effect.redraw = true;
                }
                KeyCode::Char('r') => {
                    if let Some(entry) = list.selected_entry() {
                        action = Some(MarkerListAction::StartInput(InputAction::MarkerRename {
                            name: entry.name.clone(),
                        }));
                        effect.redraw = true;
                    }
                }
                KeyCode::Char('e') => {
                    if let Some(entry) = list.selected_entry() {
                        action = Some(MarkerListAction::StartInput(InputAction::MarkerEditPath {
                            name: entry.name.clone(),
                        }));
                        effect.redraw = true;
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(entry) = list.selected_entry() {
                        action = Some(MarkerListAction::Delete(entry.name.clone()));
                        effect.redraw = true;
                    }
                }
                KeyCode::Char('a') => {
                    action = Some(MarkerListAction::StartInput(InputAction::MarkerCreateName));
                    effect.redraw = true;
                }
                KeyCode::Char('/') => {
                    action = Some(MarkerListAction::StartInput(InputAction::MarkerSearch));
                    effect.redraw = true;
                }
                _ => {}
            }
        }

        match action {
            Some(MarkerListAction::Jump(path)) => {
                app.current_dir = path;
                app.pending_selection = None;
                app.selected = 0;
                app.clear_preview();
                app.refresh_dirs(tx);
            }
            Some(MarkerListAction::StartInput(action)) => {
                Self::start_input(app, action);
            }
            Some(MarkerListAction::Delete(name)) => {
                if app.markers.remove(&name) {
                    let save_task = app.markers.save_task();
                    tokio::spawn(save_task);
                    app.sync_marker_list(None);
                }
            }
            None => {}
        }

        if close {
            app.marker_list = None;
            app.mode = Mode::Normal;
        }
        effect
    }

    fn handle_program_list(
        app: &mut App,
        key: KeyEvent,
        _tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> InputEffect {
        let mut effect = InputEffect::default();
        let target_path = app.selected_entry().map(|entry| entry.path.clone());
        let cwd = app.current_dir.clone();
        let mut action: Option<SuspendAction> = None;
        let mut close = false;
        {
            let Some(list) = app.program_list.as_mut() else {
                app.mode = Mode::Normal;
                return effect;
            };
            match key.code {
                KeyCode::Esc => {
                    close = true;
                    effect.redraw = true;
                }
                KeyCode::Up => {
                    if list.selected > 0 {
                        list.selected -= 1;
                        effect.redraw = true;
                    }
                }
                KeyCode::Down => {
                    if list.selected + 1 < list.filtered_indices.len() {
                        list.selected += 1;
                        effect.redraw = true;
                    }
                }
                KeyCode::Enter => {
                    if let (Some(program), Some(target)) =
                        (list.selected_entry(), target_path.as_ref())
                    {
                        action = Some(SuspendAction::OpenWith {
                            program: program.path.clone(),
                            path: target.clone(),
                            cwd: cwd.clone(),
                        });
                        close = true;
                        effect.redraw = true;
                    }
                }
                KeyCode::Backspace => {
                    let mut next = list.filter.clone();
                    next.pop();
                    list.update_filter(next);
                    effect.redraw = true;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    let mut next = list.filter.clone();
                    next.push(ch);
                    list.update_filter(next);
                    effect.redraw = true;
                }
                _ => {}
            }
        }

        if close {
            app.program_list = None;
            app.mode = Mode::Normal;
        }

        effect.suspend = action;
        effect
    }

    fn start_input(app: &mut App, action: InputAction) {
        let buffer = match &action {
            InputAction::Search => app.filter.clone(),
            InputAction::MarkerSearch => app
                .marker_list
                .as_ref()
                .map(|list| list.filter.clone())
                .unwrap_or_default(),
            InputAction::Rename => app
                .selected_entry()
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
            InputAction::MarkerRename { name } => name.clone(),
            InputAction::MarkerEditPath { name } => app
                .markers
                .get(name)
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
            InputAction::MarkerCreatePath { .. } => app.current_dir.to_string_lossy().to_string(),
            _ => String::new(),
        };
        app.pending_prefix = None;
        app.mode = Mode::Input(InputState::new(action, buffer));
    }

    fn copy_selection(app: &mut App, op: ClipboardOp) {
        if let Some(entry) = app.selected_entry() {
            app.clipboard = Some(ClipboardEntry {
                op,
                path: entry.path.clone(),
            });
        }
    }

    fn paste_selection(app: &mut App, tx: &tokio_mpsc::UnboundedSender<AppEvent>) {
        let Some(clipboard) = app.clipboard.clone() else {
            return;
        };
        let Some(file_name) = clipboard.path.file_name() else {
            return;
        };
        let dest = app.current_dir.join(file_name);
        let select = Some(dest.clone());
        match clipboard.op {
            ClipboardOp::Cut => {
                let src = clipboard.path.clone();
                let dest = dest.clone();
                spawn_refresh(tx, select, async move { core::rename_path(&src, &dest).await });
                app.clipboard = None;
            }
            ClipboardOp::Copy => {
                let src = clipboard.path.clone();
                let dest = dest.clone();
                spawn_refresh(tx, select, async move { core::copy_recursively(&src, &dest).await });
            }
        }
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self, Box<dyn Error>> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

fn spawn_input(
    tx: tokio_mpsc::UnboundedSender<AppEvent>,
    paused: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        if paused.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => match event::read() {
                Ok(event) => {
                    if tx.send(AppEvent::Input(event)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },
            Ok(false) => continue,
            Err(_) => break,
        }
    })
}

fn spawn_dir_listing(
    tx: tokio_mpsc::UnboundedSender<AppEvent>,
    target: DirTarget,
    id: u64,
    path: PathBuf,
) {
    tokio::spawn(async move {
        let stream = match core::read_dir_stream(&path).await {
            Ok(stream) => stream,
            Err(_) => {
                let _ = tx.send(AppEvent::DirEntries {
                    id,
                    target,
                    entries: Vec::new(),
                    done: true,
                });
                return;
            }
        };
        let mut batch = Vec::with_capacity(DIR_BATCH_SIZE);
        let mut stream = stream;
        while let Some(entry) = stream.next().await {
            if let Ok(entry) = entry {
                if let Ok(file_entry) = FileEntry::from_dir_entry(entry).await {
                    batch.push(file_entry);
                }
            }
            if batch.len() >= DIR_BATCH_SIZE {
                let entries = std::mem::take(&mut batch);
                let _ = tx.send(AppEvent::DirEntries {
                    id,
                    target,
                    entries,
                    done: false,
                });
            }
        }
        if !batch.is_empty() {
            let _ = tx.send(AppEvent::DirEntries {
                id,
                target,
                entries: batch,
                done: false,
            });
        }
        let _ = tx.send(AppEvent::DirEntries {
            id,
            target,
            entries: Vec::new(),
            done: true,
        });
    });
}

fn spawn_image_worker(
    tx: tokio_mpsc::UnboundedSender<AppEvent>,
) -> Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)> {
    let (worker_tx, worker_rx) =
        mpsc::channel::<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>();
    thread::spawn(move || {
        while let Ok((version, mut protocol, resize, rect)) = worker_rx.recv() {
            protocol.resize_encode(&resize, None, rect);
            let _ = tx.send(AppEvent::ImageReady { version, protocol });
        }
    });
    worker_tx
}

fn spawn_refresh<F>(
    tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    select: Option<PathBuf>,
    action: F,
) where
    F: Future<Output = std::io::Result<()>> + Send + 'static,
{
    let tx = tx.clone();
    tokio::spawn(async move {
        let _ = action.await;
        let _ = tx.send(AppEvent::Action(ActionResult::Refresh { select }));
    });
}

fn spawn_open(path: PathBuf) {
    tokio::task::spawn_blocking(move || {
        let _ = open::that(path);
    });
}

fn spawn_copy_path(path: PathBuf) {
    let value = path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(value);
        }
    });
}

fn suspend_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;
    Ok(())
}

fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;
    Ok(())
}

fn run_shell(path: &Path) -> io::Result<()> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    Command::new(shell).current_dir(path).status().map(|_| ())
}

fn run_program(program: &Path, path: &Path, cwd: &Path) -> io::Result<()> {
    Command::new(program).current_dir(cwd).arg(path).status().map(|_| ())
}

fn run_suspend_action(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    paused: &Arc<AtomicBool>,
    action: SuspendAction,
) -> io::Result<()> {
    paused.store(true, Ordering::SeqCst);
    let suspend_result = suspend_terminal();
    if let Err(err) = suspend_result {
        paused.store(false, Ordering::SeqCst);
        return Err(err);
    }

    let action_result = match action {
        SuspendAction::Shell(path) => run_shell(&path),
        SuspendAction::OpenWith { program, path, cwd } => run_program(&program, &path, &cwd),
    };

    let resume_result = resume_terminal(terminal);
    paused.store(false, Ordering::SeqCst);
    if let Err(err) = resume_result {
        return Err(err);
    }

    action_result
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;
    let guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut picker = Picker::new((8, 12));
    #[cfg(unix)]
    {
        if let Ok(found) = Picker::from_termios() {
            picker = found;
        }
    }
    if io::stdin().is_terminal() {
        picker.guess_protocol();
    }

    let (tx, mut rx) = tokio_mpsc::unbounded_channel();
    let input_paused = Arc::new(AtomicBool::new(false));
    let _input_handle = spawn_input(tx.clone(), input_paused.clone());
    let image_worker_tx = spawn_image_worker(tx.clone());

    let mut app = App::new(config, picker, image_worker_tx, &tx).await?;
    terminal.draw(|frame| ui::render(frame, app.ui_state()))?;

    while let Some(event) = rx.recv().await {
        let mut redraw = false;
        let mut request_preview = false;
        match event {
            AppEvent::Input(Event::Key(key)) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let effect = InputHandler::handle_key(&mut app, key, &tx);
                if let Some(action) = effect.suspend {
                    if let Err(err) = run_suspend_action(&mut terminal, &input_paused, action) {
                        eprintln!("Failed to run command: {err}");
                    }
                    redraw = true;
                }
                if effect.exit {
                    break;
                }
                if effect.redraw {
                    redraw = true;
                }
                if effect.request_preview {
                    request_preview = true;
                }
            }
            AppEvent::Input(Event::Resize(_, _)) => {
                redraw = true;
            }
            AppEvent::Preview { id, result } => {
                if app.apply_preview(id, result) {
                    redraw = true;
                }
            }
            AppEvent::DirEntries {
                id,
                target,
                entries,
                done,
            } => {
                if id != app.listing_id {
                    continue;
                }
                let selected_path = app.selected_entry().map(|entry| entry.path.clone());
                let list = match target {
                    DirTarget::Parent => &mut app.parent_entries,
                    DirTarget::Current => &mut app.current_entries,
                };
                let mut entries = entries;
                if !app.show_hidden {
                    entries.retain(|entry| !is_hidden_name(&entry.name));
                }
                list.extend(entries);
                if done {
                    core::sort_entries(list);
                }
                if matches!(target, DirTarget::Current) {
                    let preferred = if done {
                        app.pending_selection.take().or(selected_path)
                    } else {
                        selected_path
                    };
                    let selection_changed = app.apply_filter(preferred);
                    if selection_changed {
                        app.clear_preview();
                        request_preview = true;
                    }
                    if !app.preview_pending && app.preview.is_none() && !app.filtered_indices.is_empty()
                    {
                        request_preview = true;
                    }
                }
                redraw = true;
            }
            AppEvent::ImageReady { version, protocol } => {
                if let Some(image_state) = app.image_state.as_mut() {
                    if image_state.version() == version {
                        image_state.set_inner(protocol);
                        redraw = true;
                    }
                }
            }
            AppEvent::Action(ActionResult::Refresh { select }) => {
                if let Some(path) = select {
                    app.pending_selection = Some(path);
                }
                app.refresh_dirs(&tx);
                redraw = true;
            }
            _ => {}
        }

        if request_preview {
            app.request_preview(&tx);
        }

        if redraw {
            terminal.draw(|frame| ui::render(frame, app.ui_state()))?;
        }
    }

    drop(terminal);
    drop(guard);

    Ok(())
}
