use crate::app::Application;
use crate::i18n::tr;
use crate::project::{GeneratedMedia, MediaId, SolidColor, Time, VisualizerAlignment};
use crate::ui::theme;
use egui::RichText;

pub fn show(ui: &mut egui::Ui, app: &mut Application) {
    ui.label(
        RichText::new(tr("inspector.header").to_uppercase())
            .small()
            .color(theme::MUTED)
            .strong(),
    );
    ui.add_space(8.0);
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(&clip_id) = app.selection.clips.iter().next() {
                show_clip(ui, app, clip_id);
            } else if let Some(track_id) = app.selection.track {
                show_track(ui, app, track_id);
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new("☰").size(20.0).color(theme::MUTED));
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("inspector.empty")).color(theme::MUTED));
                });
            }
        });
}

/// Linha padrão dentro de um `theme::collapsing_section`: rótulo pequeno em
/// cima, controle embaixo — consistente pra todo slider do inspetor em vez
/// de cada um improvisar seu próprio espaçamento.
fn row(ui: &mut egui::Ui, label: &str, slider: egui::Slider<'_>) -> egui::Response {
    ui.label(RichText::new(label).small().color(theme::MUTED));
    ui.add_space(2.0);
    let response = ui.add(slider);
    ui.add_space(6.0);
    response
}

fn show_clip(ui: &mut egui::Ui, app: &mut Application, clip_id: crate::project::ClipId) {
    let is_audio = app
        .project
        .audio_tracks
        .iter()
        .any(|t| t.clips.iter().any(|c| c.id == clip_id));

    let Some(clip) = app
        .project
        .video_tracks
        .iter_mut()
        .chain(app.project.audio_tracks.iter_mut())
        .find_map(|t| t.clips.iter_mut().find(|c| c.id == clip_id))
    else {
        return;
    };
    let source = clip.source;

    theme::collapsing_section(ui, "insp_clip", &tr("inspector.section_clip"), true, |ui| {
        if is_audio {
            row(
                ui,
                &tr("inspector.volume"),
                egui::Slider::new(&mut clip.volume, 0.0..=2.0).suffix("x"),
            );
        } else {
            row(
                ui,
                &tr("inspector.opacity"),
                egui::Slider::new(&mut clip.opacity, 0.0..=1.0),
            );
        }
    });

    theme::collapsing_section(ui, "insp_fade", &tr("inspector.section_fade"), true, |ui| {
        let mut fade_in = clip.fade_in.as_secs_f64();
        if row(
            ui,
            &tr("inspector.fade_in"),
            egui::Slider::new(&mut fade_in, 0.0..=5.0).suffix("s"),
        )
        .changed()
        {
            clip.fade_in = Time::from_secs_f64(fade_in);
        }

        let mut fade_out = clip.fade_out.as_secs_f64();
        if row(
            ui,
            &tr("inspector.fade_out"),
            egui::Slider::new(&mut fade_out, 0.0..=5.0).suffix("s"),
        )
        .changed()
        {
            clip.fade_out = Time::from_secs_f64(fade_out);
        }
    });

    theme::collapsing_section(
        ui,
        "insp_playback",
        &tr("inspector.section_playback"),
        true,
        |ui| {
            ui.checkbox(&mut clip.reversed, tr("inspector.reversed"));
        },
    );

    show_generated_editor(ui, app, source);
}

