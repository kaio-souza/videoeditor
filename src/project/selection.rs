//! Ver CLAUDE.md seção 54 — Selection.

use std::collections::HashSet;

use super::clip::ClipId;
use super::track::TrackId;

/// O que está selecionado na UI. Não é parte de `Project` (não é dado de
/// projeto, é estado de UI) — vive em `Application`/`AppState`, mas o tipo
/// mora aqui porque referencia `ClipId`/`TrackId`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub clips: HashSet<ClipId>,
    pub track: Option<TrackId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.track.is_none()
    }

    pub fn clear(&mut self) {
        self.clips.clear();
        self.track = None;
    }

    /// Seleciona só `clip`, substituindo qualquer seleção de clip anterior
    /// (comportamento de clique simples — seção 54; shift/drag select ficam
    /// pra depois).
    pub fn select_only(&mut self, clip: ClipId) {
        self.clips.clear();
        self.clips.insert(clip);
    }

    pub fn add_clip(&mut self, clip: ClipId) {
        self.clips.insert(clip);
    }

    pub fn remove_clip(&mut self, clip: ClipId) {
        self.clips.remove(&clip);
    }

    pub fn is_clip_selected(&self, clip: ClipId) -> bool {
        self.clips.contains(&clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_has_no_clips_and_no_track() {
        let selection = Selection::default();
        assert!(selection.is_empty());
    }

    #[test]
    fn select_only_replaces_previous_clip_selection() {
        let mut selection = Selection::default();
        let a = ClipId::new();
        let b = ClipId::new();

        selection.select_only(a);
        assert!(selection.is_clip_selected(a));

        selection.select_only(b);
        assert!(!selection.is_clip_selected(a));
        assert!(selection.is_clip_selected(b));
    }

    #[test]
    fn add_clip_extends_a_multi_selection() {
        let mut selection = Selection::default();
        let a = ClipId::new();
        let b = ClipId::new();

        selection.select_only(a);
        selection.add_clip(b);

        assert!(selection.is_clip_selected(a));
        assert!(selection.is_clip_selected(b));
        assert!(!selection.is_empty());
    }

    #[test]
    fn clear_empties_both_clip_and_track_selection() {
        let mut selection = Selection::default();
        selection.add_clip(ClipId::new());
        selection.track = Some(TrackId::new());

        selection.clear();

        assert!(selection.is_empty());
    }
}
