use crate::app::Application;
use crate::project::{Clip, ClipId, MediaId, Time};

const MIN_DURATION: Time = Time::from_ticks(1_764_000); // 0.05s

impl Application {
    fn find_clip_mut(&mut self, clip_id: ClipId) -> Option<&mut Clip> {
        self.project
            .video_tracks
            .iter_mut()
            .chain(self.project.audio_tracks.iter_mut())
            .find_map(|t| t.clips.iter_mut().find(|c| c.id == clip_id))
    }

    /// Arrasta a borda esquerda do clip: encurta por trás, preservando o
    /// ponto do source que fica visível a partir dali (seção 21).
    pub fn trim_clip_left(&mut self, clip_id: ClipId, delta: Time) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        let max_delta = clip.timeline_duration - MIN_DURATION;
        let min_delta = Time::ZERO - clip.timeline_start;
        let applied = clamp_time(delta, min_delta, max_delta);

        clip.timeline_start += applied;
        clip.timeline_duration -= applied;
        clip.source_start += applied;
        clip.source_duration -= applied;
    }

    /// Arrasta a borda direita do clip: encurta pela frente (seção 21).
    pub fn trim_clip_right(&mut self, clip_id: ClipId, delta: Time) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        let min_delta = MIN_DURATION - clip.timeline_duration;
        let applied = clamp_time(delta, min_delta, Time::from_ticks(i64::MAX / 2));

        clip.timeline_duration += applied;
        clip.source_duration += applied;
    }

    pub fn move_clip(&mut self, clip_id: ClipId, delta: Time) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        let min_delta = Time::ZERO - clip.timeline_start;
        let applied = clamp_time(delta, min_delta, Time::from_ticks(i64::MAX / 2));
        clip.timeline_start += applied;
    }

    /// Facilitadores de fade/opacidade/volume arrastáveis direto no clip da
    /// timeline (estilo Sony Vegas — pegar a alcinha do canto ou a linha
    /// horizontal em vez de abrir o Inspector). Ver `ui/timeline.rs`.
    pub fn set_clip_fade_in(&mut self, clip_id: ClipId, value: Time) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        clip.fade_in = clamp_time(value, Time::ZERO, clip.timeline_duration);
    }

    pub fn set_clip_fade_out(&mut self, clip_id: ClipId, value: Time) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        clip.fade_out = clamp_time(value, Time::ZERO, clip.timeline_duration);
    }

    pub fn set_clip_opacity(&mut self, clip_id: ClipId, value: f32) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        clip.opacity = value.clamp(0.0, 1.0);
    }

    pub fn set_clip_volume(&mut self, clip_id: ClipId, value: f32) {
        let Some(clip) = self.find_clip_mut(clip_id) else {
            return;
        };
        clip.volume = value.clamp(0.0, 2.0);
    }

    pub fn split_selected_at_playhead(&mut self) {
        let playhead = self.playhead;
        let selected: Vec<_> = self.selection.clips.iter().copied().collect();
        if selected.is_empty() {
            return;
        }

        self.push_undo();

        for track in self
            .project
            .video_tracks
            .iter_mut()
            .chain(self.project.audio_tracks.iter_mut())
        {
            let Some(index) = track.clips.iter().position(|c| {
                selected.contains(&c.id) && c.contains(playhead) && c.timeline_start != playhead
            }) else {
                continue;
            };

            let original = track.clips[index].clone();
            let local = playhead - original.timeline_start;

            let left = Clip {
                id: crate::project::ClipId::default(),
                timeline_duration: local,
                source_duration: local,
                ..original.clone()
            };
            let right_source_start = if original.reversed {
                original.source_start
            } else {
                original.source_start + local
            };
            let right = Clip {
                id: crate::project::ClipId::default(),
                timeline_start: playhead,
                timeline_duration: original.timeline_duration - local,
                source_start: right_source_start,
                source_duration: original.source_duration - local,
                ..original.clone()
            };

            track.clips.splice(index..=index, [left, right]);
        }
    }

    pub fn delete_selected_clips(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let selected = self.selection.clips.clone();
        self.push_undo();
        for track in self
            .project
            .video_tracks
            .iter_mut()
            .chain(self.project.audio_tracks.iter_mut())
        {
            track.clips.retain(|c| !selected.contains(&c.id));
        }
        self.selection.clear();
    }

    /// Duplica cada clip selecionado (seção 54 selection + Ctrl+D estilo
    /// concorrentes — CapCut, Resolve, Premiere via botão direito "Duplicate").
    /// Cópia não destrutiva: mesma `MediaId`/`source_start`/`source_duration`,
    /// nunca duplica o arquivo original. A cópia entra logo após o clip
    /// original na mesma track (`timeline_start + timeline_duration`) —
    /// como não há checagem de overlap em nenhum outro comando (`move_clip`
    /// já permite sobrepor), colar encostado é só o ponto de partida óbvio,
    /// o usuário arrasta se quiser outro lugar. Seleção final vira as
    /// cópias novas, não os originais, pra já poder arrastar/ajustar.
    pub fn duplicate_selected_clips(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let selected = self.selection.clips.clone();
        self.push_undo();
        let mut new_ids = Vec::new();
        let project = &mut self.project;
        for track in project
            .video_tracks
            .iter_mut()
            .chain(project.audio_tracks.iter_mut())
        {
            let originals: Vec<Clip> = track
                .clips
                .iter()
                .filter(|c| selected.contains(&c.id))
                .cloned()
                .collect();
            for original in originals {
                // Mídia gerada (Cor Sólida/Visualizador) guarda seus
                // parâmetros no `GeneratedMediaEntry`, indexado por
                // `MediaId` — não no `Clip`. Se a cópia continuasse
                // apontando pro mesmo `MediaId`, editar a cor de um dos
                // dois no Inspector mudaria os dois juntos (usuário
                // relatou "itens vinculados"). Mídia real (arquivo) não
                // tem esse problema — opacidade/fade/volume/transform já
                // vivem no `Clip`, então dois clips apontando pro mesmo
                // arquivo é comportamento correto de NLE, não "linkado".
                let source = if let Some(entry) = project.generated.get(&original.source) {
                    let new_media_id = MediaId::new();
                    project.generated.insert(new_media_id, entry.clone());
                    new_media_id
                } else {
                    original.source
                };
                let copy = Clip {
                    id: ClipId::default(),
                    source,
                    timeline_start: original.timeline_start + original.timeline_duration,
                    ..original
                };
                new_ids.push(copy.id);
                track.clips.push(copy);
            }
        }
        if !new_ids.is_empty() {
            self.selection.clips = new_ids.into_iter().collect();
            self.selection.track = None;
        }
    }

    pub fn toggle_reverse_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let selected = self.selection.clips.clone();
        self.push_undo();
        for track in self
            .project
            .video_tracks
            .iter_mut()
            .chain(self.project.audio_tracks.iter_mut())
        {
            for clip in track.clips.iter_mut().filter(|c| selected.contains(&c.id)) {
                clip.reversed = !clip.reversed;
            }
        }
    }

    /// Adiciona uma track de vídeo nova (`V1`, `V2`, ... — próximo número
    /// livre, seção 14). Extraído pra `commands.rs` pra não duplicar entre
    /// o botão "➕🎬" da timeline e o menu Inserir.
    pub fn add_video_track(&mut self) {
        self.push_undo();
        let name = self.project.next_video_track_name();
        self.project.add_video_track(name);
    }

    /// Mesma ideia de `add_video_track`, pra áudio (`A1`, `A2`, ...).
    pub fn add_audio_track(&mut self) {
        self.push_undo();
        let name = self.project.next_audio_track_name();
        self.project.add_audio_track(name);
    }
}

fn clamp_time(value: Time, min: Time, max: Time) -> Time {
    if min.ticks() > max.ticks() {
        return min;
    }
    Time::from_ticks(value.ticks().clamp(min.ticks(), max.ticks()))
}