/// Se o clipe aponta pra uma mídia gerada (Solid Color / Audio Visualizer,
/// seção 35), mostra os controles pra editar essa mídia *depois* de já
/// inserida na timeline — sem isso a única forma de mudar a cor era apagar
/// o clipe e gerar de novo. Cada clique em "Solid Color"/"Audio Visualizer"
/// no painel de mídia cria um `GeneratedMediaEntry` próprio (não
/// compartilhado), então editar aqui só afeta este clipe.
fn show_generated_editor(ui: &mut egui::Ui, app: &mut Application, source: MediaId) {
    let mut changed = false;
    if let Some(entry) = app.project.generated.get_mut(&source) {
        match &mut entry.kind {
            GeneratedMedia::SolidColor(color) => {
                theme::collapsing_section(
                    ui,
                    "insp_gen_color",
                    &tr("media.solid_color"),
                    true,
                    |ui| {
                        let mut rgb = [color.r, color.g, color.b];
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            color.r = rgb[0];
                            color.g = rgb[1];
                            color.b = rgb[2];
                            changed = true;
                        }
                    },
                );
            }
            GeneratedMedia::AudioVisualizer(settings) => {
                theme::collapsing_section(
                    ui,
                    "insp_gen_viz",
                    &tr("media.audio_visualizer"),
                    true,
                    |ui| {
                        let mut rgb = [settings.color.r, settings.color.g, settings.color.b];
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            settings.color.r = rgb[0];
                            settings.color.g = rgb[1];
                            settings.color.b = rgb[2];
                            changed = true;
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(tr("media.visualizer_height"))
                                .small()
                                .color(theme::MUTED),
                        );
                        if ui
                            .add(egui::Slider::new(&mut settings.height, 0.05..=1.0))
                            .changed()
                        {
                            changed = true;
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(tr("media.visualizer_padding"))
                                .small()
                                .color(theme::MUTED),
                        );
                        if ui
                            .add(egui::Slider::new(&mut settings.padding, 0.0..=0.4))
                            .changed()
                        {
                            changed = true;
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(tr("media.visualizer_bar_count"))
                                .small()
                                .color(theme::MUTED),
                        );
                        if ui
                            .add(egui::Slider::new(&mut settings.bar_count, 4..=128))
                            .changed()
                        {
                            changed = true;
                        }
                        ui.add_space(4.0);
                        let current = match settings.alignment {
                            VisualizerAlignment::Base => tr("media.visualizer_align_base"),
                            VisualizerAlignment::Center => tr("media.visualizer_align_center"),
                        };
                        egui::ComboBox::from_id_salt("insp_viz_alignment")
                            .selected_text(current)
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(
                                        &mut settings.alignment,
                                        VisualizerAlignment::Base,
                                        tr("media.visualizer_align_base"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if ui
                                    .selectable_value(
                                        &mut settings.alignment,
                                        VisualizerAlignment::Center,
                                        tr("media.visualizer_align_center"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                        ui.add_space(6.0);
                        let mut has_bg = settings.background.is_some();
                        if ui
                            .checkbox(&mut has_bg, tr("inspector.background"))
                            .changed()
                        {
                            settings.background = if has_bg {
                                Some(
                                    settings
                                        .background
                                        .unwrap_or(SolidColor { r: 0, g: 0, b: 0 }),
                                )
                            } else {
                                None
                            };
                            changed = true;
                        }
                        if let Some(bg) = &mut settings.background {
                            let mut bg_rgb = [bg.r, bg.g, bg.b];
                            if ui.color_edit_button_srgb(&mut bg_rgb).changed() {
                                bg.r = bg_rgb[0];
                                bg.g = bg_rgb[1];
                                bg.b = bg_rgb[2];
                                changed = true;
                            }
                        }
                    },
                );
            }
        }
    }
    if changed {
        app.preview_key = None;
    }
}

fn show_track(ui: &mut egui::Ui, app: &mut Application, track_id: crate::project::TrackId) {
    let Some(track) = app.project.find_track_mut(track_id) else {
        return;
    };

    ui.add_space(4.0);
    ui.label(RichText::new(&track.name).strong().size(13.0));

    let is_audio = matches!(track.track_type, crate::project::TrackType::Audio);
    let mute_label = if is_audio {
        tr("inspector.muted")
    } else {
        tr("inspector.hidden")
    };

    theme::collapsing_section(
        ui,
        "insp_track",
        &tr("inspector.section_track"),
        true,
        |ui| {
            ui.checkbox(&mut track.muted, mute_label);
            if is_audio {
                ui.add_space(6.0);
                row(
                    ui,
                    &tr("inspector.volume"),
                    egui::Slider::new(&mut track.volume, 0.0..=2.0).suffix("x"),
                );
                row(
                    ui,
                    &tr("inspector.pan"),
                    egui::Slider::new(&mut track.pan, -1.0..=1.0),
                );
            }
        },
    );
}
