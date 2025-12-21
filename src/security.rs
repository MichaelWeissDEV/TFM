use std::path::Path;
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;

const READ_LIMIT: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedType {
    pub extension: String,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MismatchStatus {
    Match,
    Mismatch { detected: DetectedType, extension: String },
    Unknown,
}

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("failed to read file: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn check_file_mismatch(path: &Path) -> Result<MismatchStatus, SecurityError> {
    let metadata = fs::metadata(path).await?;
    if !metadata.is_file() {
        return Ok(MismatchStatus::Unknown);
    }

    let mut file = File::open(path).await?;
    let mut buf = vec![0u8; READ_LIMIT];
    let read_len = file.read(&mut buf).await?;
    if read_len == 0 {
        return Ok(MismatchStatus::Unknown);
    }
    buf.truncate(read_len);

    Ok(check_buffer_mismatch(path, &buf))
}

fn extensions_match(extension: &str, detected: &str) -> bool {
    normalize_extension(extension) == normalize_extension(detected)
}

pub fn check_buffer_mismatch(path: &Path, buf: &[u8]) -> MismatchStatus {
    if buf.is_empty() {
        return MismatchStatus::Unknown;
    }

    let detected = match infer::get(buf) {
        Some(kind) => DetectedType {
            extension: kind.extension().to_string(),
            mime: kind.mime_type().to_string(),
        },
        None => return MismatchStatus::Unknown,
    };

    let extension = match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if !ext.is_empty() => ext.to_ascii_lowercase(),
        _ => return MismatchStatus::Unknown,
    };

    if extensions_match(&extension, &detected.extension) {
        MismatchStatus::Match
    } else {
        MismatchStatus::Mismatch {
            detected,
            extension,
        }
    }
}

fn normalize_extension(extension: &str) -> &str {
    match extension {
        "jpeg" | "jpe" => "jpg",
        "tiff" => "tif",
        "htm" => "html",
        "yml" => "yaml",
        "oga" | "ogv" | "ogm" => "ogg",
        _ => extension,
    }
}
