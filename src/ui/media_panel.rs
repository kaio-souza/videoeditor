use crate::app::Application;
use crate::i18n::tr;
use crate::media::metadata;
use crate::project::{
    AudioVisualizerSettings, Media, MediaId, SolidColor, Time, VisualizerAlignment,
};
use crate::ui::theme;
use egui::{Button, RichText};

/// Payload de drag-and-drop (seção 48): arrastar um item da lista de mídia
/// pra uma track na timeline. `ui/timeline.rs` lê isso via
/// `Response::dnd_release_payload` na área de cada track.
#[derive(Debug, Clone, Copy)]
pub struct MediaDragPayload {
    pub media_id: MediaId,
    pub is_video: bool,
}

pub fn show(ui: &mut egui::Ui, app: &mut Application) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr("media.header").to_uppercase())
                .small()
                .color(theme::MUTED)
                .strong(),
        );
    });
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            show_content(ui, app);
        });
}

/// Altura máxima da lista de mídia antes de virar rolagem própria — sem
/// isso uma lista longa empurra os cards de Generate embaixo pra fora de
/// vista (o scroll do painel inteiro ainda existe, mas obriga a rolar tudo
/// só pra ver os geradores).
const MEDIA_LIST_MAX_HEIGHT: f32 = 130.0;

fn show_content(ui: &mut egui::Ui, app: &mut Application) {
    let import_label = RichText::new(format!("+  {}", tr("common.import_media")))
        .color(theme::ON_ACCENT)
        .strong();
    let import = Button::new(import_label)
        .fill(theme::ACCENT)
        .corner_radius(6);
    if ui.add_sized([ui.available_width(), 30.0], import).clicked() {
        import_media(app);
    }
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt("media_list_scroll")
        .max_height(MEDIA_LIST_MAX_HEIGHT)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            show_media_list(ui, app);
        });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);
    ui.label(
        RichText::new(tr("media.generate_header").to_uppercase())
            .small()
            .color(theme::MUTED)
            .strong(),
    );

    theme::collapsing_section(
        ui,
        "generate_solid_color",
        &tr("media.solid_color"),
        false,
        |ui| {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut app.solid_color);
                ui.add(
                    egui::DragValue::new(&mut app.solid_color_duration)
                        .suffix("s")
                        .range(0.1..=3600.0)
                        .speed(0.1),
                );
            });
            ui.add_space(6.0);
            if ui
                .add_sized(
                    [ui.available_width(), 26.0],
                    Button::new(format!("🎨 {}", tr("media.solid_color"))).corner_radius(5),
                )
                .clicked()
            {
                let color = SolidColor {
                    r: app.solid_color[0],
                    g: app.solid_color[1],
                    b: app.solid_color[2],
                };
                let duration = Time::from_secs_f64(app.solid_color_duration);
                app.push_undo();
                let media_id = app.project.add_solid_color(color, duration);
                add_to_timeline(app, media_id, true, duration);
            }
        },
    );

    theme::collapsing_section(
        ui,
        "generate_audio_visualizer",
        &tr("media.audio_visualizer"),
        false,
        |ui| {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut app.visualizer_color);
                egui::ComboBox::from_id_salt("visualizer_alignment")
                    .selected_text(alignment_label(app.visualizer_alignment))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.visualizer_alignment,
                            VisualizerAlignment::Base,
                            tr("media.visualizer_align_base"),
                        );
                        ui.selectable_value(
                            &mut app.visualizer_alignment,
                            VisualizerAlignment::Center,
                            tr("media.visualizer_align_center"),
                        );
                    });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(tr("media.visualizer_height"))
                    .small()
                    .color(theme::MUTED),
            );
            ui.add(egui::Slider::new(&mut app.visualizer_height, 0.05..=1.0));
            ui.add_space(4.0);
            ui.label(
                RichText::new(tr("media.visualizer_padding"))
                    .small()
                    .color(theme::MUTED),
            );
            ui.add(egui::Slider::new(&mut app.visualizer_padding, 0.0..=0.4));
            ui.add_space(4.0);
            ui.label(
                RichText::new(tr("media.visualizer_bar_count"))
                    .small()
                    .color(theme::MUTED),
            );
            ui.add(egui::Slider::new(&mut app.visualizer_bar_count, 4..=128));
            ui.add_space(6.0);
            if ui
                .add_sized(
                    [ui.available_width(), 26.0],
                    Button::new(format!("📊 {}", tr("media.audio_visualizer"))).corner_radius(5),
                )
                .on_hover_text(tr("media.audio_visualizer_hint"))
                .clicked()
            {
                let duration = Time::from_secs_f64(app.solid_color_duration);
                let settings = AudioVisualizerSettings {
                    background: None,
                    color: SolidColor {
                        r: app.visualizer_color[0],
                        g: app.visualizer_color[1],
                        b: app.visualizer_color[2],
                    },
                    height: app.visualizer_height,
                    padding: app.visualizer_padding,
                    bar_count: app.visualizer_bar_count,
                    alignment: app.visualizer_alignment,
                    ..Default::default()
                };
                app.push_undo();
                let media_id = app.project.add_audio_visualizer(settings, duration);
                add_overlay_to_timeline(app, media_id, duration);
            }
        },
    );
}

