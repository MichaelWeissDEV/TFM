use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub check_mismatch: bool,
    pub theme: Theme,
    pub icons: Icons,
    pub metadata_bar: MetadataBar,
    pub open_with: OpenWithConfig,
    pub keys: KeyBindings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            check_mismatch: false,
            theme: Theme::default(),
            icons: Icons::default(),
            metadata_bar: MetadataBar::default(),
            open_with: OpenWithConfig::default(),
            keys: KeyBindings::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let fallback = Self::default();
        if let Ok(path) = env::var("TFM_CONFIG") {
            let path = PathBuf::from(path);
            if path.exists() {
                return load_from_path(&path);
            }
            let _ = write_default_config(&path, &fallback);
            return Ok(fallback);
        }

        let paths = default_paths();
        for path in &paths {
            if path.exists() {
                return load_from_path(path);
            }
        }
        if let Some(path) = paths.first() {
            let _ = write_default_config(path, &fallback);
        }

        Ok(fallback)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub selection_bg: String,
    pub selection_fg: String,
    pub accent: String,
    pub folder: String,
    pub warning: String,
    pub error: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "black".to_string(),
            foreground: "white".to_string(),
            selection_bg: "blue".to_string(),
            selection_fg: "black".to_string(),
            accent: "cyan".to_string(),
            folder: "lightblue".to_string(),
            warning: "yellow".to_string(),
            error: "red".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Icons {
    pub folder: String,
    pub file: String,
    pub text: String,
    pub image: String,
    pub video: String,
    pub audio: String,
    pub archive: String,
    pub symlink: String,
    pub unknown: String,
}

