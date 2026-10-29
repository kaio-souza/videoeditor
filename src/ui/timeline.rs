use crate::app::Application;
use crate::i18n::tr;
use crate::project::{ClipId, Time, TrackId};
use crate::ui::theme;
use egui::{Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind};

const HEADER_WIDTH: f32 = 100.0;
const TRACK_HEIGHT: f32 = 44.0;
const AUDIO_TRACK_HEIGHT: f32 = 60.0;
const TRACK_GAP: f32 = 5.0;
/// Faixa de cor colada na borda esquerda do cabeçalho, só pra identificar o
/// tipo da track num piscar de olho (vídeo vs áudio) — visual clássico de
/// NLE profissional.
const ACCENT_STRIP: f32 = 3.0;
const VIDEO_COLOR: Color32 = theme::ACCENT;
const AUDIO_COLOR: Color32 = Color32::from_rgb(0x4e, 0xc9, 0x8f);
const EDGE_GRAB: f32 = 6.0;
/// Distância em pixels dentro da qual uma borda arrastada gruda num alvo
/// (seção 53 — Snap).
const SNAP_PIXELS: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragMode {
    TrimLeft,
    TrimRight,
    Move,
    /// Alcinha do canto superior esquerdo (facilitador estilo Vegas) —
    /// arrasta pra direita/esquerda pra aumentar/diminuir `fade_in`.
    FadeIn,
    /// Mesma coisa, canto superior direito, controla `fade_out`.
    FadeOut,
    /// Linha horizontal no meio do clip — arrasta pra cima/baixo pra
    /// controlar opacidade (vídeo) ou volume (áudio) direto, sem abrir o
    /// Inspector.
    Level,
}

/// Dados de um clip só pra desenhar/interagir na timeline — extraídos antes
/// do loop pra não segurar um empréstimo de `app.project` durante o desenho
/// (mesmo padrão que já existia pra tupla anterior, só nomeado agora porque
/// os facilitadores de fade/level precisam de mais campos).
struct ClipRow {
    id: ClipId,
    start: Time,
    duration: Time,
    reversed: bool,
    source: crate::project::MediaId,
    source_start: Time,
    source_duration: Time,
    fade_in: Time,
    fade_out: Time,
    /// Opacidade (vídeo, `0.0..=1.0`) ou volume normalizado pra
    /// `0.0..=1.0` (áudio, `clip.volume / 2.0` já que volume vai até 2.0x)
    /// — mesma escala visual `0.0..=1.0` pros dois tipos, pra reaproveitar
    /// o mesmo desenho/cálculo de linha em `track_row`.
    level: f32,
}

/// Distância em pixels pra "grudar" numa alcinha de fade (facilitador
/// estilo Vegas) — menor que `EDGE_GRAB` porque a alcinha é um alvo
/// pequeno e específico, não a borda inteira do clip.
const HANDLE_GRAB: f32 = 7.0;
const LEVEL_GRAB: f32 = 8.0;
/// Keeps the level line clear of the fade-handle hit zone at the top edge.
const LEVEL_MARGIN: f32 = 10.0;
/// Ícone de Pan/Crop no canto do clipe (seção 19, estilo Vegas — usuário
/// pediu "um ícone na timeline igual no Vegas pra ir para essa modal", em
/// vez de só o menu de contexto).
const PAN_CROP_ICON_SIZE: f32 = 15.0;

/// Decide o que um drag no clip vai fazer, a partir da posição do ponteiro
/// relativa ao retângulo do clip e das posições atuais das alcinhas/linha.
/// Usado tanto pro cursor (hover) quanto pra decidir o modo no
/// `drag_started` — um só lugar pra essa lógica, em vez dos dois blocos
/// quase-iguais que existiam antes de fade/level entrarem (ficaria fácil
/// os dois divergirem).
fn hit_test_drag_mode(
    pos: egui::Pos2,
    clip_rect: egui::Rect,
    fade_in_x: f32,
    fade_out_x: f32,
    level_y: f32,
) -> DragMode {
    let top = clip_rect.top();
    if (pos.x - fade_in_x).abs() <= HANDLE_GRAB && (pos.y - top).abs() <= HANDLE_GRAB {
        return DragMode::FadeIn;
    }
    if (pos.x - fade_out_x).abs() <= HANDLE_GRAB && (pos.y - top).abs() <= HANDLE_GRAB {
        return DragMode::FadeOut;
    }
    if pos.x - clip_rect.left() < EDGE_GRAB {
        return DragMode::TrimLeft;
    }
    if clip_rect.right() - pos.x < EDGE_GRAB {
        return DragMode::TrimRight;
    }
    if (pos.y - level_y).abs() <= LEVEL_GRAB {
        return DragMode::Level;
    }
    DragMode::Move
}