fn show_media_list(ui: &mut egui::Ui, app: &mut Application) {
    if app.project.media.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("🎬").size(20.0).color(theme::MUTED));
            ui.add_space(6.0);
            ui.label(RichText::new(tr("media.empty_title")).color(theme::MUTED));
            ui.label(
                RichText::new(tr("media.empty_hint"))
                    .small()
                    .color(theme::MUTED),
            );
        });
        return;
    }

    let media_ids: Vec<_> = app.project.media.keys().copied().collect();
    for id in media_ids {
        media_row(ui, app, id);
    }
}

/// Uma linha da lista de mídia — card com highlight suave no hover (mesmo
/// padrão de um NLE profissional: a lista inteira reage, não só o texto),
/// arrastável pra timeline (`MediaDragPayload`) e com ações rápidas
/// (adicionar/relink/remover) reveladas de forma consistente.
fn media_row(ui: &mut egui::Ui, app: &mut Application, id: MediaId) {
    let Some(media) = app.project.media.get(&id) else {
        return;
    };
    let name = media
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| media.path.to_string_lossy().to_string());
    let is_video = media.metadata.width.is_some();
    let duration = media.metadata.duration;
    let missing = !media.path.exists();

    let row_id = ui.id().with(("media_row", id));
    let row_rect_start = ui.next_widget_position();
    let response = ui
        .scope(|ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                let drag_id = ui.id().with(("media_drag", id));
                ui.dnd_drag_source(
                    drag_id,
                    MediaDragPayload {
                        media_id: id,
                        is_video,
                    },
                    |ui| {
                        ui.label(if is_video { "🎬" } else { "🎵" });
                        if missing {
                            ui.label(RichText::new("⚠").color(theme::MUTED))
                                .on_hover_text(tr("media.missing"));
                            ui.label(RichText::new(&name).color(theme::MUTED).strikethrough());
                        } else {
                            ui.add(egui::Label::new(&name).truncate());
                        }
                    },
                )
                .response
                .on_hover_text(tr("media.drag_hint"));
                ui.add_space((ui.available_width() - 62.0).max(0.0));
                if theme::icon_button(ui, "+", 13.0)
                    .on_hover_text(tr("common.add_to_timeline"))
                    .clicked()
                {
                    app.push_undo();
                    add_to_timeline(app, id, is_video, duration);
                }
                if missing
                    && theme::icon_button(ui, "📁", 13.0)
                        .on_hover_text(tr("media.relink"))
                        .clicked()
                {
                    relink_media(app, id);
                }
                if theme::icon_button(ui, "🗑", 13.0)
                    .on_hover_text(tr("common.remove"))
                    .clicked()
                {
                    app.push_undo();
                    app.project.remove_media(id);
                }
                ui.add_space(4.0);
            });
        })
        .response;

    let row_rect =
        egui::Rect::from_min_size(row_rect_start, egui::vec2(ui.available_width(), 24.0))
            .union(response.rect);
    let hover_t = theme::animate(
        ui,
        row_id,
        response.hovered() || response.contains_pointer(),
    );
    if hover_t > 0.01 {
        ui.painter().rect_filled(
            row_rect.expand2(egui::vec2(0.0, 2.0)),
            4.0,
            theme::MUTED.gamma_multiply(0.08 * hover_t),
        );
    }
    ui.add_space(2.0);
}