impl Default for Icons {
    fn default() -> Self {
        Self {
            folder: "󰉋".to_string(),
            file: "󰈔".to_string(),
            text: "󰈙".to_string(),
            image: "󰈟".to_string(),
            video: "󰕧".to_string(),
            audio: "󰎆".to_string(),
            archive: "󰀼".to_string(),
            symlink: "󰌷".to_string(),
            unknown: "󰈚".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MetadataBar {
    pub enabled: bool,
    pub show_permissions: bool,
    pub show_dates: bool,
    pub show_owner: bool,
    pub icons: MetadataIcons,
}

impl Default for MetadataBar {
    fn default() -> Self {
        Self {
            enabled: false,
            show_permissions: true,
            show_dates: true,
            show_owner: true,
            icons: MetadataIcons::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MetadataIcons {
    pub permissions: String,
    pub owner: String,
    pub created: String,
    pub modified: String,
    pub accessed: String,
}

impl Default for MetadataIcons {
    fn default() -> Self {
        Self {
            permissions: "󰌾".to_string(),
            owner: "󰉍".to_string(),
            created: "󰃰".to_string(),
            modified: "󰃯".to_string(),
            accessed: "󰃱".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenWithConfig {
    pub quick: HashMap<String, String>,
}

impl Default for OpenWithConfig {
    fn default() -> Self {
        Self {
            quick: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KeyBindings {
    pub normal: NormalKeys,
    pub add: AddKeys,
    pub settings: SettingsKeys,
    pub view: ViewKeys,
    pub copy: CopyKeys,
    pub delete: DeleteKeys,
    pub marker_list: MarkerListKeys,
    pub open_with: OpenWithKeys,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            normal: NormalKeys::default(),
            add: AddKeys::default(),
            settings: SettingsKeys::default(),
            view: ViewKeys::default(),
            copy: CopyKeys::default(),
            delete: DeleteKeys::default(),
            marker_list: MarkerListKeys::default(),
            open_with: OpenWithKeys::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NormalKeys {
    pub quit: Vec<String>,
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub parent: Vec<String>,
    pub open: Vec<String>,
    pub search: Vec<String>,
    pub add: Vec<String>,
    pub rename: Vec<String>,
    pub delete: Vec<String>,
    pub marker_set: Vec<String>,
    pub marker_list: Vec<String>,
    pub marker_jump: Vec<String>,
    pub settings: Vec<String>,
    pub view: Vec<String>,
    pub copy: Vec<String>,
    pub cut: Vec<String>,
    pub paste: Vec<String>,
    pub open_shell: Vec<String>,
    pub open_with_picker: Vec<String>,
    pub open_with_quick: Vec<String>,
}

impl Default for NormalKeys {
    fn default() -> Self {
        Self {
            quit: vec!["q".to_string()],
            up: vec!["up".to_string(), "k".to_string()],
            down: vec!["down".to_string(), "j".to_string()],
            parent: vec!["left".to_string(), "h".to_string()],
            open: vec!["right".to_string(), "l".to_string(), "enter".to_string()],
            search: vec!["/".to_string()],
            add: vec!["a".to_string()],
            rename: vec!["r".to_string()],
            delete: vec!["d".to_string()],
            marker_set: vec!["m".to_string()],
            marker_list: vec!["M".to_string()],
            marker_jump: vec!["g".to_string()],
            settings: vec!["s".to_string()],
            view: vec!["v".to_string()],
            copy: vec!["c".to_string()],
            cut: vec!["x".to_string()],
            paste: vec!["p".to_string()],
            open_shell: vec!["t".to_string()],
            open_with_picker: vec!["ctrl+o".to_string(), "O".to_string()],
            open_with_quick: vec!["o".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AddKeys {
    pub dir: Vec<String>,
}

impl Default for AddKeys {
    fn default() -> Self {
        Self {
            dir: vec!["d".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SettingsKeys {
    pub toggle_permissions: Vec<String>,
    pub toggle_dates: Vec<String>,
    pub toggle_owner: Vec<String>,
    pub toggle_metadata: Vec<String>,
    pub toggle_hidden: Vec<String>,
}

impl Default for SettingsKeys {
    fn default() -> Self {
        Self {
            toggle_permissions: vec!["r".to_string()],
            toggle_dates: vec!["d".to_string()],
            toggle_owner: vec!["o".to_string()],
            toggle_metadata: vec!["m".to_string()],
            toggle_hidden: vec!["h".to_string(), "H".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ViewKeys {
    pub toggle_list_permissions: Vec<String>,
    pub toggle_list_owner: Vec<String>,
}

impl Default for ViewKeys {
    fn default() -> Self {
        Self {
            toggle_list_permissions: vec!["p".to_string()],
            toggle_list_owner: vec!["o".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CopyKeys {
    pub copy_path: Vec<String>,
}

impl Default for CopyKeys {
    fn default() -> Self {
        Self {
            copy_path: vec!["p".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DeleteKeys {
    pub confirm: Vec<String>,
}

impl Default for DeleteKeys {
    fn default() -> Self {
        Self {
            confirm: vec!["d".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MarkerListKeys {
    pub close: Vec<String>,
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub open: Vec<String>,
    pub rename: Vec<String>,
    pub edit_path: Vec<String>,
    pub delete: Vec<String>,
    pub add: Vec<String>,
    pub search: Vec<String>,
}

impl Default for MarkerListKeys {
    fn default() -> Self {
        Self {
            close: vec!["esc".to_string()],
            up: vec!["up".to_string(), "k".to_string()],
            down: vec!["down".to_string(), "j".to_string()],
            open: vec!["enter".to_string()],
            rename: vec!["r".to_string()],
            edit_path: vec!["e".to_string()],
            delete: vec!["d".to_string()],
            add: vec!["a".to_string()],
            search: vec!["/".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenWithKeys {
    pub close: Vec<String>,
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub open: Vec<String>,
    pub backspace: Vec<String>,
}

impl Default for OpenWithKeys {
    fn default() -> Self {
        Self {
            close: vec!["esc".to_string()],
            up: vec!["up".to_string()],
            down: vec!["down".to_string()],
            open: vec!["enter".to_string()],
            backspace: vec!["backspace".to_string()],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    Missing(PathBuf),
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported config format: {0}")]
    UnsupportedFormat(PathBuf),
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

fn load_from_path(path: &Path) -> Result<Config, ConfigError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ConfigError::Missing(path.to_path_buf()));
        }
        Err(err) => return Err(ConfigError::Io(err)),
    };
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => Ok(toml::from_str(&content)?),
        Some("yaml") | Some("yml") => Ok(serde_yaml::from_str(&content)?),
        _ => Err(ConfigError::UnsupportedFormat(path.to_path_buf())),
    }
}

fn write_default_config(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = match path.extension().and_then(|ext| ext.to_str()) {
        Some("yaml") | Some("yml") => serde_yaml::to_string(config)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?,
        _ => toml::to_string_pretty(config)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?,
    };
    fs::write(path, content)
}

fn default_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(dir) = dirs::config_dir() {
        let base = dir.join("tfm");
        paths.push(base.join("config.toml"));
        paths.push(base.join("config.yaml"));
        paths.push(base.join("config.yml"));
    }

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".tfm.toml"));
        paths.push(home.join(".tfm.yaml"));
        paths.push(home.join(".tfm.yml"));
    }

    paths
}
