//! Ver CLAUDE.md seção 17-18 — Clip, AudioClip. `Clip::source_time_at`
//! implementa o Source Time Mapping da seção 25/78 — o núcleo que Trim,
//! Split, Reverse e Fade vão todos usar (seção 117): nunca duplicar essa
//! conta em outro lugar do pipeline.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::media::MediaId;
use super::time::Time;
use super::transform::Transform;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClipId(Uuid);

impl ClipId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ClipId {
    fn default() -> Self {
        Self::new()
    }
}

/// Um clip de vídeo (ou a base de um `AudioClip`) na timeline. Não
/// destrutivo: `source_start`/`source_duration` recortam a `Media` original
/// sem modificá-la (seção 2.3); Split cria dois `Clip`s apontando pro mesmo
/// `source`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,

    pub source: MediaId,

    pub timeline_start: Time,
    pub timeline_duration: Time,

    pub source_start: Time,
    pub source_duration: Time,

    pub transform: Transform,

    pub opacity: f32,

    /// Volume do clipe (seção 18/33) — só relevante quando o clip está numa
    /// track de áudio; vídeo ignora. Fica direto no `Clip` comum em vez de
    /// só no `AudioClip` (que não é usado pra guardar clips — ver nota ali)
    /// pra não duplicar toda a struct só por causa de um campo.
    pub volume: f32,

    pub fade_in: Time,
    pub fade_out: Time,

    pub reversed: bool,
}

impl Clip {
    pub fn new(
        source: MediaId,
        timeline_start: Time,
        timeline_duration: Time,
        source_start: Time,
        source_duration: Time,
    ) -> Self {
        Self {
            id: ClipId::new(),
            source,
            timeline_start,
            timeline_duration,
            source_start,
            source_duration,
            transform: Transform::default(),
            opacity: 1.0,
            volume: 1.0,
            fade_in: Time::ZERO,
            fade_out: Time::ZERO,
            reversed: false,
        }
    }

    /// Fim do clip na timeline (`timeline_start + timeline_duration`).
    pub fn timeline_end(&self) -> Time {
        self.timeline_start + self.timeline_duration
    }

    /// Se o instante `t` da timeline cai dentro do clip (`[start, end)`).
    pub fn contains(&self, t: Time) -> bool {
        t >= self.timeline_start && t < self.timeline_end()
    }

    /// Source Time Mapping (spec §25/§78): normal = `source_start +
    /// local_time`, reverse = `source_end - local_time`.
    pub fn source_time_at(&self, t: Time) -> Option<Time> {
        if !self.contains(t) {
            return None;
        }
        let local = t - self.timeline_start;
        Some(if self.reversed {
            (self.source_start + self.source_duration) - local
        } else {
            self.source_start + local
        })
    }
}

/// Um clip de áudio — envolve um `Clip` com os campos específicos de áudio
/// (seção 18). `volume`/`fade_in`/`fade_out` aqui são os do clip; a track
/// tem os seus próprios (seção 33), o ganho final combina os dois.
///
/// Ainda não usado: `Track.clips` guarda `Clip` puro pra vídeo e áudio (ver
/// `track.rs`) — volume por-clipe de áudio ainda não tem UI própria.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    pub clip: Clip,
    pub volume: f32,
    pub fade_in: Time,
    pub fade_out: Time,
}

#[allow(dead_code)]
impl AudioClip {
    pub fn new(clip: Clip) -> Self {
        Self {
            clip,
            volume: 1.0,
            fade_in: Time::ZERO,
            fade_out: Time::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_10s() -> Clip {
        // Seção 73: source duration = 10s, clip cheio na timeline de 0 a 10s.
        Clip::new(
            MediaId::new(),
            Time::ZERO,
            Time::from_secs_f64(10.0),
            Time::ZERO,
            Time::from_secs_f64(10.0),
        )
    }

    #[test]
    fn normal_mapping_matches_spec_example() {
        let clip = clip_10s();
        assert_eq!(
            clip.source_time_at(Time::from_secs_f64(0.0)),
            Some(Time::from_secs_f64(0.0))
        );
        assert_eq!(
            clip.source_time_at(Time::from_secs_f64(5.0)),
            Some(Time::from_secs_f64(5.0))
        );
        // timeline 10s está fora do intervalo semiaberto [start, end) — é o
        // instante logo depois do último frame do clip.
        assert_eq!(clip.source_time_at(Time::from_secs_f64(10.0)), None);
    }

    #[test]
    fn reverse_mapping_matches_spec_example() {
        let mut clip = clip_10s();
        clip.reversed = true;

        assert_eq!(
            clip.source_time_at(Time::from_secs_f64(0.0)),
            Some(Time::from_secs_f64(10.0))
        );
        assert_eq!(
            clip.source_time_at(Time::from_secs_f64(5.0)),
            Some(Time::from_secs_f64(5.0))
        );
    }

    #[test]
    fn reverse_after_trim_maps_within_trimmed_range() {
        // Seção 27: source A..J (10s), trim pra C..H (5s a partir de 2s).
        let mut clip = Clip::new(
            MediaId::new(),
            Time::ZERO,
            Time::from_secs_f64(5.0),
            Time::from_secs_f64(2.0),
            Time::from_secs_f64(5.0),
        );
        clip.reversed = true;

        // timeline 0s (início do clip trimado) deve mapear pro fim do
        // trecho fonte trimado (7s), não do source inteiro (10s).
        assert_eq!(
            clip.source_time_at(Time::ZERO),
            Some(Time::from_secs_f64(7.0))
        );
        // timeline 4s (1s antes do fim do clip de 5s) mapeia pro instante
        // simétrico dentro do trecho fonte trimado: 2s + 5s - 4s = 3s.
        assert_eq!(
            clip.source_time_at(Time::from_secs_f64(4.0)),
            Some(Time::from_secs_f64(3.0))
        );
        // timeline_end (5s) já está fora do intervalo semiaberto do clip.
        assert_eq!(clip.source_time_at(clip.timeline_end()), None);
    }

    #[test]
    fn source_time_outside_clip_is_none() {
        let clip = clip_10s();
        assert_eq!(clip.source_time_at(Time::from_secs_f64(-1.0)), None);
        assert_eq!(clip.source_time_at(Time::from_secs_f64(10.5)), None);
    }

    #[test]
    fn split_keeps_both_halves_pointing_at_the_same_media() {
        // Seção 20: split não duplica a mídia — os dois clips apontam pro
        // mesmo MediaId.
        let source = MediaId::new();
        let original = Clip::new(
            source,
            Time::ZERO,
            Time::from_secs_f64(10.0),
            Time::ZERO,
            Time::from_secs_f64(10.0),
        );

        let split_at = Time::from_secs_f64(4.0);
        let local_split = split_at - original.timeline_start;

        let left = Clip::new(
            original.source,
            original.timeline_start,
            local_split,
            original.source_start,
            local_split,
        );
        let right = Clip::new(
            original.source,
            split_at,
            original.timeline_duration - local_split,
            original.source_start + local_split,
            original.source_duration - local_split,
        );

        assert_eq!(left.source, source);
        assert_eq!(right.source, source);
        assert_ne!(left.id, right.id);
        assert_eq!(left.timeline_end(), right.timeline_start);
    }
}
