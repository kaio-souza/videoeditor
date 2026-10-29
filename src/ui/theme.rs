//! Tema visual da aplicação — cores, espaçamento, arredondamento.
//!
//! Aplicado uma vez em `Application::new`. Painéis individuais podem se
//! referenciar às constantes daqui para manter consistência (ex: `theme::CANVAS`
//! para a área de preview, que deve ser mais escura que os painéis ao redor).

use egui::{Color32, CornerRadius, Margin, Stroke, Vec2};

/// Fundo dos painéis de chrome (menu bar, toolbar) — ligeiramente mais escuro
/// que os painéis de conteúdo, pra ler como "moldura" da janela.
pub const CHROME: Color32 = Color32::from_rgb(0x18, 0x18, 0x1b);

/// Fundo da área de preview/canvas de vídeo — quase preto, como em qualquer
/// editor de vídeo profissional, pra não competir com a imagem.
pub const CANVAS: Color32 = Color32::from_rgb(0x0c, 0x0c, 0x0e);

/// Cor de destaque (seleção, botões primários, links, playhead).
///
/// Escura o bastante pra texto branco em cima ler bem — ver `ON_ACCENT`.
pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xe0);

/// Texto sobre um fundo `ACCENT` (botão primário, item selecionado).
pub const ON_ACCENT: Color32 = Color32::WHITE;

/// Texto apagado (estados vazios, hints).
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x8d, 0x93);

/// Botão de ícone compacto sem moldura — usado em barras densas (cabeçalho
/// de track, lista de mídia, inspector) pra dar um visual consistente em
/// vez de cada painel montar seu próprio botãozinho com padding/tamanho
/// diferente. `frame(false)` já zera o `button_padding` global (12x6,
/// pensado pra botões de texto normais, exageraria muito num glifo de
/// ícone sozinho).
pub fn icon_button(ui: &mut egui::Ui, icon: &str, size: f32) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(icon).size(size)).frame(false))
}

/// Anima uma interpolação 0→1 suave (ease, não instantânea) pra estados de
/// hover/seleção — é o que dá o "fluido/smooth" que um editor profissional
/// tem em vez de cores trocando de golpe.
pub fn animate(ui: &egui::Ui, id: egui::Id, target: bool) -> f32 {
    ui.ctx().animate_bool(id, target)
}

pub fn collapsing_section(
    ui: &mut egui::Ui,
    id_source: &str,
    title: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(6.0);
    egui::CollapsingHeader::new(egui::RichText::new(title).strong().size(12.0))
        .id_salt(id_source)
        .default_open(default_open)
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, add_contents);
        });
}

/// Aplica o tema escuro customizado ao contexto do egui.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    let panel = Color32::from_rgb(0x1e, 0x1e, 0x22);
    let base = Color32::from_rgb(0x15, 0x15, 0x17);

    visuals.panel_fill = panel;
    visuals.window_fill = panel;
    visuals.faint_bg_color = base;
    visuals.extreme_bg_color = CANVAS;
    visuals.code_bg_color = base;

    visuals.selection.bg_fill = ACCENT;
    // `selection.stroke.color` doubles as the text color egui draws over
    // `selection.bg_fill` (selected menu items, `selectable_label`, …) — it
    // must NOT equal `ACCENT` itself, or that text is invisible.
    visuals.selection.stroke = Stroke::new(1.0, ON_ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = Color32::from_rgb(0xe0, 0xa5, 0x3e);
    visuals.error_fg_color = Color32::from_rgb(0xe0, 0x5a, 0x5a);

    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.widgets.noninteractive.weak_bg_fill = panel;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x2f);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x2f);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x35, 0x35, 0x3c);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x35, 0x35, 0x3c);
    visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.85);
    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.85);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, ON_ACCENT);

    let corner_radius = CornerRadius::same(5);
    visuals.widgets.noninteractive.corner_radius = corner_radius;
    visuals.widgets.inactive.corner_radius = corner_radius;
    visuals.widgets.hovered.corner_radius = corner_radius;
    visuals.widgets.active.corner_radius = corner_radius;
    visuals.widgets.open.corner_radius = corner_radius;
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);

    visuals.window_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals = visuals;
        style.text_styles = [
            (
                egui::TextStyle::Small,
                egui::FontId::new(9.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Heading,
                egui::FontId::new(15.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(12.0, egui::FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(7.0, 5.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.window_margin = Margin::same(10);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.indent = 16.0;
    });

    super::fonts::install_fallbacks(ctx);
}