pub fn import_media(app: &mut Application) {
    let Some(paths) = rfd::FileDialog::new()
        .add_filter(
            "Mídia",
            &[
                crate::media::VIDEO_EXTENSIONS,
                crate::media::AUDIO_EXTENSIONS,
                crate::media::IMAGE_EXTENSIONS,
            ]
            .concat(),
        )
        .add_filter("Vídeo", crate::media::VIDEO_EXTENSIONS)
        .add_filter("Áudio", crate::media::AUDIO_EXTENSIONS)
        .add_filter("Imagem", crate::media::IMAGE_EXTENSIONS)
        .pick_files()
    else {
        return;
    };

    let mut imported = 0;
    let mut failed = Vec::new();
    for path in paths {
        // Some OS pickers let you type a name past the filter, so re-check.
        if !crate::media::is_supported_media_path(&path) {
            failed.push(format!(
                "{} ({})",
                path.display(),
                tr("status.unsupported_media")
            ));
            continue;
        }
        match metadata::probe(&path) {
            Ok(meta) => {
                app.project.add_media(Media::new(path, meta));
                imported += 1;
            }
            Err(e) => {
                // Não bloqueia o import — entra mesmo com metadados vazios
                // (seção 113: arquivo corrompido/codec desconhecido não pode
                // travar a aplicação), só avisa que não deu pra ler.
                failed.push(format!("{} ({e})", path.display()));
                app.project.add_media(Media::new(path, Default::default()));
                imported += 1;
            }
        }
    }

    app.status_message = Some(if failed.is_empty() {
        tr("status.media_imported").replace("%{count}", &imported.to_string())
    } else {
        tr("status.media_imported_with_errors")
            .replace("%{count}", &imported.to_string())
            .replace("%{files}", &failed.join("; "))
    });
}

/// Relink de mídia (seção 59) — troca o caminho do arquivo sem mexer em
/// nenhum clip que já aponta pro `MediaId`; os metadados são relidos do
/// arquivo novo, já que pode ser uma versão diferente do original.
fn relink_media(app: &mut Application, media_id: MediaId) {
    let Some(path) = rfd::FileDialog::new().pick_file() else {
        return;
    };
    let meta = metadata::probe(&path).unwrap_or_default();
    if let Some(media) = app.project.media.get_mut(&media_id) {
        media.path = path;
        media.metadata = meta;
        app.status_message = Some(tr("status.media_relinked"));
    }
}

fn alignment_label(alignment: VisualizerAlignment) -> String {
    match alignment {
        VisualizerAlignment::Base => tr("media.visualizer_align_base"),
        VisualizerAlignment::Center => tr("media.visualizer_align_center"),
    }
}

/// Pra camadas pensadas como overlay (fundo transparente, ex: a onda de
/// áudio) — sempre cria uma track de vídeo nova por cima de todas as
/// existentes, em vez de compartilhar espaço com o que já tá lá (senão não
/// ia sobrepor nada, só entrar em sequência atrás/na frente na mesma
/// track).
fn add_overlay_to_timeline(app: &mut Application, media_id: MediaId, duration: Time) {
    let track_id = app
        .project
        .add_video_track(app.project.next_video_track_name());
    if let Some(track) = app.project.find_track_mut(track_id) {
        track.clips.push(crate::project::Clip::new(
            media_id,
            Time::ZERO,
            duration,
            Time::ZERO,
            duration,
        ));
    }
}

/// Comum a mídia real e gerada (seção 47-48 — "Add to Timeline").
fn add_to_timeline(app: &mut Application, media_id: MediaId, is_video: bool, duration: Time) {
    let track_id = if is_video {
        if app.project.video_tracks.is_empty() {
            app.project.add_video_track("V1")
        } else {
            app.project.video_tracks[0].id
        }
    } else if app.project.audio_tracks.is_empty() {
        app.project.add_audio_track("A1")
    } else {
        app.project.audio_tracks[0].id
    };

    if let Some(track) = app.project.find_track_mut(track_id) {
        let start = track
            .clips
            .iter()
            .map(|c| c.timeline_end())
            .max()
            .unwrap_or(Time::ZERO);
        let clip = crate::project::Clip::new(media_id, start, duration, Time::ZERO, duration);
        track.clips.push(clip);
    }
}
