//! Diálogo de Pan/Crop (seção 19), estilo Sony Vegas — usuário pediu "zoom
//! em uma parte do video/foto" + "modal de crop/panorâmica igual ao
//! Vegas". Reaproveita `Clip::transform` (existia na struct desde a spec,
//! sem nenhum consumidor até agora): `scale_x`/`scale_y` = fator de zoom
//! (1.0 = frame inteiro), `position_x`/`position_y` = deslocamento do
//! centro do recorte, em frações do **source original** (`0.0` = centrado).
//!
//! O diálogo mostra o frame inteiro do source sem cortar nada
//! (`decoder::decode_frame_letterboxed`) — é o que deixa escolher qualquer
//! região, incluindo partes que o crop-fit "estilo Vegas" padrão (usado
//! quando não há Pan/Crop) teria descartado. Usuário relatou exatamente
//! essa lacuna numa primeira versão que mostrava o frame já pré-cortado
//! pro aspect ratio do projeto: numa foto 9:16 num projeto 16:9, as bordas
//! já tinham sumido antes do diálogo abrir.
//!
//! O canvas do diálogo não é um tamanho fixo — é calculado em `open()` pro
//! aspect ratio *do source*, ocupando o máximo de `MAX_CANVAS` sem sobrar
//! nem faltar (`force_original_aspect_ratio=decrease` do ffmpeg então nunca
//! precisa preencher barra de letterbox nenhuma). Usuário relatou a
//! primeira versão (canvas fixo 640x480 sem relação com o source) deixando
//! a imagem "bem menor que a original", difícil de posicionar o
//! retângulo — uma fonte vertical, por exemplo, ocupava só uma faixa
//! estreita no meio de um canvas largo, sobrando bastante barra preta.

use crate::app::Application;
use crate::i18n::tr;
use crate::project::{ClipId, Transform};
use crate::ui::theme;
use egui::{Color32, Sense, Stroke, StrokeKind};

/// Canvas nunca passa disso, não importa o aspect ratio do source — janela
/// não pode ficar maior que a tela em fontes muito largas/altas. Também
/// limitado pela altura de viewport disponível em `show()` (usuário
/// relatou "modal ficou muito grande não consigo clicar nos botões" com
/// esse valor sozinho — fonte vertical batia perto de 620px de canvas e,
/// somado a hint/zoom/botões, estourava a tela).
const MAX_CANVAS: egui::Vec2 = egui::vec2(760.0, 520.0);
/// Raio de captura de uma alcinha de canto — mesma ideia de `HANDLE_GRAB`
/// em `ui/timeline.rs`, mas em coordenadas de tela do diálogo.
const HANDLE_GRAB: f32 = 10.0;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 6.0;

