use crate::config::Config;
use crate::preview::{self, Preview};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub permissions: String,
    pub owner: String,
}

impl FileEntry {
    pub async fn from_dir_entry(entry: fs::DirEntry) -> Result<Self, std::io::Error> {
        let file_type = entry.file_type().await?;
        let metadata = entry.metadata().await?;
        let name = entry.file_name().to_string_lossy().to_string();
        Ok(FileEntry {
            name,
            path: entry.path(),
            is_dir: file_type.is_dir(),
            permissions: permissions_string(&metadata),
            owner: owner_string(&metadata),
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

pub async fn read_dir_stream(path: &Path) -> Result<ReadDirStream, CoreError> {
    Ok(ReadDirStream::new(fs::read_dir(path).await?))
}

pub fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase()),
    });
}

pub async fn load_preview(path: &Path, config: &Config) -> Result<Preview, CoreError> {
    Ok(preview::load(path, config).await?)
}

pub async fn create_file(path: &Path) -> std::io::Result<()> {
    fs::File::create(path).await.map(|_| ())
}

pub async fn create_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path).await
}

pub async fn remove_path(path: &Path) -> std::io::Result<()> {
    let metadata = fs::metadata(path).await?;
    if metadata.is_dir() {
        fs::remove_dir_all(path).await
    } else {
        fs::remove_file(path).await
    }
}

pub async fn rename_path(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::rename(src, dest).await
}

pub async fn copy_recursively(src: &Path, dest: &Path) -> std::io::Result<()> {
    let mut stack = vec![(src.to_path_buf(), dest.to_path_buf())];
    while let Some((src_path, dest_path)) = stack.pop() {
        let metadata = fs::metadata(&src_path).await?;
        if metadata.is_dir() {
            fs::create_dir_all(&dest_path).await?;
            let mut entries = fs::read_dir(&src_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let entry_path = entry.path();
                let entry_dest = dest_path.join(entry.file_name());
                stack.push((entry_path, entry_dest));
            }
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&src_path, &dest_path).await?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn permissions_string(metadata: &std::fs::Metadata) -> String {
    let mode = metadata.permissions().mode();
    let mut output = String::with_capacity(9);
    output.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    output.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    output.push(match (mode & 0o100 != 0, mode & 0o4000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    output.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    output.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    output.push(match (mode & 0o010 != 0, mode & 0o2000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        (false, false) => '-',
    });
    output.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    output.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    output.push(match (mode & 0o001 != 0, mode & 0o1000 != 0) {
        (true, true) => 't',
        (false, true) => 'T',
        (true, false) => 'x',
        (false, false) => '-',
    });
    output
}

#[cfg(not(unix))]
fn permissions_string(metadata: &std::fs::Metadata) -> String {
    if metadata.permissions().readonly() {
        "r--r--r--".to_string()
    } else {
        "rw-rw-rw-".to_string()
    }
}

#[cfg(unix)]
fn owner_string(metadata: &std::fs::Metadata) -> String {
    format!("{}:{}", metadata.uid(), metadata.gid())
}

#[cfg(not(unix))]
fn owner_string(_: &std::fs::Metadata) -> String {
    "-".to_string()
}