/// Posição em Y da linha de opacidade/volume pra um `level` em
/// `0.0..=1.0` (1.0 = topo, 0.0 = base) — nunca encosta literalmente no
/// topo/base do clip (`LEVEL_MARGIN`), pra não competir pelo mesmo pixel
/// com a faixa de detecção das alcinhas de fade (ver `LEVEL_MARGIN`).
fn level_y_for(rect: egui::Rect, level: f32) -> f32 {
    let usable_h = (rect.height() - 2.0 * LEVEL_MARGIN).max(4.0);
    rect.bottom() - LEVEL_MARGIN - level.clamp(0.0, 1.0) * usable_h
}

/// Inverso de `level_y_for` — a partir de um Y do ponteiro (durante um
/// drag), devolve o `level` em `0.0..=1.0` correspondente.
fn level_t_for(rect: egui::Rect, y: f32) -> f32 {
    let usable_h = (rect.height() - 2.0 * LEVEL_MARGIN).max(4.0);
    (((rect.bottom() - LEVEL_MARGIN) - y) / usable_h).clamp(0.0, 1.0)
}

pub fn show(ui: &mut egui::Ui, app: &mut Application) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr("timeline.header").to_uppercase())
                .small()
                .color(theme::MUTED)
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::icon_button(ui, "➕🔊", 12.0)
                .on_hover_text(tr("timeline.add_audio_track"))
                .clicked()
            {
                app.add_audio_track();
            }
            if theme::icon_button(ui, "➕🎬", 12.0)
                .on_hover_text(tr("timeline.add_video_track"))
                .clicked()
            {
                app.add_video_track();
            }
        });
    });
    ui.add_space(6.0);

    // Ctrl held routes wheel delta into `zoom_delta()`, not
    // `smooth_scroll_delta` (which stays zero) — read the former for zoom.
    let scroll_id = ui.make_persistent_id("timeline_scroll");
    let viewport_left = ui.next_widget_position().x;
    let mut forced_offset: Option<egui::Vec2> = None;

    let zoom_factor = ui.ctx().input(|i| i.zoom_delta());
    let pointer_over_timeline = ui
        .ctx()
        .input(|i| i.pointer.hover_pos())
        .filter(|p| ui.max_rect().contains(*p));
    if zoom_factor != 1.0
        && let Some(pointer) = pointer_over_timeline
    {
        let old_zoom = app.timeline_zoom;
        let new_zoom = (old_zoom * zoom_factor).clamp(4.0, 400.0);
        if let Some(state) = egui::containers::scroll_area::State::load(ui.ctx(), scroll_id) {
            let content_origin_x = viewport_left - state.offset.x;
            let pointer_local_x = pointer.x - content_origin_x;
            let time_at_cursor = ((pointer_local_x - HEADER_WIDTH) / old_zoom).max(0.0);
            let new_offset_x = viewport_left - pointer.x + HEADER_WIDTH + time_at_cursor * new_zoom;
            forced_offset = Some(egui::vec2(new_offset_x.max(0.0), state.offset.y));
        }
        app.timeline_zoom = new_zoom;
    }

    let mut scroll_area = egui::ScrollArea::both()
        .id_salt("timeline_scroll")
        .auto_shrink([false, false]);
    if let Some(offset) = forced_offset {
        scroll_area = scroll_area.scroll_offset(offset);
    }
    scroll_area.show(ui, |ui| {
        ui.set_min_width(ui.available_width().max(700.0));

        ruler(ui, app);
        ui.add_space(4.0);

        let video_tracks: Vec<TrackId> = app
            .project
            .video_tracks
            .iter()
            .rev()
            .map(|t| t.id)
            .collect();
        let audio_tracks: Vec<TrackId> = app.project.audio_tracks.iter().map(|t| t.id).collect();

        // Alvos de snap (seção 53): início/fim de todo clip da timeline,
        // o playhead e o zero — coletados uma vez por frame, usados por
        // todo drag de qualquer track.
        let mut snap_targets: Vec<Time> = vec![Time::ZERO, app.playhead];
        for track in app
            .project
            .video_tracks
            .iter()
            .chain(app.project.audio_tracks.iter())
        {
            for clip in &track.clips {
                snap_targets.push(clip.timeline_start);
                snap_targets.push(clip.timeline_end());
            }
        }

        for track_id in video_tracks.into_iter().chain(audio_tracks) {
            track_row(ui, app, track_id, &snap_targets);
            ui.add_space(TRACK_GAP);
        }

        add_track_drop_zone(ui, app);
    });
}