/// Abre o diálogo pro clipe `clip_id` — decodifica o frame base uma única
/// vez (não em todo frame de UI, `ffmpeg` é síncrono) e prepara o rascunho
/// a partir do `Transform` atual do clipe.
pub fn open(app: &mut Application, clip_id: ClipId, ctx: &egui::Context) {
    let Some((source, source_time, transform)) = app
        .project
        .video_tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id == clip_id)
        .map(|c| {
            let t = c.source_time_at(app.playhead).unwrap_or(c.source_start);
            (c.source, t, c.transform)
        })
    else {
        return;
    };
    // Mídia gerada (Cor Sólida/Visualizador) não tem "região do source" pra
    // recortar — Pan/Crop só faz sentido pra vídeo/foto de verdade.
    let Some(media) = app.project.get_media(source) else {
        return;
    };
    // Precisa da resolução real do source pra travar o aspect ratio do
    // retângulo corretamente (ver `combined_aspect` em `show`) — sem isso
    // não dá pra saber onde a imagem letterboxed cai dentro do canvas.
    let Some(sw) = media.metadata.width else {
        app.status_message = Some(tr("pan_crop.decode_failed"));
        return;
    };
    let Some(sh) = media.metadata.height else {
        app.status_message = Some(tr("pan_crop.decode_failed"));
        return;
    };
    // Canvas do tamanho exato do aspect ratio do source (até `MAX_CANVAS`)
    // — sem isso a imagem decodificada sobra pequena dentro de um canvas
    // de proporção genérica, com barra de letterbox desperdiçando espaço
    // que podia ser imagem de verdade.
    let scale = (MAX_CANVAS.x / sw as f32).min(MAX_CANVAS.y / sh as f32);
    let canvas = (
        ((sw as f32 * scale).round() as u32).max(1),
        ((sh as f32 * scale).round() as u32).max(1),
    );
    let decoded = crate::media::decoder::decode_frame_letterboxed(&media.path, source_time, canvas);
    let Ok(frame) = decoded else {
        app.status_message = Some(tr("pan_crop.decode_failed"));
        return;
    };
    let image = egui::ColorImage::from_rgb([canvas.0 as usize, canvas.1 as usize], &frame.rgb);
    app.pan_crop_texture =
        Some(ctx.load_texture("pan_crop_source", image, egui::TextureOptions::LINEAR));
    app.pan_crop_draft = transform;
    app.pan_crop_source_size = (sw, sh);
    app.pan_crop_clip = Some(clip_id);
}

