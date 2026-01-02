use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedType {
    pub extension: String,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MismatchStatus {
    Match,
    Mismatch {
        detected: DetectedType,
        extension: String,
    },
    Unknown,
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