fn add_track_drop_zone(ui: &mut egui::Ui, app: &mut Application) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let hovered = response.contains_pointer();
    let stroke_color = if hovered {
        theme::ACCENT
    } else {
        Color32::from_gray(0x40)
    };
    painter.rect_stroke(
        rect.shrink(2.0),
        6.0,
        Stroke::new(1.0, stroke_color),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        tr("timeline.drop_to_create_track"),
        egui::FontId::proportional(11.0),
        theme::MUTED,
    );

    let Some(payload) = response.dnd_release_payload::<crate::ui::media_panel::MediaDragPayload>()
    else {
        return;
    };
    let Some(duration) = app
        .project
        .media
        .get(&payload.media_id)
        .map(|m| m.metadata.duration)
    else {
        return;
    };
    app.push_undo();
    let track_id = if payload.is_video {
        let name = app.project.next_video_track_name();
        app.project.add_video_track(name)
    } else {
        let name = app.project.next_audio_track_name();
        app.project.add_audio_track(name)
    };
    if let Some(track) = app.project.find_track_mut(track_id) {
        track.clips.push(crate::project::Clip::new(
            payload.media_id,
            Time::ZERO,
            duration,
            Time::ZERO,
            duration,
        ));
    }
}

fn ruler(ui: &mut egui::Ui, app: &mut Application) {
    let height = 18.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        Sense::click_and_drag(),
    );

    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        app.playhead = x_to_time(pos.x - rect.left() - HEADER_WIDTH, app.timeline_zoom);
    }

    let step = nice_ruler_step(app.timeline_zoom);

    let painter = ui.painter_at(rect);
    let mut x = rect.left() + HEADER_WIDTH;
    let mut secs = 0.0_f64;
    while x < rect.right() {
        // Tick embaixo do número — régua de NLE de verdade marca a
        // graduação com uma linha, não só o texto solto.
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom() - 4.0),
                egui::pos2(x, rect.bottom()),
            ],
            Stroke::new(1.0, theme::MUTED.gamma_multiply(0.5)),
        );
        painter.text(
            egui::pos2(x + 3.0, rect.top()),
            egui::Align2::LEFT_TOP,
            Time::from_secs_f64(secs).to_string(),
            egui::FontId::monospace(10.0),
            theme::MUTED,
        );
        secs += step;
        x += step as f32 * app.timeline_zoom;
    }
}

/// Menor intervalo "redondo" (em segundos) da lista que ainda deixa pelo
/// menos `MIN_TICK_SPACING` pixels entre marcações no `zoom` atual — o
/// rótulo "00:00:00.000" precisa de uns 80px pra não sobrepor o vizinho.
/// Nos extremos (zoom no mínimo ou no máximo do clamp em `timeline_zoom`)
/// sempre existe um passo da lista que serve, já que ela cobre de 0.5s até
/// 1h.
fn nice_ruler_step(zoom: f32) -> f64 {
    const MIN_TICK_SPACING: f32 = 90.0;
    const STEPS: &[f64] = &[
        0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0,
    ];
    STEPS
        .iter()
        .copied()
        .find(|&s| s as f32 * zoom >= MIN_TICK_SPACING)
        .unwrap_or(*STEPS.last().unwrap())
}