fn close(app: &mut Application) {
    app.pan_crop_clip = None;
    app.pan_crop_texture = None;
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragMode {
    Move,
    ResizeCorner(Corner),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Retângulo em pixels de tela, dentro de `canvas`, onde a imagem do
/// source realmente aparece — o resto são as barras de letterbox (mesmo
/// cálculo que o filtro `pad` do ffmpeg já faz em `decode_frame_letterboxed`,
/// refeito aqui só pra saber onde desenhar/travar o retângulo por cima).
fn letterbox_inner(canvas_rect: egui::Rect, source_size: (u32, u32)) -> egui::Rect {
    let (sw, sh) = (source_size.0 as f32, source_size.1 as f32);
    let scale = (canvas_rect.width() / sw).min(canvas_rect.height() / sh);
    let inner_size = egui::vec2(sw * scale, sh * scale);
    let pad = (canvas_rect.size() - inner_size) / 2.0;
    egui::Rect::from_min_size(canvas_rect.min + pad, inner_size)
}

/// Retângulo normalizado (`0.0..=1.0`, frações do source original) que o
/// `Transform` atual representa.
fn draft_rect(transform: Transform) -> egui::Rect {
    let w = (1.0 / transform.scale_x.max(0.01)).clamp(0.01, 1.0);
    let h = (1.0 / transform.scale_y.max(0.01)).clamp(0.01, 1.0);
    let cx = 0.5 + transform.position_x;
    let cy = 0.5 + transform.position_y;
    egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(w, h))
}

fn rect_to_transform(rect: egui::Rect) -> Transform {
    Transform {
        position_x: rect.center().x - 0.5,
        position_y: rect.center().y - 0.5,
        scale_x: (1.0 / rect.width().max(0.01)).clamp(MIN_ZOOM, MAX_ZOOM * 4.0),
        scale_y: (1.0 / rect.height().max(0.01)).clamp(MIN_ZOOM, MAX_ZOOM * 4.0),
        rotation: 0.0,
    }
}

pub fn show(app: &mut Application, ctx: &egui::Context) {
    let Some(clip_id) = app.pan_crop_clip else {
        return;
    };
    let Some(texture) = app.pan_crop_texture.clone() else {
        return;
    };

    // Aspect combinado: uma fração `w x h` do source só sai sem distorcer
    // quando escalada pro projeto se essa fração, em *pixels do source*,
    // já bate com o aspect ratio do projeto — como o retângulo é medido em
    // frações do source (não do projeto), a razão altura/largura que
    // mantém isso depende dos dois aspect ratios juntos, não só do
    // projeto (era o bug: usar só o aspect do projeto direto nas frações
    // do source distorcia sempre que os dois aspect ratios diferiam).
    let (sw, sh) = app.pan_crop_source_size;
    let (pw, ph) = (app.project.settings.width, app.project.settings.height);
    let combined_aspect = (sw as f32 * ph as f32) / (sh as f32 * pw as f32);
    // Tamanho real do canvas, decidido em `open()` (aspect ratio do source,
    // não um valor fixo) — o `TextureHandle` já sabe o tamanho exato que
    // decodificou, não precisa duplicar essa conta aqui.
    let canvas_size = texture.size_vec2();

    let mut open = true;
    let mut apply_clicked = false;
    let mut cancel_clicked = false;
    let mut reset_clicked = false;

    // Mesmo padrão do diálogo de Export/Preferences (também tiveram esse
    // exato bug antes): altura da janela nunca passa da viewport, conteúdo
    // rola dentro de um `ScrollArea` — só a linha de botões fica sempre
    // fixa/visível fora dele, nunca empurrada pra fora da tela.
    let max_window_height = (ctx.viewport_rect().height() - 80.0).max(240.0);

    egui::Window::new(tr("pan_crop.title"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .max_height(max_window_height)
        .show(ctx, |ui| {
            ui.set_min_width(canvas_size.x + 20.0);
            egui::ScrollArea::vertical()
                .max_height(max_window_height - 90.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(tr("pan_crop.hint"))
                            .small()
                            .color(theme::MUTED),
                    );
                    ui.add_space(6.0);

                    let (canvas_rect, response) =
                        ui.allocate_exact_size(canvas_size, Sense::click_and_drag());
                    let painter = ui.painter_at(canvas_rect);
                    painter.rect_filled(canvas_rect, 0.0, Color32::BLACK);
                    painter.image(
                        texture.id(),
                        canvas_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    // Região onde a imagem de verdade aparece dentro do canvas —
                    // o retângulo de Pan/Crop só pode existir aqui dentro, nunca
                    // nas barras de letterbox.
                    let inner = letterbox_inner(canvas_rect, (sw, sh));

                    let mut draft = app.pan_crop_draft;
                    let norm_rect = draft_rect(draft);
                    let screen_rect = egui::Rect::from_min_max(
                        inner.min + norm_rect.min.to_vec2() * inner.size(),
                        inner.min + norm_rect.max.to_vec2() * inner.size(),
                    );

                    let widget_id = response.id;
                    if response.drag_started()
                        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
                    {
                        let mode = hit_test(pos, screen_rect);
                        ui.ctx().data_mut(|d| d.insert_temp(widget_id, mode));
                    }
                    if response.dragged()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        let mode = ui
                            .ctx()
                            .data(|d| d.get_temp::<DragMode>(widget_id))
                            .unwrap_or(DragMode::Move);
                        let local = ((pos - inner.min) / inner.size()).to_pos2();
                        let new_rect = match mode {
                            DragMode::Move => {
                                let half = norm_rect.size() / 2.0;
                                let center = local.clamp(
                                    egui::pos2(half.x, half.y),
                                    egui::pos2(1.0 - half.x, 1.0 - half.y),
                                );
                                egui::Rect::from_center_size(center, norm_rect.size())
                            }
                            DragMode::ResizeCorner(corner) => {
                                let anchor = match corner {
                                    Corner::TopLeft => norm_rect.max,
                                    Corner::TopRight => norm_rect.left_bottom(),
                                    Corner::BottomLeft => norm_rect.right_top(),
                                    Corner::BottomRight => norm_rect.min,
                                };
                                // Largura livre, altura derivada da largura pra manter
                                // o aspect ratio combinado travado — o retângulo
                                // nunca pode distorcer a imagem final.
                                let w = (local.x - anchor.x).abs().clamp(0.05, 1.0);
                                let h = w * combined_aspect;
                                let (min, max) = match corner {
                                    Corner::TopLeft => {
                                        (egui::pos2(anchor.x - w, anchor.y - h), anchor)
                                    }
                                    Corner::TopRight => (
                                        egui::pos2(anchor.x, anchor.y - h),
                                        egui::pos2(anchor.x + w, anchor.y),
                                    ),
                                    Corner::BottomLeft => (
                                        egui::pos2(anchor.x - w, anchor.y),
                                        egui::pos2(anchor.x, anchor.y + h),
                                    ),
                                    Corner::BottomRight => {
                                        (anchor, egui::pos2(anchor.x + w, anchor.y + h))
                                    }
                                };
                                egui::Rect::from_min_max(min, max)
                            }
                        };
                        // Nunca deixa o retângulo sair do source (frações fora de
                        // `0.0..=1.0`).
                        let clamped_min = new_rect
                            .min
                            .clamp(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        let clamped_max = new_rect
                            .max
                            .clamp(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        if clamped_max.x - clamped_min.x > 0.02
                            && clamped_max.y - clamped_min.y > 0.02
                        {
                            draft = rect_to_transform(egui::Rect::from_min_max(
                                clamped_min,
                                clamped_max,
                            ));
                            app.pan_crop_draft = draft;
                        }
                    }

                    // Cursor de acordo com a zona — mesma ideia de `ui/timeline.rs`.
                    if let Some(pos) = response.hover_pos() {
                        let mode = hit_test(pos, screen_rect);
                        let icon = match mode {
                            DragMode::Move => egui::CursorIcon::Grab,
                            DragMode::ResizeCorner(Corner::TopLeft | Corner::BottomRight) => {
                                egui::CursorIcon::ResizeNwSe
                            }
                            DragMode::ResizeCorner(_) => egui::CursorIcon::ResizeNeSw,
                        };
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(icon);
                        }
                    }

                    // Sombra fora do retângulo + contorno — mesmo visual de "janela
                    // de recorte" que qualquer NLE usa. Cobre o canvas inteiro
                    // (inclusive as barras de letterbox), não só `inner` — a área
                    // fora do retângulo escolhido deve ficar visualmente apagada
                    // de qualquer jeito.
                    painter.rect_filled(canvas_rect, 0.0, Color32::from_black_alpha(140));
                    painter.image(
                        texture.id(),
                        screen_rect,
                        egui::Rect::from_min_max(
                            norm_rect.min.to_vec2().to_pos2(),
                            norm_rect.max.to_vec2().to_pos2(),
                        ),
                        Color32::WHITE,
                    );
                    painter.rect_stroke(
                        screen_rect,
                        0.0,
                        Stroke::new(2.0, theme::ACCENT),
                        StrokeKind::Outside,
                    );
                    for corner in [
                        screen_rect.left_top(),
                        screen_rect.right_top(),
                        screen_rect.left_bottom(),
                        screen_rect.right_bottom(),
                    ] {
                        painter.circle_filled(corner, 5.0, theme::ACCENT);
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(tr("pan_crop.zoom"))
                                .small()
                                .color(theme::MUTED),
                        );
                        let mut zoom = draft.scale_x.max(draft.scale_y);
                        if ui
                            .add(egui::Slider::new(&mut zoom, MIN_ZOOM..=MAX_ZOOM).suffix("x"))
                            .changed()
                        {
                            // Slider zoom simétrico a partir do centro atual —
                            // caminho rápido pro pedido original ("só o zoom"), sem
                            // precisar arrastar cantos.
                            let center = norm_rect.center();
                            let w = (1.0 / zoom).clamp(0.01, 1.0);
                            let h = w * combined_aspect;
                            let half = egui::vec2(w, h) / 2.0;
                            let clamped_center = center.clamp(
                                egui::pos2(half.x, half.y),
                                egui::pos2(1.0 - half.x, 1.0 - half.y),
                            );
                            app.pan_crop_draft = rect_to_transform(egui::Rect::from_center_size(
                                clamped_center,
                                egui::vec2(w, h),
                            ));
                        }
                    });
                });

            // Fora do `ScrollArea` de propósito — a linha de botões precisa
            // ficar sempre visível/clicável, nunca empurrada pra fora da
            // janela junto com o resto do conteúdo (esse era exatamente o
            // bug: "modal ficou muito grande não consigo clicar nos
            // botões").
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let apply_label = egui::RichText::new(tr("common.apply"))
                    .color(theme::ON_ACCENT)
                    .strong();
                if ui
                    .add(egui::Button::new(apply_label).fill(theme::ACCENT))
                    .clicked()
                {
                    apply_clicked = true;
                }
                if ui.button(tr("pan_crop.reset")).clicked() {
                    reset_clicked = true;
                }
                if ui.button(tr("common.cancel")).clicked() {
                    cancel_clicked = true;
                }
            });
        });

    if reset_clicked {
        app.pan_crop_draft = Transform::default();
    }
    if apply_clicked {
        app.push_undo();
        if let Some(track) = app
            .project
            .video_tracks
            .iter_mut()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
            && let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id)
        {
            clip.transform = app.pan_crop_draft;
        }
        app.preview_key = None;
        close(app);
    } else if !open || cancel_clicked {
        close(app);
    }
}

