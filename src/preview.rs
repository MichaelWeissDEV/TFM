use crate::config::Config;
use crate::security::{self, MismatchStatus};
use image::DynamicImage;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const PREVIEW_LIMIT: usize = 65536;

#[derive(Debug)]
pub enum PreviewData {
    Text(String),
    Image { width: u32, height: u32 },
    Binary { size: u64 },
    Empty,
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub permissions: String,
    pub owner: String,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub accessed: Option<String>,
}

#[derive(Debug)]
pub struct Preview {
    pub path: PathBuf,
    pub data: PreviewData,
    pub mismatch: Option<MismatchStatus>,
    pub metadata: Option<FileMetadata>,
    pub image: Option<DynamicImage>,
}

#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn load(path: &Path, config: &Config) -> Result<Preview, PreviewError> {
    let metadata = fs::metadata(path).await?;
    let file_metadata = build_metadata(&metadata);
    if !metadata.is_file() {
        return Ok(Preview {
            path: path.to_path_buf(),
            data: PreviewData::Empty,
            mismatch: None,
            metadata: Some(file_metadata),
            image: None,
        });
    }

    let mut file = File::open(path).await?;
    let mut buf = vec![0u8; PREVIEW_LIMIT];
    let read_len = file.read(&mut buf).await?;
    buf.truncate(read_len);

    let mismatch = if config.check_mismatch {
        Some(
            security::check_file_mismatch(path)
                .await
                .unwrap_or(MismatchStatus::Unknown),
        )
    } else {
        None
    };

    let is_image = read_len > 0
        && infer::get(&buf)
            .map(|kind| kind.mime_type().starts_with("image/"))
            .unwrap_or(false);
    let image = if is_image {
        decode_image(path.to_path_buf()).await
    } else {
        None
    };
    let data = if let Some(image) = image.as_ref() {
        PreviewData::Image {
            width: image.width(),
            height: image.height(),
        }
    } else if read_len == 0 {
        PreviewData::Empty
    } else if let Ok(text) = std::str::from_utf8(&buf) {
        PreviewData::Text(text.to_string())
    } else {
        PreviewData::Binary {
            size: metadata.len(),
        }
    };

    Ok(Preview {
        path: path.to_path_buf(),
        data,
        mismatch,
        metadata: Some(file_metadata),
        image,
    })
}

async fn decode_image(path: PathBuf) -> Option<DynamicImage> {
    tokio::task::spawn_blocking(move || {
        let reader = image::io::Reader::open(path).ok()?;
        reader.with_guessed_format().ok()?.decode().ok()
    })
    .await
    .ok()
    .flatten()
}

fn build_metadata(metadata: &std::fs::Metadata) -> FileMetadata {
    FileMetadata {
        permissions: permissions_string(metadata),
        owner: owner_string(metadata),
        created: time_string(metadata.created()),
        modified: time_string(metadata.modified()),
        accessed: time_string(metadata.accessed()),
    }
}

fn time_string(value: std::io::Result<SystemTime>) -> Option<String> {
    value.ok().and_then(format_time)
}

fn format_time(time: SystemTime) -> Option<String> {
    let timestamp = OffsetDateTime::from(time);
    timestamp.format(&Rfc3339).ok()
}

#[cfg(unix)]
fn permissions_string(metadata: &std::fs::Metadata) -> String {
    let mode = metadata.permissions().mode();
    let mut output = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        output.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        output.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        output.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
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
