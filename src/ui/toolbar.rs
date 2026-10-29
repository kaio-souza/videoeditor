use crate::app::Application;
use crate::i18n::tr;
use egui::{Button, RichText};

fn icon_button(ui: &mut egui::Ui, icon: &str, tooltip: String) -> egui::Response {
    ui.add(Button::new(RichText::new(icon).size(14.0)).frame(false))
        .on_hover_text(tooltip)
}

pub fn show(ui: &mut egui::Ui, app: &mut Application) {
    ui.horizontal_centered(|ui| {
        let play_icon = if app.playing { "⏸" } else { "⏵" };
        if icon_button(ui, play_icon, tr("toolbar.play_pause")).clicked() {
            app.playing = !app.playing;
        }
        if icon_button(ui, "✂", tr("toolbar.split")).clicked() {
            app.split_selected_at_playhead();
        }
        if icon_button(ui, "🗑", tr("toolbar.delete_clip")).clicked() {
            app.delete_selected_clips();
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        if icon_button(ui, "↺", tr("toolbar.undo")).clicked() {
            app.undo();
        }
        if icon_button(ui, "↻", tr("toolbar.redo")).clicked() {
            app.redo();
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        if icon_button(ui, "⏪", tr("toolbar.reverse_clip")).clicked() {
            app.toggle_reverse_selected();
        }
    });
}
