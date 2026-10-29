use crate::app::Application;
use crate::i18n::tr;
use crate::media::decoder;
use crate::project::{
    AudioVisualizerSettings, GeneratedMedia, GeneratedMediaEntry, MediaId, Project, Time,
    TrackType, Transform, VisualizerAlignment,
};
use crate::ui::theme;
use egui::{Button, RichText};

#[derive(Clone, Copy)]
struct Layer {
    media_id: MediaId,
    source_time: Time,
    /// `opacity * fade_in/out` no instante atual (seção 28) — alpha final
    /// que o compositor aplica a essa camada.
    alpha: f32,
    /// Pan/Crop do clipe (seção 19) — aplicado antes de escalar pra `w x h`.
    transform: Transform,
}

/// Chave de cache do preview — `(MediaId, source_time, alpha bits)` de
/// cada camada ativa (seção 9/43). Só recompõe quando isso muda.
pub type PreviewKey = Vec<(MediaId, Time, u32)>;

/// Composição rodando numa thread separada (ver `dispatch_composite`) — o
/// resultado chega pelo `rx` quando fica pronto, sem travar a UI thread
/// enquanto isso.
pub struct PendingComposite {
    key: PreviewKey,
    rx: std::sync::mpsc::Receiver<Option<egui::ColorImage>>,
}

/// Todas as camadas de vídeo ativas no playhead, de baixo pra cima (seção
/// 15/43 — composição bottom-to-top: `video_tracks[0]` é o fundo).
fn active_layers(app: &Application) -> Vec<Layer> {
    app.project
        .video_tracks
        .iter()
        .filter(|t| !t.muted && matches!(t.track_type, TrackType::Video))
        .filter_map(|track| {
            let clip = track.clips.iter().find(|c| c.contains(app.playhead))?;
            let source_time = clip.source_time_at(app.playhead)?;
            let local = (app.playhead - clip.timeline_start).as_secs_f64();
            let duration = clip.timeline_duration.as_secs_f64();
            let fade_in = clip.fade_in.as_secs_f64();
            let fade_out = clip.fade_out.as_secs_f64();

            let mut fade_factor: f64 = 1.0;
            if fade_in > 0.0 && local < fade_in {
                fade_factor = fade_factor.min(local / fade_in);
            }
            if fade_out > 0.0 && local > duration - fade_out {
                fade_factor = fade_factor.min(((duration - local) / fade_out).max(0.0));
            }

            Some(Layer {
                media_id: clip.source,
                source_time,
                alpha: (clip.opacity as f64 * fade_factor).clamp(0.0, 1.0) as f32,
                transform: clip.transform,
            })
        })
        .collect()
}

/// RGBA (`w*h*4` bytes) de uma camada, já escalada pra `w x h` (pra todas
/// as camadas darem blend do mesmo tamanho). Mídia real decodificada
/// (`ffmpeg`) é sempre opaca (`a=255`); mídia gerada (seção 35) pode ter
/// transparência por pixel — é o que permite um visualizador de fundo
/// transparente (seção 40) sobrepor a camada de baixo em vez de tapá-la.
fn layer_rgba(
    project: &Project,
    playhead: Time,
    media_id: MediaId,
    source_time: Time,
    w: u32,
    h: u32,
    transform: Transform,
) -> Option<Vec<u8>> {
    if let Some(entry) = project.generated.get(&media_id) {
        return Some(generated_rgba(project, playhead, entry, w, h));
    }
    let media = project.get_media(media_id)?;
    let frame =
        decoder::decode_frame_at_scaled(&media.path, source_time, (w, h), transform).ok()?;
    let mut rgba = Vec::with_capacity(frame.rgb.len() / 3 * 4);
    for px in frame.rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Some(rgba)
}

fn generated_rgba(
    project: &Project,
    playhead: Time,
    entry: &GeneratedMediaEntry,
    w: u32,
    h: u32,
) -> Vec<u8> {
    match &entry.kind {
        GeneratedMedia::SolidColor(c) => {
            let mut buf = Vec::with_capacity((w * h * 4) as usize);
            for _ in 0..(w * h) {
                buf.extend_from_slice(&[c.r, c.g, c.b, 255]);
            }
            buf
        }
        GeneratedMedia::AudioVisualizer(settings) => {
            visualizer_rgba(project, playhead, settings, w, h)
        }
    }
}

