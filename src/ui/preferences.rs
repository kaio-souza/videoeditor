use egui::Color32;

use crate::app::Application;
use crate::app::shortcuts::{Action, Preset, Shortcut};
use crate::i18n::tr;
use crate::ui::theme;

const PRESET_RAIL_WIDTH: f32 = 140.0;

pub fn show(app: &mut Application, ctx: &egui::Context) {
    if !app.show_preferences {
        return;
    }

    let mut open = true;
    let mut close_clicked = false;
    let max_window_height = (ctx.viewport_rect().height() - 80.0).max(240.0);

    egui::Window::new(tr("preferences.title"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .max_height(max_window_height)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_min_width(620.0);

            egui::ScrollArea::vertical()
                .max_height(max_window_height - 90.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(PRESET_RAIL_WIDTH, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(PRESET_RAIL_WIDTH);
                                ui.label(
                                    egui::RichText::new(tr("preferences.presets").to_uppercase())
                                        .small()
                                        .color(theme::MUTED)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                                for preset in Preset::ALL {
                                    if preset_row(app, ui, preset) {
                                        app.key_bindings = preset.bindings();
                                        app.recording_shortcut = None;
                                    }
                                }
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(tr("preferences.presets_hint"))
                                        .small()
                                        .color(theme::MUTED),
                                );
                            },
                        );

                        ui.separator();

                        ui.vertical(|ui| {
                            if app.recording_shortcut.is_some() {
                                ui.label(
                                    egui::RichText::new(tr("preferences.press_a_key"))
                                        .color(theme::ACCENT)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                            }
                            egui::Grid::new("prefs_shortcuts_grid")
                                .num_columns(2)
                                .spacing([12.0, 6.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for action in Action::ALL {
                                        ui.label(tr(action.label_key()));
                                        let recording = app.recording_shortcut == Some(action);
                                        let label = if recording {
                                            "…".to_string()
                                        } else {
                                            app.shortcut_label(action)
                                        };
                                        if ui
                                            .selectable_label(
                                                recording,
                                                egui::RichText::new(label).monospace(),
                                            )
                                            .on_hover_text(tr("preferences.click_to_record"))
                                            .clicked()
                                        {
                                            app.recording_shortcut = Some(action);
                                        }
                                        ui.end_row();
                                    }
                                });
                            ui.add_space(6.0);
                            if ui.small_button(tr("preferences.reset_defaults")).clicked() {
                                app.key_bindings = crate::app::shortcuts::default_bindings();
                                app.recording_shortcut = None;
                            }
                        });
                    });
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(tr("common.close")).clicked() {
                    close_clicked = true;
                }
            });
        });

    if !open || close_clicked {
        app.show_preferences = false;
        app.recording_shortcut = None;
    }

    capture_recording(app, ctx);
}

/// Uma linha da trilha de presets à esquerda — mesmo padrão de linha
/// clicável alinhada à esquerda que `Application::export_preset_button`
/// (não dá pra reaproveitar a função direto: aquela é `impl Application`
/// e compara campos de export, esta compara o `HashMap` de atalhos
/// inteiro). Devolve `true` no frame em que foi clicada.
fn preset_row(app: &Application, ui: &mut egui::Ui, preset: Preset) -> bool {
    let active = app.key_bindings == preset.bindings();
    let desired = egui::vec2(ui.available_width(), 26.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let hover_t = theme::animate(ui, response.id, response.hovered());

    let bg = if active {
        theme::ACCENT
    } else {
        Color32::TRANSPARENT.lerp_to_gamma(theme::MUTED.gamma_multiply(0.18), hover_t)
    };
    ui.painter().rect_filled(rect, 4.0, bg);

    let text_color = if active {
        theme::ON_ACCENT
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{} {}", preset.icon(), tr(preset.label_key())),
        egui::FontId::proportional(12.5),
        text_color,
    );

    response.clicked()
}

/// Enquanto `recording_shortcut` está setado, captura a próxima tecla
/// pressionada e vira o novo atalho daquela ação. `Escape` cancela sem
/// gravar nada (jeito padrão de sair de um modo de captura sem querer
/// atribuir `Escape` como atalho de alguma coisa).
fn capture_recording(app: &mut Application, ctx: &egui::Context) {
    let Some(action) = app.recording_shortcut else {
        return;
    };

    let mut new_shortcut = None;
    let mut cancel = false;
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Escape) {
            cancel = true;
            return;
        }
        for event in &i.events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            // Modificador sozinho (só Ctrl, só Shift...) não é um atalho
            // válido por si — espera a tecla "principal" que vem junto.
            if is_pure_modifier(*key) {
                continue;
            }
            new_shortcut = Some(Shortcut {
                key: *key,
                ctrl: modifiers.command,
                shift: modifiers.shift,
                alt: modifiers.alt,
            });
            break;
        }
    });

    if cancel {
        app.recording_shortcut = None;
    } else if let Some(shortcut) = new_shortcut {
        app.key_bindings.insert(action, shortcut);
        app.recording_shortcut = None;
    }
}

fn is_pure_modifier(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}
