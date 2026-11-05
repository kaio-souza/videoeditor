// Editor de Vídeo em Rust — entry point.
// Ver CLAUDE.md para arquitetura e roadmap completo.

// `i18n!` gera os itens que `t!` usa (via `crate::_rust_i18n_t!`) — precisa
// ficar na raiz do crate. Ver `src/i18n.rs` para o wrapper usado no resto do
// código.
rust_i18n::i18n!("locales", fallback = "en");

mod app;
mod audio;
mod effects;
mod error;
mod export;
mod i18n;
mod media;
mod project;
mod ui;
mod video;

use app::Application;

fn main() -> eframe::Result<()> {
    i18n::set_locale("en");

    std::thread::spawn(|| {
        if let Err(err) = ffmpeg_sidecar::download::auto_download() {
            eprintln!("[MEDIA] não consegui baixar o FFmpeg automaticamente: {err}");
        }
    });

    let native_options = eframe::NativeOptions::default();

    eframe::run_native(
        &i18n::tr("app.title"),
        native_options,
        Box::new(|cc| Ok(Box::new(Application::new(cc)))),
    )
}