fn hit_test(pos: egui::Pos2, rect: egui::Rect) -> DragMode {
    let corners = [
        (rect.left_top(), Corner::TopLeft),
        (rect.right_top(), Corner::TopRight),
        (rect.left_bottom(), Corner::BottomLeft),
        (rect.right_bottom(), Corner::BottomRight),
    ];
    for (corner_pos, corner) in corners {
        if pos.distance(corner_pos) <= HANDLE_GRAB {
            return DragMode::ResizeCorner(corner);
        }
    }
    DragMode::Move
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_transform_is_the_full_frame_rect() {
        let rect = draft_rect(Transform::default());
        assert!((rect.min.x - 0.0).abs() < 1e-6);
        assert!((rect.min.y - 0.0).abs() < 1e-6);
        assert!((rect.max.x - 1.0).abs() < 1e-6);
        assert!((rect.max.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn draft_rect_and_rect_to_transform_round_trip() {
        let original = Transform {
            position_x: 0.1,
            position_y: -0.05,
            scale_x: 2.0,
            scale_y: 2.0,
            rotation: 0.0,
        };
        let rect = draft_rect(original);
        let back = rect_to_transform(rect);
        assert!((back.position_x - original.position_x).abs() < 1e-4);
        assert!((back.position_y - original.position_y).abs() < 1e-4);
        assert!((back.scale_x - original.scale_x).abs() < 1e-4);
        assert!((back.scale_y - original.scale_y).abs() < 1e-4);
    }

    #[test]
    fn hit_test_finds_the_nearest_corner_before_falling_back_to_move() {
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0));
        assert_eq!(
            hit_test(egui::pos2(101.0, 101.0), rect),
            DragMode::ResizeCorner(Corner::TopLeft)
        );
        assert_eq!(
            hit_test(egui::pos2(299.0, 199.0), rect),
            DragMode::ResizeCorner(Corner::BottomRight)
        );
        assert_eq!(hit_test(egui::pos2(200.0, 150.0), rect), DragMode::Move);
    }

    #[test]
    fn letterbox_inner_centers_a_vertical_source_horizontally() {
        // Source 9:16 (vertical) num canvas 640x480 (mais largo que alto)
        // deve sobrar barra preta nas laterais, não em cima/embaixo — e a
        // imagem em si precisa continuar do tamanho inteiro na vertical
        // (altura = canvas inteiro), sem cortar nada.
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(640.0, 480.0));
        let inner = letterbox_inner(canvas, (1080, 1920));
        assert!((inner.height() - 480.0).abs() < 0.01);
        assert!(inner.width() < 640.0);
        assert!(inner.left() > 0.0);
        assert!(inner.right() < 640.0);
    }
}