fn track_row(ui: &mut egui::Ui, app: &mut Application, track_id: TrackId, snap_targets: &[Time]) {
    let Some(track) = app.project.find_track(track_id) else {
        return;
    };
    let is_video = matches!(track.track_type, crate::project::TrackType::Video);
    let badge_color = if is_video { VIDEO_COLOR } else { AUDIO_COLOR };
    let name = track.name.clone();
    let muted = track.muted;
    let volume = track.volume;
    let pan = track.pan;
    let row_height = if is_video {
        TRACK_HEIGHT
    } else {
        AUDIO_TRACK_HEIGHT
    };
    let clips: Vec<ClipRow> = track
        .clips
        .iter()
        .map(|c| ClipRow {
            id: c.id,
            start: c.timeline_start,
            duration: c.timeline_duration,
            reversed: c.reversed,
            source: c.source,
            source_start: c.source_start,
            source_duration: c.source_duration,
            fade_in: c.fade_in,
            fade_out: c.fade_out,
            level: if is_video { c.opacity } else { c.volume / 2.0 },
        })
        .collect();
    let mut toggle_mute = false;
    let mut delete_track = false;
    let mut start_rename = false;
    let mut new_name: Option<String> = None;
    let mut new_volume: Option<f32> = None;
    let mut new_pan: Option<f32> = None;
    let mut dropped_media: Option<crate::ui::media_panel::MediaDragPayload> = None;
    let mut drop_x = 0.0_f32;

    let mut select_track = false;

    let rename_id = ui.id().with(("track_rename", track_id));
    // `bool` marca se ainda precisa pegar foco (só na primeira vez que a
    // caixa aparece, não recomeça a cada frame).
    let renaming: Option<(String, bool)> = ui.ctx().data(|d| d.get_temp(rename_id));

    let is_selected = app.selection.track == Some(track_id);

    ui.horizontal(|ui| {
        let header_id = ui.id().with(("track_header", track_id));
        let (header_rect, header_response) = ui.allocate_exact_size(
            egui::vec2(HEADER_WIDTH - ui.spacing().item_spacing.x, row_height),
            Sense::click(),
        );
        header_response.context_menu(|ui| {
            if ui.button(tr("timeline.rename_track")).clicked() {
                start_rename = true;
                ui.close();
            }
            ui.separator();
            let toggle_label = if is_video {
                tr("timeline.toggle_visibility")
            } else {
                tr("timeline.toggle_mute")
            };
            if ui.button(toggle_label).clicked() {
                toggle_mute = true;
                ui.close();
            }
            ui.separator();
            if ui.button(tr("timeline.remove_track")).clicked() {
                delete_track = true;
                ui.close();
            }
        });
        let hover_t = theme::animate(ui, header_id, header_response.hovered() || is_selected);
        let header_painter = ui.painter_at(header_rect);

        let bg = if is_selected {
            badge_color.gamma_multiply(0.22)
        } else {
            theme::CANVAS
        };
        let bg = bg.lerp_to_gamma(badge_color.gamma_multiply(0.3), hover_t * 0.4);
        header_painter.rect_filled(header_rect, 5.0, bg);
        header_painter.rect_filled(
            egui::Rect::from_min_size(
                header_rect.min,
                egui::vec2(ACCENT_STRIP, header_rect.height()),
            ),
            CornerRadius {
                nw: 5,
                sw: 5,
                ne: 0,
                se: 0,
            },
            badge_color,
        );
        if is_selected {
            header_painter.rect_stroke(
                header_rect,
                5.0,
                Stroke::new(1.5, theme::ON_ACCENT.gamma_multiply(0.6 + 0.4 * hover_t)),
                StrokeKind::Inside,
            );
        }

        let content_rect = header_rect.shrink2(egui::vec2(ACCENT_STRIP + 6.0, 4.0));
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        // Interseccionar com o clip ambiente (herdado do `ScrollArea`) —
        // `set_clip_rect` sozinho *substitui* o clip em vez de
        // interseccionar (diferente de `painter_at`), então sem isso o
        // cabeçalho (ícones/nome) continuava sendo pintado mesmo depois de
        // rolar a track pra fora do viewport, sobrepondo o que tem acima
        // do scroll (usuário relatou: "ícones e nome sobrepõem o título
        // timeline" ao rolar pra baixo).
        content_ui.set_clip_rect(content_rect.intersect(ui.clip_rect()));
        // Espaçamento vertical padrão do tema (5px, `theme::apply`) é
        // generoso demais pra um cabeçalho de 100x60px com 4 linhas —
        // reduzido só aqui.
        content_ui.spacing_mut().item_spacing.y = 2.0;
        let name_color = if muted { theme::MUTED } else { badge_color };
        if let Some((mut buffer, needs_focus)) = renaming.clone() {
            let edit = content_ui.add(
                egui::TextEdit::singleline(&mut buffer)
                    .font(egui::FontId::proportional(11.5))
                    .desired_width(content_rect.width()),
            );
            if needs_focus {
                edit.request_focus();
            }
            if edit.lost_focus() || content_ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                new_name = Some(buffer);
                ui.ctx().data_mut(|d| d.remove::<(String, bool)>(rename_id));
            } else {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(rename_id, (buffer, false)));
            }
        } else {
            let name_response = content_ui
                .add(
                    egui::Label::new(RichText::new(&name).strong().color(name_color).size(11.5))
                        .truncate()
                        .selectable(false),
                )
                .interact(Sense::click());
            if name_response.double_clicked() {
                start_rename = true;
            } else if name_response.clicked() {
                select_track = true;
            }
        }
        content_ui.horizontal(|ui| {
            let toggle_icon = if is_video {
                if muted { "🚫" } else { "👁" }
            } else if muted {
                "🔇"
            } else {
                "🔊"
            };
            if theme::icon_button(ui, toggle_icon, 12.0)
                .on_hover_text(if is_video {
                    tr("timeline.toggle_visibility")
                } else {
                    tr("timeline.toggle_mute")
                })
                .clicked()
            {
                toggle_mute = true;
            }
            if theme::icon_button(ui, "🗑", 12.0)
                .on_hover_text(tr("timeline.remove_track"))
                .clicked()
            {
                delete_track = true;
            }
        });

        if !is_video {
            content_ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let half = (content_rect.width() - ui.spacing().item_spacing.x) / 2.0;
                let mut v = volume;
                if mini_fader(ui, "🔊", &mut v, 0.0..=2.0, half) {
                    new_volume = Some(v);
                }
                let mut p = pan;
                if mini_fader(ui, "⚖", &mut p, -1.0..=1.0, half) {
                    new_pan = Some(p);
                }
            });
        }

        let (rect, bg_response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::CANVAS);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Color32::from_gray(0x30)),
            StrokeKind::Inside,
        );

        // Drop de mídia arrastada do painel de mídia (seção 48) — só aceita
        // se o tipo bate com o tipo da track (vídeo em track de vídeo,
        // áudio em track de áudio), senão ignora silenciosamente.
        if let Some(payload) =
            bg_response.dnd_release_payload::<crate::ui::media_panel::MediaDragPayload>()
            && payload.is_video == is_video
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            dropped_media = Some(*payload);
            drop_x = pos.x - rect.left();
        }

        for clip in clips {
            let ClipRow {
                id: clip_id,
                start,
                duration,
                reversed,
                source,
                source_start,
                source_duration,
                fade_in,
                fade_out,
                level,
            } = clip;
            let x0 = rect.left() + time_to_x(start, app.timeline_zoom);
            let x1 = rect.left() + time_to_x(start + duration, app.timeline_zoom);
            let clip_rect = egui::Rect::from_min_max(
                egui::pos2(x0, rect.top() + 2.0),
                egui::pos2(x1.max(x0 + 2.0), rect.bottom() - 2.0),
            );
            let widget_id = ui.id().with(("clip", clip_id));
            let response = ui.interact(clip_rect, widget_id, Sense::click_and_drag());

            // Posições das alcinhas de fade e da linha de opacidade/volume
            // (facilitadores estilo Vegas) — calculadas uma vez, usadas pro
            // hit-test do cursor, pro `drag_started` e pro desenho.
            let fade_in_x = x0 + fade_in.as_secs_f64() as f32 * app.timeline_zoom;
            let fade_out_x = x1 - fade_out.as_secs_f64() as f32 * app.timeline_zoom;
            let level_y = level_y_for(clip_rect, level);

            if response.hovered() || response.dragged() {
                let hover_mode = if response.dragged() {
                    ui.ctx()
                        .data(|d| d.get_temp::<DragMode>(widget_id))
                        .unwrap_or(DragMode::Move)
                } else {
                    match response.hover_pos() {
                        Some(pos) => {
                            hit_test_drag_mode(pos, clip_rect, fade_in_x, fade_out_x, level_y)
                        }
                        None => DragMode::Move,
                    }
                };
                let icon = match hover_mode {
                    DragMode::TrimLeft | DragMode::TrimRight => egui::CursorIcon::ResizeHorizontal,
                    DragMode::FadeIn => egui::CursorIcon::ResizeNeSw,
                    DragMode::FadeOut => egui::CursorIcon::ResizeNwSe,
                    DragMode::Level => egui::CursorIcon::ResizeVertical,
                    DragMode::Move if response.dragged() => egui::CursorIcon::Grabbing,
                    DragMode::Move => egui::CursorIcon::Grab,
                };
                ui.ctx().set_cursor_icon(icon);
            }

            if response.drag_started() {
                let press_pos = ui
                    .input(|i| i.pointer.press_origin())
                    .or_else(|| response.interact_pointer_pos());
                if let Some(pos) = press_pos {
                    let mode = hit_test_drag_mode(pos, clip_rect, fade_in_x, fade_out_x, level_y);
                    ui.ctx().data_mut(|d| d.insert_temp(widget_id, mode));
                    app.push_undo();
                }
            }
            if response.dragged() {
                let mode = ui
                    .ctx()
                    .data(|d| d.get_temp::<DragMode>(widget_id))
                    .unwrap_or(DragMode::Move);
                let delta =
                    Time::from_secs_f64((response.drag_delta().x / app.timeline_zoom) as f64);

                match mode {
                    DragMode::TrimLeft => {
                        let snapped = snap_delta(
                            start,
                            delta,
                            (start, start + duration),
                            snap_targets,
                            app.timeline_zoom,
                        );
                        app.trim_clip_left(clip_id, snapped);
                    }
                    DragMode::TrimRight => {
                        let snapped = snap_delta(
                            start + duration,
                            delta,
                            (start, start + duration),
                            snap_targets,
                            app.timeline_zoom,
                        );
                        app.trim_clip_right(clip_id, snapped);
                    }
                    DragMode::Move => {
                        let snapped = snap_delta(
                            start,
                            delta,
                            (start, start + duration),
                            snap_targets,
                            app.timeline_zoom,
                        );
                        app.move_clip(clip_id, snapped);
                    }
                    DragMode::FadeIn => {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let secs = ((pos.x - x0) / app.timeline_zoom).max(0.0) as f64;
                            app.set_clip_fade_in(clip_id, Time::from_secs_f64(secs));
                        }
                    }
                    DragMode::FadeOut => {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let secs = ((x1 - pos.x) / app.timeline_zoom).max(0.0) as f64;
                            app.set_clip_fade_out(clip_id, Time::from_secs_f64(secs));
                        }
                    }
                    DragMode::Level => {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let t = level_t_for(clip_rect, pos.y);
                            if is_video {
                                app.set_clip_opacity(clip_id, t);
                            } else {
                                app.set_clip_volume(clip_id, t * 2.0);
                            }
                        }
                    }
                }
            }

            let selected = app.selection.is_clip_selected(clip_id);
            // Hover e seleção suavizados (ease, não troca de cor instantânea)
            // — o toque "fluido" de um NLE profissional.
            let hover_t = theme::animate(ui, widget_id, response.hovered() && !response.dragged());
            let select_t = theme::animate(ui, widget_id.with("sel"), selected);
            let base = badge_color.gamma_multiply(0.55);
            let fill = base
                .lerp_to_gamma(badge_color, select_t)
                .lerp_to_gamma(Color32::WHITE, hover_t * 0.12);
            painter.rect_filled(clip_rect, 4.0, fill);
            if select_t > 0.01 {
                painter.rect_stroke(
                    clip_rect,
                    4.0,
                    Stroke::new(2.0, theme::ON_ACCENT.gamma_multiply(select_t)),
                    StrokeKind::Inside,
                );
            }
            if !is_video && !response.dragged() && clip_rect.width() > 6.0 {
                let peaks = app.waveform_for(source, source_start, source_duration);
                draw_waveform(&painter, clip_rect, &peaks, reversed);
            }
            // Facilitadores de fade/opacidade/volume estilo Vegas — some em
            // clipes pequenos demais pra não virar poluição visual.
            if clip_rect.width() > 24.0 {
                draw_clip_facilitators(&painter, clip_rect, fade_in_x, fade_out_x, level_y);
            }
            if reversed {
                painter.text(
                    clip_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⏪",
                    egui::FontId::proportional(12.0),
                    theme::ON_ACCENT,
                );
            }

            // Ícone de Pan/Crop (seção 19) — só em vídeo com mídia real
            // (mídia gerada não tem "região do source" pra recortar, mesma
            // condição do menu de contexto abaixo). Clique é detectado
            // junto com o `response.clicked()` normal do clipe (não um
            // `Sense` separado) — mais simples e sem risco de duas áreas
            // clicáveis sobrepostas brigando pelo mesmo evento.
            let show_pan_crop_icon =
                is_video && clip_rect.width() > 24.0 && app.project.get_media(source).is_some();
            let pan_crop_icon_rect = egui::Rect::from_min_size(
                egui::pos2(
                    clip_rect.right() - PAN_CROP_ICON_SIZE - 4.0,
                    clip_rect.top() + 3.0,
                ),
                egui::vec2(PAN_CROP_ICON_SIZE, PAN_CROP_ICON_SIZE),
            );
            if show_pan_crop_icon {
                let hovered = response
                    .hover_pos()
                    .is_some_and(|p| pan_crop_icon_rect.contains(p));
                painter.rect_filled(
                    pan_crop_icon_rect,
                    3.0,
                    if hovered {
                        theme::ACCENT
                    } else {
                        Color32::from_black_alpha(150)
                    },
                );
                painter.text(
                    pan_crop_icon_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⛶",
                    egui::FontId::proportional(10.0),
                    theme::ON_ACCENT,
                );
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }

            let mut open_pan_crop = false;
            if response.clicked() {
                if show_pan_crop_icon
                    && response
                        .interact_pointer_pos()
                        .is_some_and(|p| pan_crop_icon_rect.contains(p))
                {
                    open_pan_crop = true;
                    app.selection.select_only(clip_id);
                    app.selection.track = None;
                } else if ui.input(|i| i.modifiers.shift) {
                    if app.selection.is_clip_selected(clip_id) {
                        app.selection.remove_clip(clip_id);
                    } else {
                        app.selection.add_clip(clip_id);
                    }
                    app.selection.track = None;
                } else {
                    app.selection.select_only(clip_id);
                    app.selection.track = None;
                }
            }
            if open_pan_crop {
                let ctx = ui.ctx().clone();
                crate::ui::pan_crop::open(app, clip_id, &ctx);
            }

            if response.secondary_clicked() && !app.selection.is_clip_selected(clip_id) {
                app.selection.select_only(clip_id);
                app.selection.track = None;
            }
            response.context_menu(|ui| {
                if ui.button(tr("toolbar.split")).clicked() {
                    app.split_selected_at_playhead();
                    ui.close();
                }
                if ui.button(tr("toolbar.reverse_clip")).clicked() {
                    app.toggle_reverse_selected();
                    ui.close();
                }
                if ui.button(tr("common.duplicate_clip")).clicked() {
                    app.duplicate_selected_clips();
                    ui.close();
                }
                // Pan/Crop (seção 19, estilo Vegas) só faz sentido pra
                // vídeo com mídia real — mídia gerada (Cor Sólida/
                // Visualizador) não tem uma "região do source" pra
                // recortar.
                if is_video && app.project.get_media(source).is_some() {
                    ui.separator();
                    if ui.button(tr("timeline.pan_crop")).clicked() {
                        let ctx = ui.ctx().clone();
                        crate::ui::pan_crop::open(app, clip_id, &ctx);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(tr("toolbar.delete_clip")).clicked() {
                    app.delete_selected_clips();
                    ui.close();
                }
            });
        }

        let playhead_x = rect.left() + time_to_x(app.playhead, app.timeline_zoom);
        if playhead_x >= rect.left() && playhead_x <= rect.right() {
            painter.line_segment(
                [
                    egui::pos2(playhead_x, rect.top()),
                    egui::pos2(playhead_x, rect.bottom()),
                ],
                Stroke::new(2.0, theme::ACCENT),
            );
        }
    });

    if toggle_mute && let Some(track) = app.project.find_track_mut(track_id) {
        track.muted = !track.muted;
    }
    if let Some(v) = new_volume
        && let Some(track) = app.project.find_track_mut(track_id)
    {
        track.volume = v;
    }
    if let Some(p) = new_pan
        && let Some(track) = app.project.find_track_mut(track_id)
    {
        track.pan = p;
    }
    if start_rename {
        ui.ctx()
            .data_mut(|d| d.insert_temp(rename_id, (name.clone(), true)));
    }
    if let Some(new_name) = new_name
        && !new_name.trim().is_empty()
    {
        app.push_undo();
        if let Some(track) = app.project.find_track_mut(track_id) {
            track.name = new_name.trim().to_string();
        }
    }
    if select_track {
        app.selection.clear();
        app.selection.track = Some(track_id);
    }
    if delete_track {
        app.push_undo();
        app.project.remove_track(track_id);
        if app.selection.track == Some(track_id) {
            app.selection.clear();
        }
    }
    if let Some(payload) = dropped_media
        && let Some(duration) = app
            .project
            .media
            .get(&payload.media_id)
            .map(|m| m.metadata.duration)
    {
        let start = x_to_time(drop_x, app.timeline_zoom);
        app.push_undo();
        if let Some(track) = app.project.find_track_mut(track_id) {
            track.clips.push(crate::project::Clip::new(
                payload.media_id,
                start,
                duration,
                Time::ZERO,
                duration,
            ));
        }
    }
}

