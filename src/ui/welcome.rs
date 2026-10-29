//! Tela de boas-vindas, mostrada antes do editor abrir (`Application::ui`
//! retorna cedo enquanto `show_welcome` é `true`). Usuário pediu
//! explicitamente: "tela de boas vindas welcome antes da aplicação, para
//! abrir, criar novo, recentes e nela definir qual template / dimensões
//! para facilitar o crop e o projeto ja ficar coerente com a exportação".
//!
//! As três ações do pedido viram as duas colunas abaixo: "Novo Projeto"
//! (templates + dimensões custom) à esquerda, "Abrir"/"Recentes" à
//! direita. Escolher resolução/fps AQUI, antes de qualquer clip existir,
//! é o que resolve o problema real por trás do pedido — o projeto já
//! nasce no aspect ratio certo em vez do usuário só descobrir na hora de
//! exportar (ou de abrir o diálogo de Pan/Crop, `ui/pan_crop.rs`) que a
//! imagem "esticou". `Application::start_new_project` espelha as
//! configurações de export nas do projeto assim que ele é criado, pra
//! completar essa coerência.

use crate::app::Application;
use crate::i18n::tr;
use crate::project::Rational;
use crate::ui::theme;
use egui::{Color32, RichText};

struct ProjectTemplate {
    /// Chave de locale (não texto direto — precisa passar por `tr()`).
    name: &'static str,
    icon: &'static str,
    width: u32,
    height: u32,
    fps: Rational,
}

const TEMPLATES: &[ProjectTemplate] = &[
    ProjectTemplate {
        name: "welcome.template_youtube_1080p",
        icon: "▶",
        width: 1920,
        height: 1080,
        fps: Rational::new(30, 1),
    },
    ProjectTemplate {
        name: "welcome.template_youtube_4k",
        icon: "▶",
        width: 3840,
        height: 2160,
        fps: Rational::new(30, 1),
    },
    ProjectTemplate {
        name: "welcome.template_vertical",
        icon: "📱",
        width: 1080,
        height: 1920,
        fps: Rational::new(30, 1),
    },
    ProjectTemplate {
        name: "welcome.template_square",
        icon: "◻",
        width: 1080,
        height: 1080,
        fps: Rational::new(30, 1),
    },
    ProjectTemplate {
        name: "welcome.template_cinema",
        icon: "🎬",
        width: 1920,
        height: 800,
        fps: Rational::new(24, 1),
    },
];

/// Mesmo conjunto de FPS sugerido pela seção 10 do spec.
const FPS_OPTIONS: &[u32] = &[24, 25, 30, 50, 60];

pub fn show(ui: &mut egui::Ui, app: &mut Application) {
    egui::Frame::new().fill(theme::CANVAS).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new(tr("app.title")).size(28.0).strong());
            ui.add_space(2.0);
            ui.label(RichText::new(tr("welcome.subtitle")).color(theme::MUTED));
            ui.add_space(28.0);

            let panel_width = (ui.available_width() - 32.0).min(860.0);
            ui.allocate_ui(egui::vec2(panel_width, ui.available_height()), |ui| {
                ui.columns(2, |columns| {
                    new_project_column(&mut columns[0], app);
                    open_column(&mut columns[1], app);
                });
            });
        });
    });
}

fn new_project_column(ui: &mut egui::Ui, app: &mut Application) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("welcome.new_project")).strong());
        ui.add_space(8.0);

        for template in TEMPLATES {
            template_button(ui, app, template);
            ui.add_space(4.0);
        }

        ui.add_space(4.0);
        theme::collapsing_section(ui, "welcome_custom", &tr("welcome.custom"), false, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut app.new_project_settings.width).range(16..=7680));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut app.new_project_settings.height).range(16..=4320));
            });
            ui.horizontal(|ui| {
                ui.label(tr("welcome.fps"));
                for &fps in FPS_OPTIONS {
                    let active = app.new_project_settings.fps == Rational::new(fps as i64, 1);
                    if ui.selectable_label(active, fps.to_string()).clicked() {
                        app.new_project_settings.fps = Rational::new(fps as i64, 1);
                    }
                }
            });
        });

        ui.add_space(10.0);
        let create_label = RichText::new(tr("welcome.create"))
            .color(theme::ON_ACCENT)
            .strong();
        let clicked = ui
            .add_sized(
                [ui.available_width(), 32.0],
                egui::Button::new(create_label).fill(theme::ACCENT),
            )
            .clicked();
        if clicked {
            let settings = app.new_project_settings;
            app.start_new_project(settings);
        }
    });
}

/// Mesmo visual/hit-test de `Application::export_preset_button` —
/// destacado (`theme::ACCENT`) quando o rascunho atual já bate com o
/// template, hover suave via `theme::animate` quando não bate.
fn template_button(ui: &mut egui::Ui, app: &mut Application, template: &ProjectTemplate) {
    let active = app.new_project_settings.width == template.width
        && app.new_project_settings.height == template.height
        && app.new_project_settings.fps == template.fps;

    let desired = egui::vec2(ui.available_width(), 28.0);
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
        format!("{} {}", template.icon, tr(template.name)),
        egui::FontId::proportional(12.0),
        text_color,
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(8.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!(
            "{}×{} · {}fps",
            template.width, template.height, template.fps.numerator
        ),
        egui::FontId::proportional(11.0),
        if active {
            theme::ON_ACCENT
        } else {
            theme::MUTED
        },
    );

    if response.clicked() {
        app.new_project_settings.width = template.width;
        app.new_project_settings.height = template.height;
        app.new_project_settings.fps = template.fps;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

fn open_column(ui: &mut egui::Ui, app: &mut Application) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("welcome.open")).strong());
        ui.add_space(8.0);
        if ui
            .add_sized(
                [ui.available_width(), 30.0],
                egui::Button::new(tr("menu.file.open_project")),
            )
            .clicked()
        {
            app.open_project_dialog();
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(RichText::new(tr("welcome.recent")).strong());
        ui.add_space(4.0);

        if app.recent_projects.is_empty() {
            ui.label(
                RichText::new(tr("welcome.no_recents"))
                    .small()
                    .color(theme::MUTED),
            );
            return;
        }

        // Clonado antes do loop — `recent_button` pode chamar
        // `open_project_from_path`, que muta `app.recent_projects` (ex:
        // remove um caminho que sumiu do disco) enquanto ainda estaríamos
        // iterando sobre ele.
        let recents = app.recent_projects.clone();
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .show(ui, |ui| {
                for path in &recents {
                    recent_button(ui, app, path);
                    ui.add_space(2.0);
                }
            });
    });
}

fn recent_button(ui: &mut egui::Ui, app: &mut Application, path: &std::path::Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let parent = path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let desired = egui::vec2(ui.available_width(), 34.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let hover_t = theme::animate(ui, response.id, response.hovered());
    let bg = Color32::TRANSPARENT.lerp_to_gamma(theme::MUTED.gamma_multiply(0.18), hover_t);
    ui.painter().rect_filled(rect, 4.0, bg);

    ui.painter().text(
        rect.left_top() + egui::vec2(8.0, 5.0),
        egui::Align2::LEFT_TOP,
        &name,
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    ui.painter().text(
        rect.left_bottom() + egui::vec2(8.0, -5.0),
        egui::Align2::LEFT_BOTTOM,
        &parent,
        egui::FontId::proportional(10.0),
        theme::MUTED,
    );

    if response.clicked() {
        app.open_project_from_path(path);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}
