//! Ver CLAUDE.md seção 9-21 — Project, Timeline, Track, Clip, Media,
//! Serialization. O núcleo do sistema (seção 117): todo o resto (UI,
//! playback, export) opera sobre esses tipos.
//!
mod clip;
mod generator;
mod media;
// `project/project.rs` é a estrutura de diretório exigida pela seção 7 do
// spec — o "stutter" é esperado, não um erro de nomenclatura.
#[allow(clippy::module_inception)]
mod project;
mod selection;
mod time;
mod track;
mod transform;

pub mod serialization;
pub mod timeline;

pub use clip::{Clip, ClipId};
pub use generator::{
    AudioVisualizerSettings, GeneratedMedia, GeneratedMediaEntry, SolidColor, VisualizerAlignment,
};
pub use media::{Media, MediaId, MediaMetadata};
pub use project::Project;
pub use project::ProjectSettings;
pub use selection::Selection;
pub use time::{Rational, Time};
pub use track::{Track, TrackId, TrackType};
pub use transform::Transform;

#[allow(unused_imports)]
pub use clip::AudioClip;