fn mini_fader(
    ui: &mut egui::Ui,
    icon: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    total_width: f32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.label(RichText::new(icon).size(8.0).color(theme::MUTED));
        let bar_width = (total_width - 12.0).max(10.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(bar_width, 8.0), Sense::click_and_drag());
        let (min, max) = (*range.start(), *range.end());
        let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, Color32::from_gray(0x28));
        let mut fill = rect;
        fill.set_right(rect.left() + rect.width() * t);
        painter.rect_filled(fill, 2.0, theme::ACCENT.gamma_multiply(0.75));
        painter.circle_filled(
            egui::pos2(
                fill.right().clamp(rect.left() + 2.0, rect.right() - 2.0),
                rect.center().y,
            ),
            2.5,
            theme::ACCENT,
        );
        if (response.dragged() || response.clicked())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let nt = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            *value = min + nt * (max - min);
            changed = true;
        }
    });
    changed
}

/// Desenha os facilitadores de fade/opacidade-volume estilo Sony Vegas por
/// cima do clip: uma cunha escura + linha diagonal com alcinha em cada
/// ponta faded (mostra visualmente o ganho subindo/descendo), e uma linha
/// horizontal amarela representando opacidade (vídeo) ou volume (áudio) —
/// arrastável direto (ver `hit_test_drag_mode`/`DragMode::Level` acima),
/// sem precisar abrir o Inspector.
fn draw_clip_facilitators(
    painter: &egui::Painter,
    rect: egui::Rect,
    fade_in_x: f32,
    fade_out_x: f32,
    level_y: f32,
) {
    let shade = Color32::from_black_alpha(100);
    let line_color = theme::ON_ACCENT.gamma_multiply(0.9);

    if fade_in_x > rect.left() + 1.0 {
        let fin_x = fade_in_x.min(rect.right());
        painter.add(egui::Shape::convex_polygon(
            vec![
                rect.left_bottom(),
                rect.left_top(),
                egui::pos2(fin_x, rect.top()),
            ],
            shade,
            Stroke::NONE,
        ));
        painter.line_segment(
            [rect.left_bottom(), egui::pos2(fin_x, rect.top())],
            Stroke::new(1.5, line_color),
        );
        painter.circle_filled(egui::pos2(fin_x, rect.top()), 3.0, line_color);
    }

    if fade_out_x < rect.right() - 1.0 {
        let fout_x = fade_out_x.max(rect.left());
        painter.add(egui::Shape::convex_polygon(
            vec![
                rect.right_bottom(),
                rect.right_top(),
                egui::pos2(fout_x, rect.top()),
            ],
            shade,
            Stroke::NONE,
        ));
        painter.line_segment(
            [egui::pos2(fout_x, rect.top()), rect.right_bottom()],
            Stroke::new(1.5, line_color),
        );
        painter.circle_filled(egui::pos2(fout_x, rect.top()), 3.0, line_color);
    }

    let level_color = Color32::from_rgba_unmultiplied(255, 210, 60, 200);
    painter.line_segment(
        [
            egui::pos2(rect.left(), level_y),
            egui::pos2(rect.right(), level_y),
        ],
        Stroke::new(1.0, level_color),
    );
    painter.circle_filled(egui::pos2(rect.center().x, level_y), 2.5, level_color);
}

