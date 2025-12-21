use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug)]
pub struct MarkerStore {
    path: PathBuf,
    markers: HashMap<String, PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
struct MarkerFile {
    markers: HashMap<String, String>,
}

impl MarkerStore {
    pub async fn load() -> Self {
        let path = default_marker_path();
        let markers = match fs::read_to_string(&path).await {
            Ok(content) => parse_markers(&content),
            Err(_) => HashMap::new(),
        };
        Self { path, markers }
    }

    pub fn get(&self, key: &str) -> Option<&PathBuf> {
        self.markers.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, path: PathBuf) {
        self.markers.insert(key.into(), path);
    }

    pub fn remove(&mut self, key: &str) -> bool {
        self.markers.remove(key).is_some()
    }

    pub fn rename(&mut self, old: &str, new: String) -> bool {
        if old == new {
            return false;
        }
        let Some(path) = self.markers.remove(old) else {
            return false;
        };
        self.markers.insert(new, path);
        true
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &PathBuf)> {
        self.markers.iter()
    }

    pub fn save_task(&self) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let path = self.path.clone();
        let markers = self.markers.clone();
        async move { save_markers(path, markers).await }
    }
}

fn parse_markers(content: &str) -> HashMap<String, PathBuf> {
    let file: MarkerFile = toml::from_str(content).unwrap_or_default();
    let mut markers = HashMap::new();
    for (key, value) in file.markers {
        let name = key.trim();
        if name.is_empty() {
            continue;
        }
        markers.insert(name.to_string(), PathBuf::from(value));
    }
    markers
}

fn default_marker_path() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir.join("vfm").join("markers.toml");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".vfm.markers.toml");
    }
    PathBuf::from("markers.toml")
}

async fn save_markers(path: PathBuf, markers: HashMap<String, PathBuf>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let markers = markers
        .iter()
        .map(|(key, value)| (key.clone(), value.to_string_lossy().to_string()))
        .collect();
    let content = toml::to_string(&MarkerFile { markers })
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
    fs::write(&path, content).await
}