/// Visualizador de áudio de verdade (seção 37-40): mixa uma janela curta do
/// áudio da timeline perto do playhead e desenha barras de espectro (FFT,
/// `audio::analyzer`) — altura, cor, fundo e alinhamento (base/centro)
/// customizáveis via `settings`.
fn visualizer_rgba(
    project: &Project,
    playhead: Time,
    settings: &AudioVisualizerSettings,
    w: u32,
    h: u32,
) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const WINDOW_SECS: f64 = 0.15;

    let samples =
        crate::audio::mixer::mix_window(project, playhead, WINDOW_SECS, SAMPLE_RATE, CHANNELS);

    let (wu, hu) = (w as usize, h as usize);
    let bg = match settings.background {
        Some(c) => [c.r, c.g, c.b, 255],
        None => [0, 0, 0, 0],
    };
    let mut rgba = vec![0u8; wu * hu * 4];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }

    let bands = crate::audio::analyzer::frequency_bands(
        &samples,
        CHANNELS as usize,
        SAMPLE_RATE,
        (settings.bar_count.max(1) as usize).min(256),
        settings.sensitivity.max(0.01),
    );
    draw_bars(
        &mut rgba,
        (wu, hu),
        &bands,
        settings.color,
        (
            settings.height.clamp(0.05, 1.0),
            settings.padding.clamp(0.0, 0.4),
        ),
        settings.alignment,
    );

    rgba
}

fn draw_bars(
    rgba: &mut [u8],
    (w, h): (usize, usize),
    bands: &[f32],
    color: crate::project::SolidColor,
    (height_frac, padding_frac): (f32, f32),
    alignment: VisualizerAlignment,
) {
    let bar_count = bands.len().max(1);
    let gap = 2usize;
    let pad_px = (w as f32 * padding_frac) as usize;
    let pad_py = (h as f32 * padding_frac) as usize;
    let usable_w = w.saturating_sub(pad_px * 2).max(1);
    let usable_h = h.saturating_sub(pad_py * 2).max(1);
    let total_gap = gap * bar_count.saturating_sub(1);
    let bar_width = (usable_w.saturating_sub(total_gap) / bar_count).max(1);
    let max_bar_h = (usable_h as f32 * height_frac) as usize;

    for (i, &level) in bands.iter().enumerate() {
        let bar_h = ((level.clamp(0.0, 1.0) * max_bar_h as f32) as usize).min(usable_h);
        let x0 = (pad_px + i * (bar_width + gap)).min(w);
        let x1 = (x0 + bar_width).min(w);
        let (y0, y1) = match alignment {
            VisualizerAlignment::Base => {
                let baseline = h.saturating_sub(pad_py);
                (baseline.saturating_sub(bar_h), baseline)
            }
            VisualizerAlignment::Center => {
                let half = bar_h / 2;
                let mid = h / 2;
                (mid.saturating_sub(half), (mid + half).min(h))
            }
        };
        for y in y0..y1 {
            let row = &mut rgba[y * w * 4..(y + 1) * w * 4];
            for x in x0..x1 {
                row[x * 4..x * 4 + 4].copy_from_slice(&[color.r, color.g, color.b, 255]);
            }
        }
    }
}

/// Composição multi-track de verdade (seção 9/43-44): decodifica cada
/// camada ativa já escalada pra `w x h` e faz alpha blending bottom-to-top
/// sobre um fundo preto. O alpha de cada pixel de uma camada gerada
/// (transparência de fundo, seção 40) multiplica com o alpha do clip
/// (opacity/fade) — as duas coisas compõem juntas.
fn composite(
    project: &Project,
    playhead: Time,
    layers: &[Layer],
    w: u32,
    h: u32,
) -> Option<egui::ColorImage> {
    if layers.is_empty() {
        return None;
    }

    let mut buffer = vec![0u8; (w as usize) * (h as usize) * 3];

    for layer in layers {
        if layer.alpha <= 0.0 {
            continue;
        }
        let Some(rgba) = layer_rgba(
            project,
            playhead,
            layer.media_id,
            layer.source_time,
            w,
            h,
            layer.transform,
        ) else {
            continue;
        };
        if rgba.len() != buffer.len() / 3 * 4 {
            continue;
        }
        for (dst, src) in buffer.chunks_exact_mut(3).zip(rgba.chunks_exact(4)) {
            let a = (src[3] as f32 / 255.0) * layer.alpha;
            if a <= 0.0 {
                continue;
            }
            for c in 0..3 {
                dst[c] = (dst[c] as f32 * (1.0 - a) + src[c] as f32 * a).round() as u8;
            }
        }
    }

    Some(egui::ColorImage::from_rgb(
        [w as usize, h as usize],
        &buffer,
    ))
}