fn draw_waveform(painter: &egui::Painter, rect: egui::Rect, peaks: &[f32], reversed: bool) {
    if peaks.is_empty() || rect.width() < 2.0 {
        return;
    }
    let mid = rect.center().y;
    let half_h = rect.height() / 2.0 - 2.0;
    let color = theme::ON_ACCENT.gamma_multiply(0.55);

    let n = peaks.len();
    for (i, &raw) in peaks.iter().enumerate() {
        let peak = if reversed { peaks[n - 1 - i] } else { raw };
        let x = rect.left() + rect.width() * (i as f32 / n as f32);
        let h = (peak.clamp(0.0, 1.0) * half_h).max(0.5);
        painter.line_segment(
            [egui::pos2(x, mid - h), egui::pos2(x, mid + h)],
            Stroke::new(1.0, color),
        );
    }
}

/// Ajusta `delta` pra grudar `edge_before + delta` no alvo de snap mais
/// próximo dentro de `SNAP_PIXELS` (seção 53), ignorando as bordas do
/// próprio clip sendo arrastado (`own_edges`) — sem isso ele sempre "gruda"
/// na posição onde já está.
fn snap_delta(
    edge_before: Time,
    delta: Time,
    own_edges: (Time, Time),
    targets: &[Time],
    zoom: f32,
) -> Time {
    let predicted = edge_before + delta;
    let threshold = Time::from_secs_f64((SNAP_PIXELS / zoom) as f64);

    let nearest = targets
        .iter()
        .filter(|&&t| t != own_edges.0 && t != own_edges.1)
        .min_by_key(|&&t| (t.ticks() - predicted.ticks()).abs());

    match nearest {
        Some(&target) if (target.ticks() - predicted.ticks()).abs() <= threshold.ticks() => {
            target - edge_before
        }
        _ => delta,
    }
}

