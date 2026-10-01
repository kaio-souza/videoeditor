//! Ver CLAUDE.md seção 35-40 — Media Generator (Solid Color, Audio Visualizer).

use serde::{Deserialize, Serialize};

use super::time::Time;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SolidColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisualizerAlignment {
    /// Barras ancoradas na base do frame, crescendo pra cima.
    Base,
    /// Barras simétricas em torno do centro vertical.
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioVisualizerSettings {
    pub bar_count: u32,
    pub sensitivity: f32,
    pub smoothing: f32,
    pub color: SolidColor,
    /// `None` = fundo transparente (seção 40 — "Background"): só o desenho
    /// do visualizador fica opaco, o resto deixa a camada de baixo aparecer.
    pub background: Option<SolidColor>,
    /// Altura máxima das barras, fração do frame (0.0-1.0) — seção 40
    /// "Bar Width"/altura.
    pub height: f32,
    pub alignment: VisualizerAlignment,
    /// Margem horizontal, fração do frame de cada lado (0.0-0.4) — seção 40
    /// "Bar Width"/margem: sem isso o desenho sempre ia de ponta a ponta do
    /// frame. `#[serde(default)]` (=0.0, sem margem) pra projetos salvos
    /// antes desse campo existir continuarem carregando com a aparência
    /// idêntica a antes, em vez de falhar o load ou pular margem sem avisar.
    #[serde(default)]
    pub padding: f32,
}

impl Default for AudioVisualizerSettings {
    fn default() -> Self {
        Self {
            bar_count: 32,
            sensitivity: 1.0,
            smoothing: 0.5,
            color: SolidColor {
                r: 0x4f,
                g: 0x9c,
                b: 0xff,
            },
            background: Some(SolidColor { r: 0, g: 0, b: 0 }),
            height: 1.0,
            alignment: VisualizerAlignment::Base,
            padding: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GeneratedMedia {
    SolidColor(SolidColor),
    AudioVisualizer(AudioVisualizerSettings),
}

/// Mídia gerada com sua duração — igual a `Media`, mas sem arquivo de
/// origem (seção 35). Vive numa coleção separada de `Project::media` porque
/// não tem `path`/metadados de decoder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedMediaEntry {
    pub kind: GeneratedMedia,
    pub duration: Time,
}
