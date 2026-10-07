pub mod audio_decoder;
pub mod cache;
pub mod decoder;
pub mod metadata;
pub mod waveform;

use std::path::Path;

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v"];
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "aac", "flac", "ogg", "m4a", "wma"];
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff"];

fn has_extension(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Imagem estática (seção 67) — sem eixo de tempo, decodifica diferente de
/// vídeo (ver `decoder::decode_frame_at_scaled`) e não tem duração real
/// (ver `metadata::probe`).
pub fn is_image_path(path: &Path) -> bool {
    has_extension(path, IMAGE_EXTENSIONS)
}

pub fn is_supported_media_path(path: &Path) -> bool {
    has_extension(path, VIDEO_EXTENSIONS)
        || has_extension(path, AUDIO_EXTENSIONS)
        || has_extension(path, IMAGE_EXTENSIONS)
}
