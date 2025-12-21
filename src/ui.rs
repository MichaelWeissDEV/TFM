use crate::config::Config;
use crate::core::FileEntry;
use crate::preview::{FileMetadata, Preview, PreviewData};
use crate::security::MismatchStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, StatefulWidget};
use ratatui::Frame;
use ratatui_image::{protocol::StatefulProtocol, Resize};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

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

pub struct InputPrompt {
    pub title: String,
    pub value: String,
}

pub type HighlightedText = Text<'static>;

pub struct UiState<'a> {
    pub config: &'a Config,
    pub parent: &'a [FileEntry],
    pub current: &'a [FileEntry],
    pub current_indices: &'a [usize],
    pub selected: usize,
    pub preview: Option<&'a Preview>,
    pub highlighted_preview: Option<&'a HighlightedText>,
    pub show_metadata: bool,
    pub show_permissions: bool,
    pub show_dates: bool,
    pub metadata: Option<&'a FileMetadata>,
    pub image_state: Option<&'a mut ThreadProtocol>,
    pub input: Option<InputPrompt>,
}

pub fn render(frame: &mut Frame, mut state: UiState<'_>) {
    let theme = &state.config.theme;
    let base_style = Style::default()
        .fg(parse_color(&theme.foreground))
        .bg(parse_color(&theme.background));
    let accent_style = Style::default().fg(parse_color(&theme.accent));
    let selection_style = Style::default()
        .fg(parse_color(&theme.selection_fg))
        .bg(parse_color(&theme.selection_bg))
        .add_modifier(Modifier::BOLD);
    let warning_style = Style::default().fg(parse_color(&theme.warning));

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

    let parent_items = list_items(state.config, state.parent, None);
    let parent_list = List::new(parent_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Parent")
            .style(base_style)
            .border_style(accent_style)
            .title_style(accent_style),
    );
    frame.render_widget(parent_list, areas[0]);

    let current_items = list_items(state.config, state.current, Some(state.current_indices));
    let current_list = List::new(current_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Current")
                .style(base_style)
                .border_style(accent_style)
                .title_style(accent_style),
        )
        .highlight_style(selection_style)
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    if !state.current_indices.is_empty() {
        let selected = state.selected.min(state.current_indices.len() - 1);
        list_state.select(Some(selected));
    }
    frame.render_stateful_widget(current_list, areas[1], &mut list_state);

    let (preview_title, has_mismatch) = match state.preview {
        Some(preview) => preview_title(preview),
        None => ("Preview".to_string(), false),
    };
    let title_style = if has_mismatch {
        warning_style
    } else {
        accent_style
    };
    let preview_block = Block::default()
        .borders(Borders::ALL)
        .title(preview_title)
        .style(base_style)
        .border_style(accent_style)
        .title_style(title_style);
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
        let preview_widget = match (state.preview, state.highlighted_preview) {
            (Some(_), Some(highlighted)) => Paragraph::new(highlighted.clone())
                .block(preview_block)
                .style(base_style),
            (Some(preview), None) => Paragraph::new(preview_text(preview))
                .block(preview_block)
                .style(base_style),
            (None, _) => Paragraph::new(String::new())
                .block(preview_block)
                .style(base_style),
        };
        frame.render_widget(preview_widget, areas[2]);
    } else {
        frame.render_widget(preview_block, areas[2]);
    }

    if state.show_metadata && layout.len() > 1 {
        let metadata = Paragraph::new(metadata_text(
            state.config,
            state.metadata,
            state.show_permissions,
            state.show_dates,
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Meta")
                .style(base_style)
                .border_style(accent_style)
                .title_style(accent_style),
        )
        .style(base_style);
        frame.render_widget(metadata, layout[1]);
    }

    if let Some(input) = state.input {
        let overlay_area = input_rect(areas[1]);
        frame.render_widget(Clear, overlay_area);
        let input_widget = Paragraph::new(input.value).block(
            Block::default()
                .borders(Borders::ALL)
                .title(input.title)
                .style(base_style)
                .border_style(accent_style)
                .title_style(accent_style),
        )
        .style(base_style);
        frame.render_widget(input_widget, overlay_area);
    }
}

pub fn highlight_preview(preview: &Preview) -> Option<HighlightedText> {
    let PreviewData::Text(text) = &preview.data else {
        return None;
    };
    let syntax_set = syntax_set();
    let syntax = preview
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| syntax_set.find_syntax_by_extension(ext))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(text) {
        let ranges = highlighter
            .highlight_line(line, syntax_set)
            .unwrap_or_default();
        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, content)| Span::styled(content.to_string(), syntect_style(style)))
            .collect();
        lines.push(Line::from(spans));
    }
    Some(Text::from(lines))
}

fn list_items(
    config: &Config,
    entries: &[FileEntry],
    indices: Option<&[usize]>,
) -> Vec<ListItem<'static>> {
    let iter: Box<dyn Iterator<Item = &FileEntry>> = match indices {
        Some(indices) => Box::new(indices.iter().filter_map(|&index| entries.get(index))),
        None => Box::new(entries.iter()),
    };
    iter.map(|entry| ListItem::new(entry_label(config, entry)))
        .collect()
}

fn entry_label(config: &Config, entry: &FileEntry) -> String {
    let icon = if entry.is_dir {
        &config.icons.folder
    } else {
        &config.icons.file
    };
    format!("{icon} {}", entry.name)
}

fn preview_title(preview: &Preview) -> (String, bool) {
    let name = preview
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Preview");
    let mut title = name.to_string();
    let mismatch = matches!(preview.mismatch, Some(MismatchStatus::Mismatch { .. }));
    if mismatch {
        title.push_str(" !");
    }
    (title, mismatch)
}

fn preview_text(preview: &Preview) -> String {
    match &preview.data {
        PreviewData::Text(text) => text.clone(),
        PreviewData::Image { width, height } => format!("image ({}x{})", width, height),
        PreviewData::Binary { size } => format!("binary ({} bytes)", size),
        PreviewData::Empty => String::new(),
    }
}

fn metadata_text(
    config: &Config,
    metadata: Option<&FileMetadata>,
    show_permissions: bool,
    show_dates: bool,
) -> String {
    let Some(metadata) = metadata else {
        return String::new();
    };
    let icons = &config.metadata_bar.icons;
    let mut parts = Vec::new();
    if show_permissions {
        parts.push(format!("{} {}", icons.permissions, metadata.permissions));
    }
    parts.push(format!("{} {}", icons.owner, metadata.owner));
    if show_dates {
        if let Some(created) = &metadata.created {
            parts.push(format!("{} {}", icons.created, created));
        }
        if let Some(modified) = &metadata.modified {
            parts.push(format!("{} {}", icons.modified, modified));
        }
        if let Some(accessed) = &metadata.accessed {
            parts.push(format!("{} {}", icons.accessed, accessed));
        }
    }
    parts.join("  ")
}

fn input_rect(area: Rect) -> Rect {
    let width = (area.width * 3 / 4).max(10u16).min(area.width);
    let height = 3u16.min(area.height.max(1u16));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn parse_color(value: &str) -> Color {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 6 {
            let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
            if let (Some(r), Some(g), Some(b)) = (parse(0..2), parse(2..4), parse(4..6)) {
                return Color::Rgb(r, g, b);
            }
        }
    }
    match value.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::Reset,
    }
}

fn syntect_style(style: SyntectStyle) -> Style {
    let mut ratatui_style = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(FontStyle::BOLD) {
        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
    }
    ratatui_style
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let set = ThemeSet::load_defaults();
        set.themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| set.themes.values().next().cloned().unwrap())
    })
}
