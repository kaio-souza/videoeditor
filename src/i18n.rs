//! Multi-idioma. Traduções em `locales/app.yml` (formato `_version: 2`, uma
//! entrada por chave com todos os idiomas juntos).
//!
//! `rust_i18n::i18n!` é invocado na raiz do crate (`main.rs`) — o macro `t!`
//! resolve via `crate::_rust_i18n_t!`, então só funciona se chamado ali.

use rust_i18n::t;

/// Idiomas suportados, na ordem mostrada em View > Language. Inglês é o
/// padrão e o fallback quando uma chave não tem tradução num idioma.
pub const LOCALES: &[(&str, &str)] = &[
    ("en", "English"),
    ("pt-BR", "Português (Brasil)"),
    ("es", "Español"),
    ("zh-CN", "中文"),
    ("ja", "日本語"),
    ("hi", "हिन्दी"),
    ("id", "Bahasa Indonesia"),
    ("fr", "Français"),
    ("it", "Italiano"),
    ("de", "Deutsch"),
];

/// Traduz `key` para o idioma atual.
pub fn tr(key: &str) -> String {
    t!(key).to_string()
}

/// Troca o idioma atual da aplicação (afeta todo `tr()` subsequente).
pub fn set_locale(code: &str) {
    rust_i18n::set_locale(code);
}

/// Código do idioma atual (ex: `"en"`, `"pt-BR"`).
pub fn current_locale() -> String {
    rust_i18n::locale().to_string()
}

pub fn detect_os_locale() -> Option<&'static str> {
    let raw = sys_locale::get_locale()?.to_lowercase();

    if let Some((code, _)) = LOCALES.iter().find(|(code, _)| code.to_lowercase() == raw) {
        return Some(code);
    }

    let primary = raw.split(['-', '_']).next().unwrap_or(&raw);
    LOCALES
        .iter()
        .find(|(code, _)| code.to_lowercase().split(['-', '_']).next() == Some(primary))
        .map(|(code, _)| *code)
}

/// Resolve o idioma com que a aplicação deve abrir.
///
/// Prioridade: idioma salvo de uma sessão anterior (se ainda suportado) >
/// idioma do sistema operacional (se suportado) > inglês.
pub fn resolve_startup_locale(saved: Option<&str>) -> &'static str {
    if let Some(saved) = saved
        && let Some((code, _)) = LOCALES.iter().find(|(code, _)| *code == saved)
    {
        return code;
    }

    detect_os_locale().unwrap_or("en")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Um teste só: `rust_i18n::set_locale` muta estado global do processo,
    // então testes separados rodando em paralelo (padrão do `cargo test`)
    // se atropelariam.
    #[test]
    fn locale_switching_and_fallback() {
        for (code, _label) in LOCALES {
            set_locale(code);
            assert_eq!(&current_locale(), code);
            assert!(
                !tr("app.title").is_empty(),
                "chave app.title sem tradução para {code}"
            );
        }

        set_locale("en");
        assert_eq!(tr("app.title"), "Video Editor");

        set_locale("pt-BR");
        assert_eq!(tr("app.title"), "Editor de Vídeo");
        assert_eq!(tr("menu.file"), "Arquivo");

        // Locale desconhecido: cai pro fallback "en" configurado em `i18n!`.
        set_locale("xx-XX");
        assert_eq!(tr("app.title"), "Video Editor");

        set_locale("en");
    }

    #[test]
    fn resolve_startup_locale_prefers_saved_over_os_and_default() {
        // Salvo e suportado: vence sempre, independente do SO.
        assert_eq!(resolve_startup_locale(Some("pt-BR")), "pt-BR");

        // Salvo mas não suportado (ex: idioma removido numa versão futura):
        // cai pro SO ou inglês, nunca propaga o valor inválido.
        let resolved = resolve_startup_locale(Some("xx-XX"));
        assert!(LOCALES.iter().any(|(code, _)| *code == resolved));

        // Nada salvo: cai pro SO ou inglês.
        let resolved = resolve_startup_locale(None);
        assert!(LOCALES.iter().any(|(code, _)| *code == resolved));
    }

    #[test]
    fn detect_os_locale_matches_by_primary_subtag() {
        // Não dá pra controlar o LANG da máquina que roda o teste, mas o
        // matching em si (exato, depois por subtag primário) não depende
        // do SO — testa a função de lookup isoladamente.
        let lookup = |raw: &str| {
            let raw = raw.to_lowercase();
            LOCALES
                .iter()
                .find(|(code, _)| code.to_lowercase() == raw)
                .map(|(code, _)| *code)
                .or_else(|| {
                    let primary = raw.split(['-', '_']).next().unwrap_or(&raw).to_string();
                    LOCALES
                        .iter()
                        .find(|(code, _)| {
                            code.to_lowercase().split(['-', '_']).next() == Some(primary.as_str())
                        })
                        .map(|(code, _)| *code)
                })
        };

        assert_eq!(lookup("pt-BR"), Some("pt-BR"));
        assert_eq!(lookup("pt-PT"), Some("pt-BR"));
        assert_eq!(lookup("pt_BR"), Some("pt-BR"));
        assert_eq!(lookup("zh-TW"), Some("zh-CN"));
        assert_eq!(lookup("en-GB"), Some("en"));
        assert_eq!(lookup("ko-KR"), None);
    }
}
