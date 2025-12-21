mod config;
mod core;
mod preview;
mod security;
mod ui;

use crate::config::Config;
use crate::core::FileEntry;
use crate::preview::Preview;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, event, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::Resize;
use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_stream::StreamExt;

const DIR_BATCH_SIZE: usize = 512;

#[derive(Clone, Copy)]
enum DirTarget {
    Parent,
    Current,
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
}

struct App {
    config: Config,
    picker: Picker,
    current_dir: PathBuf,
    parent_entries: Vec<FileEntry>,
    current_entries: Vec<FileEntry>,
    selected: usize,
    preview: Option<Preview>,
    show_metadata: bool,
    preview_request_id: u64,
    preview_pending: bool,
    listing_id: u64,
    pending_selection: Option<PathBuf>,
    image_state: Option<ui::ThreadProtocol>,
    image_version: u64,
    image_worker_tx: Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>,
}

impl App {
    async fn new(
        config: Config,
        picker: Picker,
        image_worker_tx: Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self, core::CoreError> {
        let current_dir = env::current_dir()?;
        let mut app = Self {
            show_metadata: config.metadata_bar.enabled,
            config,
            picker,
            current_dir,
            parent_entries: Vec::new(),
            current_entries: Vec::new(),
            selected: 0,
            preview: None,
            preview_request_id: 0,
            preview_pending: false,
            listing_id: 0,
            pending_selection: None,
            image_state: None,
            image_version: 0,
            image_worker_tx,
        };
        app.refresh_dirs(tx).await?;
        Ok(app)
    }

    fn ui_state(&mut self) -> ui::UiState<'_> {
        ui::UiState {
            config: &self.config,
            parent: &self.parent_entries,
            current: &self.current_entries,
            selected: self.selected,
            preview: self.preview.as_ref(),
            show_metadata: self.show_metadata,
            metadata: self.preview.as_ref().and_then(|preview| preview.metadata.as_ref()),
            image_state: self.image_state.as_mut(),
        }
    }

    fn clear_preview(&mut self) {
        self.preview = None;
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
        if self.selected + 1 < self.current_entries.len() {
            self.selected += 1;
            self.clear_preview();
            return true;
        }
        false
    }

    async fn enter_selected(
        &mut self,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> Result<bool, core::CoreError> {
        let Some(entry) = self.selected_entry() else {
            return Ok(false);
        };
        if !entry.is_dir {
            return Ok(false);
        }
        self.current_dir = entry.path.clone();
        self.selected = 0;
        self.pending_selection = None;
        self.clear_preview();
        self.refresh_dirs(tx).await?;
        Ok(true)
    }

    async fn navigate_parent(
        &mut self,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> Result<bool, core::CoreError> {
        let Some(parent) = self.current_dir.parent() else {
            return Ok(false);
        };
        let previous = self.current_dir.clone();
        self.current_dir = parent.to_path_buf();
        self.selected = 0;
        self.pending_selection = Some(previous);
        self.clear_preview();
        self.refresh_dirs(tx).await?;
        Ok(true)
    }

    fn toggle_metadata(&mut self) {
        self.show_metadata = !self.show_metadata;
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
                self.image_state = None;
            }
        }
        true
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        self.current_entries.get(self.selected)
    }

    async fn refresh_dirs(
        &mut self,
        tx: &tokio_mpsc::UnboundedSender<AppEvent>,
    ) -> Result<(), core::CoreError> {
        self.listing_id = self.listing_id.wrapping_add(1);
        let listing_id = self.listing_id;
        self.current_entries.clear();
        self.parent_entries.clear();
        self.clear_preview();
        spawn_dir_listing(tx.clone(), DirTarget::Current, listing_id, self.current_dir.clone());
        if let Some(parent) = self.current_dir.parent() {
            spawn_dir_listing(tx.clone(), DirTarget::Parent, listing_id, parent.to_path_buf());
        }
        Ok(())
    }

    fn clamp_selection(&mut self) {
        if self.current_entries.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.current_entries.len() {
            self.selected = self.current_entries.len() - 1;
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

fn spawn_input(tx: tokio_mpsc::UnboundedSender<AppEvent>) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        match event::read() {
            Ok(event) => {
                if tx.send(AppEvent::Input(event)).is_err() {
                    break;
                }
            }
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let (tx, mut rx) = tokio_mpsc::unbounded_channel();
    let _input_handle = spawn_input(tx.clone());
    let image_worker_tx = spawn_image_worker(tx.clone());

    let mut picker = Picker::new((8, 12));
    #[cfg(unix)]
    {
        if let Ok(found) = Picker::from_termios() {
            picker = found;
        }
    }
    picker.guess_protocol();

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
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.select_up() {
                            redraw = true;
                            request_preview = true;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if app.select_down() {
                            redraw = true;
                            request_preview = true;
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if app.navigate_parent(&tx).await? {
                            redraw = true;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                        if app.enter_selected(&tx).await? {
                            redraw = true;
                        }
                    }
                    KeyCode::Char('m') => {
                        app.toggle_metadata();
                        redraw = true;
                    }
                    _ => {}
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
                let list = match target {
                    DirTarget::Parent => &mut app.parent_entries,
                    DirTarget::Current => &mut app.current_entries,
                };
                list.extend(entries);
                if done {
                    core::sort_entries(list);
                    if matches!(target, DirTarget::Current) {
                        if let Some(path) = app.pending_selection.take() {
                            if let Some(index) = app
                                .current_entries
                                .iter()
                                .position(|entry| entry.path == path)
                            {
                                app.selected = index;
                            }
                        }
                    }
                }
                if matches!(target, DirTarget::Current) {
                    app.clamp_selection();
                    if !app.preview_pending
                        && app.preview.is_none()
                        && !app.current_entries.is_empty()
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
            _ => {}
        }

        if request_preview {
            app.request_preview(&tx);
        }

        if redraw {
            terminal.draw(|frame| ui::render(frame, app.ui_state()))?;
        }
    }

    Ok(())
}
