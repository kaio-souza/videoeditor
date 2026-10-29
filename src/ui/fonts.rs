use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{FontData, FontFamily};

/// Caminhos comuns em distros Linux para uma fonte Noto Sans CJK (cobre
/// chinês simplificado e japonês). `.ttc`/`.otf` collection — usamos a
/// primeira face (`index = 0`); todas cobrem o mesmo repertório Han.
const CJK_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJKsc-Regular.otf",
    "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
];

/// Caminhos comuns para Noto Sans Devanagari (cobre hindi).
const DEVANAGARI_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSansDevanagari-Regular.ttf",
];

/// Registra as fontes de fallback disponíveis no sistema. Chamar uma vez,
/// depois que o tema base já foi aplicado.
pub fn install_fallbacks(ctx: &egui::Context) {
    install_fallback(ctx, "noto-cjk", CJK_CANDIDATES);
    install_fallback(ctx, "noto-devanagari", DEVANAGARI_CANDIDATES);
}

fn install_fallback(ctx: &egui::Context, name: &str, candidates: &[&str]) {
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };

    ctx.add_font(FontInsert::new(
        name,
        FontData::from_owned(bytes),
        vec![
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Lowest,
            },
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}
