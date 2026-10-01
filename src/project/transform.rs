//! Ver CLAUDE.md seção 19 — Transform.

use serde::{Deserialize, Serialize};

/// Posição, escala e rotação de um clip na composição. No MVP só
/// posição/escala são realmente usadas pelo compositor (seção 19); rotação
/// existe desde já pra não exigir migração de dado depois.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position_x: f32,
    pub position_y: f32,

    pub scale_x: f32,
    pub scale_y: f32,

    pub rotation: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position_x: 0.0,
            position_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
        }
    }
}

impl Transform {
    /// Sem pan/crop/zoom nenhum — mostra o frame fonte inteiro, como antes
    /// do Pan/Crop existir. Usado pra pular o filtro extra quando não faz
    /// diferença.
    pub fn is_identity(&self) -> bool {
        *self == Transform::default()
    }

    /// Filtro `crop` do ffmpeg (seção 19 — Pan/Crop, estilo Sony Vegas):
    /// recorta uma região do frame fonte do tamanho `1/scale` centrada em
    /// `position` (offset em frações do próprio frame, `0.0` = centro) e
    /// devolve `None` quando é identidade (não recorta nada, pula o
    /// filtro). `iw`/`ih` são variáveis do próprio `crop` do ffmpeg — não
    /// precisa saber a resolução real do source pra montar a expressão.
    pub fn crop_filter_expr(&self) -> Option<String> {
        if self.is_identity() {
            return None;
        }
        let sx = self.scale_x.max(0.01);
        let sy = self.scale_y.max(0.01);
        Some(format!(
            "crop=iw/{sx}:ih/{sy}:(iw-iw/{sx})/2+{px}*iw:(ih-ih/{sy})/2+{py}*ih",
            px = self.position_x,
            py = self.position_y,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_identity_and_skips_the_filter() {
        assert!(Transform::default().is_identity());
        assert_eq!(Transform::default().crop_filter_expr(), None);
    }

    #[test]
    fn zoomed_transform_is_not_identity_and_has_a_filter() {
        let t = Transform {
            scale_x: 2.0,
            scale_y: 2.0,
            ..Transform::default()
        };
        assert!(!t.is_identity());
        let expr = t.crop_filter_expr().unwrap();
        assert!(expr.starts_with("crop=iw/2"));
    }
}
