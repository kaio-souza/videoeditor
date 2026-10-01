//! Ver CLAUDE.md seção 14-16 — Track, TrackType, Video/Audio tracks.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::clip::Clip;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(Uuid);

impl TrackId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TrackId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackType {
    Video,
    Audio,
}

/// Uma faixa de vídeo ou áudio. Tracks de vídeo compõem de baixo pra cima —
/// `video_tracks[0]` é o fundo, a última é a de cima (seção 15).
///
/// `volume` só é relevante pra `TrackType::Audio` (seção 16) — fica na
/// struct comum em vez de um `AudioTrack` separado pra não duplicar
/// `id`/`name`/`muted`/`locked`; vídeo simplesmente ignora o campo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub track_type: TrackType,
    pub muted: bool,
    pub locked: bool,
    pub volume: f32,
    #[serde(default)]
    pub pan: f32,
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn new(name: impl Into<String>, track_type: TrackType) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            track_type,
            muted: false,
            locked: false,
            volume: 1.0,
            pan: 0.0,
            clips: Vec::new(),
        }
    }
}