/// Does **not** include `HEADER_WIDTH` — only `ruler()` adds that, at its
/// own call site. Adding it here too double-counts the header column.
fn time_to_x(time: Time, zoom: f32) -> f32 {
    time.as_secs_f64() as f32 * zoom
}

fn x_to_time(x: f32, zoom: f32) -> Time {
    Time::from_secs_f64((x.max(0.0) / zoom) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(300.0, 40.0))
    }

    #[test]
    fn near_fade_in_handle_wins_over_the_trim_edge_underneath_it() {
        let r = rect();
        // A alcinha de fade_in fica no canto, exatamente onde o alvo de
        // trim também mora quando fade_in é pequeno — a alcinha precisa
        // ganhar prioridade, senão nunca dá pra pegá-la.
        let mode = hit_test_drag_mode(egui::pos2(102.0, 1.0), r, 102.0, 298.0, 20.0);
        assert_eq!(mode, DragMode::FadeIn);
    }

    #[test]
    fn near_fade_out_handle_is_detected() {
        let r = rect();
        let mode = hit_test_drag_mode(egui::pos2(280.0, 1.0), r, 100.0, 280.0, 20.0);
        assert_eq!(mode, DragMode::FadeOut);
    }

    #[test]
    fn near_left_edge_but_far_from_top_is_trim_left() {
        let r = rect();
        let mode = hit_test_drag_mode(egui::pos2(101.0, 20.0), r, 100.0, 300.0, 5.0);
        assert_eq!(mode, DragMode::TrimLeft);
    }

    #[test]
    fn near_level_line_is_detected() {
        let r = rect();
        let mode = hit_test_drag_mode(egui::pos2(200.0, 30.0), r, 100.0, 300.0, 30.0);
        assert_eq!(mode, DragMode::Level);
    }

    #[test]
    fn middle_of_the_clip_is_move() {
        let r = rect();
        let mode = hit_test_drag_mode(egui::pos2(200.0, 20.0), r, 100.0, 300.0, 5.0);
        assert_eq!(mode, DragMode::Move);
    }

    #[test]
    fn full_level_stays_clear_of_the_fade_handle_zone() {
        let r = rect();
        let y = level_y_for(r, 1.0);
        assert!(
            y - r.top() > HANDLE_GRAB,
            "level_y {y} muito perto do topo (top={}, HANDLE_GRAB={HANDLE_GRAB})",
            r.top()
        );
    }

    #[test]
    fn level_y_for_and_level_t_for_round_trip() {
        let r = rect();
        for level in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let y = level_y_for(r, level);
            let back = level_t_for(r, y);
            assert!(
                (back - level).abs() < 1e-4,
                "level {level} -> y {y} -> {back}, deveria voltar pro mesmo valor"
            );
        }
    }
}
