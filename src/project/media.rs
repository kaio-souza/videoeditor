//! Ver CLAUDE.md seção 12-13 — Media, MediaId, MediaMetadata.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::time::{Rational, Time};

/// Identifica uma mídia por UUID, não pelo nome do arquivo — permite dois
/// arquivos com o mesmo nome coexistirem no projeto (seção 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaId(Uuid);

impl MediaId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadados lidos do arquivo pelo decoder (seção 92 — Fase 7). No MVP
/// `Media` pode existir com metadados vazios (`width`/`height`/etc. `None`)
/// antes do decoder rodar; só `duration` é sempre conhecida (mesmo que
/// `Time::ZERO` até ser lida de verdade).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub duration: Time,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<Rational>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

/// Uma mídia importada (vídeo ou áudio). O arquivo original nunca é
/// modificado (seção 2.3) — clips na timeline sempre apontam de volta pra
/// cá via `MediaId`, nunca copiam o conteúdo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub id: MediaId,
    pub path: PathBuf,
    pub metadata: MediaMetadata,
}

impl Media {
    pub fn new(path: impl Into<PathBuf>, metadata: MediaMetadata) -> Self {
        Self {
            id: MediaId::new(),
            path: path.into(),
            metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_media_gets_a_distinct_id_even_with_the_same_path() {
        let a = Media::new("video.mp4", MediaMetadata::default());
        let b = Media::new("video.mp4", MediaMetadata::default());
        assert_ne!(
            a.id, b.id,
            "arquivos com nome igual devem ter IDs diferentes"
        );
    }
}
