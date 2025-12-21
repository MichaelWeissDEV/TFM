use crate::config::Config;
use crate::core::FileEntry;
use crate::preview::{FileMetadata, Preview, PreviewData};
use crate::security::MismatchStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget};
use ratatui::Frame;
use ratatui_image::{protocol::StatefulProtocol, Resize};
use std::sync::mpsc::Sender;

pub struct ThreadImage {
    resize: Resize,
}

impl ThreadImage {
    pub fn new() -> Self {
        Self { resize: Resize::Fit }
    }

    pub fn resize(mut self, resize: Resize) -> Self {
        self.resize = resize;
        self
    }
}

pub struct ThreadProtocol {
    inner: Option<Box<dyn StatefulProtocol>>,
    tx: Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>,
    version: u64,
}

impl ThreadProtocol {
    pub fn new(
        tx: Sender<(u64, Box<dyn StatefulProtocol>, Resize, Rect)>,
        inner: Box<dyn StatefulProtocol>,
        version: u64,
    ) -> Self {
        Self {
            inner: Some(inner),
            tx,
            version,
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn set_inner(&mut self, inner: Box<dyn StatefulProtocol>) {
        self.inner = Some(inner);
    }
}

impl StatefulWidget for ThreadImage {
    type State = ThreadProtocol;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.inner = match state.inner.take() {
            Some(mut protocol) => {
                if let Some(rect) = protocol.needs_resize(&self.resize, area) {
                    let _ = state.tx.send((state.version, protocol, self.resize, rect));
                    None
                } else {
                    protocol.render(area, buf);
                    Some(protocol)
                }
            }
            None => None,
        };
    }
}

pub struct UiState<'a> {
    pub config: &'a Config,
    pub parent: &'a [FileEntry],
    pub current: &'a [FileEntry],
    pub selected: usize,
    pub preview: Option<&'a Preview>,
    pub show_metadata: bool,
    pub metadata: Option<&'a FileMetadata>,
    pub image_state: Option<&'a mut ThreadProtocol>,
}

pub fn render(frame: &mut Frame, mut state: UiState<'_>) {
    let config = state.config;
    let layout = if state.show_metadata {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(frame.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .split(frame.area())
    };

    let areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(35),
            Constraint::Percentage(40),
        ])
        .split(layout[0]);

    let parent_items = list_items(config, state.parent);
    let parent_list = List::new(parent_items)
        .block(Block::default().borders(Borders::ALL).title("Parent"));
    frame.render_widget(parent_list, areas[0]);

    let current_items = list_items(config, state.current);
    let selection_style = Style::default().add_modifier(Modifier::REVERSED);
    let current_list = List::new(current_items)
        .block(Block::default().borders(Borders::ALL).title("Current"))
        .highlight_style(selection_style)
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    if !state.current.is_empty() {
        let selected = state.selected.min(state.current.len() - 1);
        list_state.select(Some(selected));
    }
    frame.render_stateful_widget(current_list, areas[1], &mut list_state);

    let preview_title = match state.preview {
        Some(preview) => preview_title(preview),
        None => "Preview".to_string(),
    };
    let preview_block = Block::default()
        .borders(Borders::ALL)
        .title(preview_title);
    let preview_area = preview_block.inner(areas[2]);
    let mut rendered_image = false;
    if let (Some(preview), Some(image_state)) = (state.preview, state.image_state.as_deref_mut()) {
        if matches!(preview.data, PreviewData::Image { .. }) {
            let image = ThreadImage::new().resize(Resize::Fit);
            frame.render_stateful_widget(image, preview_area, image_state);
            rendered_image = true;
        }
    }
    if !rendered_image {
        let preview_content = match state.preview {
            Some(preview) => preview_text(preview),
            None => String::new(),
        };
        let preview = Paragraph::new(preview_content).block(preview_block.clone());
        frame.render_widget(preview, areas[2]);
    } else {
        frame.render_widget(preview_block, areas[2]);
    }

    if state.show_metadata && layout.len() > 1 {
        let metadata = Paragraph::new(metadata_text(config, state.metadata))
            .block(Block::default().borders(Borders::ALL).title("Meta"));
        frame.render_widget(metadata, layout[1]);
    }
}

fn list_items(config: &Config, entries: &[FileEntry]) -> Vec<ListItem<'static>> {
    entries
        .iter()
        .map(|entry| {
            let icon = if entry.is_dir {
                &config.icons.folder
            } else {
                &config.icons.file
            };
            let label = format!("{icon} {}", entry.name);
            ListItem::new(label)
        })
        .collect()
}

fn preview_title(preview: &Preview) -> String {
    let name = preview
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Preview");
    let mut title = name.to_string();
    if matches!(preview.mismatch, Some(MismatchStatus::Mismatch { .. })) {
        title.push_str(" !");
    }
    title
}

fn preview_text(preview: &Preview) -> String {
    match &preview.data {
        PreviewData::Text(text) => text.clone(),
        PreviewData::Image { width, height } => format!("image ({}x{})", width, height),
        PreviewData::Binary { size } => format!("binary ({} bytes)", size),
        PreviewData::Empty => String::new(),
    }
}

fn metadata_text(config: &Config, metadata: Option<&FileMetadata>) -> String {
    let Some(metadata) = metadata else {
        return String::new();
    };
    let icons = &config.metadata_bar.icons;
    let mut parts = Vec::new();
    parts.push(format!("{} {}", icons.permissions, metadata.permissions));
    parts.push(format!("{} {}", icons.owner, metadata.owner));
    if let Some(created) = &metadata.created {
        parts.push(format!("{} {}", icons.created, created));
    }
    if let Some(modified) = &metadata.modified {
        parts.push(format!("{} {}", icons.modified, modified));
    }
    if let Some(accessed) = &metadata.accessed {
        parts.push(format!("{} {}", icons.accessed, accessed));
    }
    parts.join("  ")
}
