use crate::config::Config;
use crate::preview::{self, Preview};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

impl FileEntry {
    pub async fn from_dir_entry(entry: fs::DirEntry) -> Result<Self, std::io::Error> {
        let file_type = entry.file_type().await?;
        let name = entry.file_name().to_string_lossy().to_string();
        Ok(FileEntry {
            name,
            path: entry.path(),
            is_dir: file_type.is_dir(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("failed to read directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to load preview: {0}")]
    Preview(#[from] preview::PreviewError),
}

pub async fn list_dir(path: &Path) -> Result<Vec<FileEntry>, CoreError> {
    let mut read_dir = fs::read_dir(path).await?;
    let mut entries = Vec::new();

    while let Some(entry) = read_dir.next_entry().await? {
        entries.push(FileEntry::from_dir_entry(entry).await?);
    }

    sort_entries(&mut entries);

    Ok(entries)
}

pub async fn read_dir_stream(path: &Path) -> Result<ReadDirStream, CoreError> {
    Ok(ReadDirStream::new(fs::read_dir(path).await?))
}

pub fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
    });
}

pub async fn load_preview(path: &Path, config: &Config) -> Result<Preview, CoreError> {
    Ok(preview::load(path, config).await?)
}
