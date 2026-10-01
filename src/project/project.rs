//! Ver CLAUDE.md seção 9-10 — Project, ProjectSettings.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::generator::{AudioVisualizerSettings, GeneratedMedia, GeneratedMediaEntry, SolidColor};
use super::media::{Media, MediaId};
use super::time::{Rational, Time};
use super::track::{Track, TrackId, TrackType};

/// Configuração do projeto (seção 10). `Default` usa a configuração inicial
/// sugerida: 1920x1080, 30fps, 48kHz estéreo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: Rational::new(30, 1),
            sample_rate: 48_000,
            channels: 2,
        }
    }
}

/// Um projeto: configurações, mídia importada e as tracks de vídeo/áudio
/// (seção 9). Camadas geradas (Solid Color, Audio Visualizer — seção 35)
/// não são uma coleção separada: entram como clips normais dentro de
/// `video_tracks`, uma vez que o gerador existir (Fase 15/16).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub settings: ProjectSettings,
    pub media: HashMap<MediaId, Media>,
    pub generated: HashMap<MediaId, GeneratedMediaEntry>,
    pub video_tracks: Vec<Track>,
    pub audio_tracks: Vec<Track>,
}

impl Project {
    pub fn new(settings: ProjectSettings) -> Self {
        Self {
            settings,
            media: HashMap::new(),
            generated: HashMap::new(),
            video_tracks: Vec::new(),
            audio_tracks: Vec::new(),
        }
    }

    /// Cria uma camada de cor sólida (seção 36) e devolve o `MediaId` que a
    /// referencia — usa como `Clip::source` igual a qualquer mídia real.
    pub fn add_solid_color(&mut self, color: SolidColor, duration: Time) -> MediaId {
        let id = MediaId::new();
        self.generated.insert(
            id,
            GeneratedMediaEntry {
                kind: GeneratedMedia::SolidColor(color),
                duration,
            },
        );
        id
    }

    /// Cria uma camada de Audio Visualizer (seção 37-40) e devolve o
    /// `MediaId` que a referencia.
    pub fn add_audio_visualizer(
        &mut self,
        settings: AudioVisualizerSettings,
        duration: Time,
    ) -> MediaId {
        let id = MediaId::new();
        self.generated.insert(
            id,
            GeneratedMediaEntry {
                kind: GeneratedMedia::AudioVisualizer(settings),
                duration,
            },
        );
        id
    }

    /// Importa uma mídia pro projeto (não a copia — só registra o caminho e
    /// os metadados, seção 2.3).
    pub fn add_media(&mut self, media: Media) -> MediaId {
        let id = media.id;
        self.media.insert(id, media);
        id
    }

    pub fn get_media(&self, id: MediaId) -> Option<&Media> {
        self.media.get(&id)
    }

    pub fn remove_media(&mut self, id: MediaId) -> Option<Media> {
        self.media.remove(&id)
    }

    /// Adiciona uma track de vídeo no topo da composição (seção 15 — a
    /// última em `video_tracks` é a de cima).
    pub fn add_video_track(&mut self, name: impl Into<String>) -> TrackId {
        let track = Track::new(name, TrackType::Video);
        let id = track.id;
        self.video_tracks.push(track);
        id
    }

    pub fn add_audio_track(&mut self, name: impl Into<String>) -> TrackId {
        let track = Track::new(name, TrackType::Audio);
        let id = track.id;
        self.audio_tracks.push(track);
        id
    }

    /// Próximo nome sequencial pra uma track nova (`V1`, `V2`, ... /
    /// `A1`, `A2`, ...) — mesma convenção que `Application::new` já usa
    /// pras tracks iniciais e que `media_panel::add_overlay_to_timeline`
    /// já usava inline; centralizado aqui pra não duplicar a conta em
    /// cada lugar que precisa criar uma track nova (botão manual,
    /// auto-criação ao soltar mídia sem track compatível).
    pub fn next_video_track_name(&self) -> String {
        format!("V{}", self.video_tracks.len() + 1)
    }

    pub fn next_audio_track_name(&self) -> String {
        format!("A{}", self.audio_tracks.len() + 1)
    }

    pub fn find_track(&self, id: TrackId) -> Option<&Track> {
        self.video_tracks
            .iter()
            .chain(self.audio_tracks.iter())
            .find(|track| track.id == id)
    }

    pub fn find_track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.video_tracks
            .iter_mut()
            .chain(self.audio_tracks.iter_mut())
            .find(|track| track.id == id)
    }

    /// Remove uma track (de vídeo ou áudio, tanto faz) com todos os clipes
    /// nela — não precisa saber o tipo antes de chamar, já que `TrackId` é
    /// único entre as duas listas (igual `find_track`).
    pub fn remove_track(&mut self, id: TrackId) {
        self.video_tracks.retain(|t| t.id != id);
        self.audio_tracks.retain(|t| t.id != id);
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new(ProjectSettings::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::media::MediaMetadata;

    #[test]
    fn default_settings_match_the_suggested_mvp_config() {
        let settings = ProjectSettings::default();
        assert_eq!(settings.width, 1920);
        assert_eq!(settings.height, 1080);
        assert_eq!(settings.fps, Rational::new(30, 1));
        assert_eq!(settings.sample_rate, 48_000);
        assert_eq!(settings.channels, 2);
    }

    #[test]
    fn project_can_exist_without_any_real_media() {
        // Seção 87: "O projeto pode existir sem mídia real."
        let project = Project::default();
        assert!(project.media.is_empty());
        assert!(project.video_tracks.is_empty());
        assert!(project.audio_tracks.is_empty());
    }

    #[test]
    fn add_media_round_trips_through_get_media() {
        let mut project = Project::default();
        let id = project.add_media(Media::new("video.mp4", MediaMetadata::default()));
        assert_eq!(
            project.get_media(id).map(|m| &m.path),
            Some(&"video.mp4".into())
        );
    }

    #[test]
    fn video_tracks_stack_bottom_to_top_in_insertion_order() {
        let mut project = Project::default();
        let v1 = project.add_video_track("V1");
        let v2 = project.add_video_track("V2");

        assert_eq!(project.video_tracks[0].id, v1);
        assert_eq!(project.video_tracks[1].id, v2);
    }

    #[test]
    fn find_track_looks_across_both_video_and_audio() {
        let mut project = Project::default();
        let video = project.add_video_track("V1");
        let audio = project.add_audio_track("A1");

        assert_eq!(project.find_track(video).map(|t| t.id), Some(video));
        assert_eq!(project.find_track(audio).map(|t| t.id), Some(audio));
    }

    #[test]
    fn next_track_names_count_only_their_own_type() {
        let mut project = Project::default();
        assert_eq!(project.next_video_track_name(), "V1");
        assert_eq!(project.next_audio_track_name(), "A1");

        project.add_video_track("V1");
        // Adicionar vídeo não deve afetar a contagem de áudio, e vice-versa
        // — cada tipo numera a partir do próprio total, não de todas as
        // tracks juntas.
        assert_eq!(project.next_video_track_name(), "V2");
        assert_eq!(project.next_audio_track_name(), "A1");

        project.add_audio_track("A1");
        assert_eq!(project.next_audio_track_name(), "A2");
    }
}