fn dispatch_composite(app: &mut Application, layers: Vec<Layer>, key: PreviewKey) {
    if app.pending_preview.is_some() {
        return;
    }
    let project = app.project.clone();
    let playhead = app.playhead;
    let (w, h) = (app.project.settings.width, app.project.settings.height);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let image = composite(&project, playhead, &layers, w, h);
        let _ = tx.send(image);
    });
    app.pending_preview = Some(PendingComposite { key, rx });
}

pub fn show(ui: &mut egui::Ui, app: &mut Application) {
    let full_rect = ui.max_rect();
    let controls_height = 44.0;
    let gap = 8.0;
    let controls_rect = egui::Rect::from_min_max(
        egui::pos2(full_rect.left(), full_rect.bottom() - controls_height),
        full_rect.max,
    );
    let canvas_rect = egui::Rect::from_min_max(
        full_rect.min,
        egui::pos2(
            full_rect.right(),
            (controls_rect.top() - gap).max(full_rect.top()),
        ),
    );

    let layers = active_layers(app);
    let has_content = !layers.is_empty();
    let key: Vec<_> = layers
        .iter()
        .map(|l| (l.media_id, l.source_time, l.alpha.to_bits()))
        .collect();

    if has_content && app.preview_key.as_ref() != Some(&key) {
        dispatch_composite(app, layers, key);
    } else if !has_content {
        app.preview_key = None;
        app.pending_preview = None;
    }

    // Recolhe o resultado de uma composição pendente (sem bloquear —
    // `try_recv` só pega o que já chegou). Enquanto não chega, o frame
    // anterior continua na tela; não trava esperando.
    if let Some(pending) = &app.pending_preview
        && let Ok(image_opt) = pending.rx.try_recv()
    {
        let result_key = app.pending_preview.take().unwrap().key;
        if let Some(image) = image_opt {
            match &mut app.preview_texture {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    app.preview_texture = Some(ui.ctx().load_texture(
                        "preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            app.preview_key = Some(result_key);
        }
    }
    // Enquanto uma composição roda em background, `ui.ctx().request_repaint()`
    // do laço de Play já garante que o frame seguinte vem checar de novo —
    // não precisa de repaint extra aqui.

    if has_content {
        let mut canvas_ui = ui.new_child(egui::UiBuilder::new().max_rect(canvas_rect));
        canvas_ui.centered_and_justified(|ui| {
            if let Some(texture) = &app.preview_texture {
                let size = texture.size_vec2();
                let scale = (ui.available_width() / size.x).min(ui.available_height() / size.y);
                ui.image((texture.id(), size * scale.clamp(0.0001, 1.0)));
            }
        });
    } else {
        let painter = ui.painter_at(canvas_rect);
        let center = canvas_rect.center();
        painter.text(
            center - egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            "▶",
            egui::FontId::proportional(28.0),
            theme::MUTED,
        );
        painter.text(
            center + egui::vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            tr("preview.empty"),
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );
    }

    ui.painter().hline(
        full_rect.left()..=full_rect.right(),
        controls_rect.top() - gap / 2.0,
        egui::Stroke::new(1.0, theme::MUTED.gamma_multiply(0.3)),
    );

    let mut controls_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    controls_ui.centered_and_justified(|ui| {
        ui.horizontal(|ui| {
            let fps = app.project.settings.fps;
            if ui
                .add(Button::new(RichText::new("⏮").size(13.0)).frame(false))
                .on_hover_text(tr("preview.previous_frame"))
                .clicked()
            {
                app.playing = false;
                let frame = (app.playhead.to_frame(fps) - 1).max(0);
                app.playhead = Time::from_frame(frame, fps);
            }
            let play_icon = if app.playing { "⏸" } else { "⏵" };
            if ui
                .add(Button::new(RichText::new(play_icon).size(15.0)).frame(false))
                .clicked()
            {
                app.playing = !app.playing;
            }
            if ui
                .add(Button::new(RichText::new("⏭").size(13.0)).frame(false))
                .on_hover_text(tr("preview.next_frame"))
                .clicked()
            {
                app.playing = false;
                let frame = app.playhead.to_frame(fps) + 1;
                app.playhead = Time::from_frame(frame, fps);
            }

            ui.add_space(16.0);
            ui.label(
                RichText::new(format!("{} / {}", app.playhead, total_duration(app)))
                    .monospace()
                    .color(theme::MUTED),
            );
        });
    });
}

fn total_duration(app: &Application) -> Time {
    app.project
        .video_tracks
        .iter()
        .chain(app.project.audio_tracks.iter())
        .flat_map(|t| t.clips.iter())
        .map(|c| c.timeline_end())
        .max()
        .unwrap_or(Time::ZERO)
}
