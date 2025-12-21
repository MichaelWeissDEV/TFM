use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub check_mismatch: bool,
    pub theme: Theme,
    pub icons: Icons,
    pub metadata_bar: MetadataBar,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            check_mismatch: false,
            theme: Theme::default(),
            icons: Icons::default(),
            metadata_bar: MetadataBar::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        if let Ok(path) = env::var("VFM_CONFIG") {
            let path = PathBuf::from(path);
            if path.exists() {
                return load_from_path(&path);
            }
            return Err(ConfigError::Missing(path));
        }

        for path in default_paths() {
            if path.exists() {
                return load_from_path(&path);
            }
        }

        Ok(Self::default())
    }
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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
    let content = fs::read_to_string(path)?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => Ok(toml::from_str(&content)?),
        Some("yaml") | Some("yml") => Ok(serde_yaml::from_str(&content)?),
        _ => Err(ConfigError::UnsupportedFormat(path.to_path_buf())),
    }
}

fn default_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(dir) = dirs::config_dir() {
        let base = dir.join("vfm");
        paths.push(base.join("config.toml"));
        paths.push(base.join("config.yaml"));
        paths.push(base.join("config.yml"));
    }

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".vfm.toml"));
        paths.push(home.join(".vfm.yaml"));
        paths.push(home.join(".vfm.yml"));
    }

    paths
}
