use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Toda ação que pode ter uma tecla de atalho — o mesmo conjunto que
/// `Application::handle_shortcuts` já cobria com valores fixos antes disso
/// virar configurável.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    PlayPause,
    Split,
    Delete,
    Duplicate,
    Reverse,
    Undo,
    Redo,
    Import,
    Save,
    Open,
    Export,
}

impl Action {
    pub const ALL: [Action; 11] = [
        Action::PlayPause,
        Action::Split,
        Action::Delete,
        Action::Duplicate,
        Action::Reverse,
        Action::Undo,
        Action::Redo,
        Action::Import,
        Action::Save,
        Action::Open,
        Action::Export,
    ];

    /// Chave de locale do rótulo — reaproveita chaves que já existem
    /// (`common.*`/`menu.*`/`toolbar.*`) onde uma serve, só cria chave nova
    /// (`shortcuts.play_pause`) pro único caso sem equivalente.
    pub fn label_key(self) -> &'static str {
        match self {
            Action::PlayPause => "shortcuts.play_pause",
            Action::Split => "common.split",
            Action::Delete => "common.delete_clip",
            Action::Duplicate => "common.duplicate_clip",
            Action::Reverse => "toolbar.reverse_clip",
            Action::Undo => "common.undo",
            Action::Redo => "common.redo",
            Action::Import => "common.import_media",
            Action::Save => "menu.file.save_project",
            Action::Open => "menu.file.open_project",
            Action::Export => "menu.file.export",
        }
    }
}

/// Uma combinação de teclas. `ctrl` compara contra `Modifiers::command`
/// (Ctrl no Windows/Linux, Cmd no Mac) — mesma convenção cross-platform que
/// o código de atalhos original já usava antes disso virar configurável.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shortcut {
    pub key: egui::Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    pub const fn plain(key: egui::Key) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub const fn ctrl(key: egui::Key) -> Self {
        Self {
            key,
            ctrl: true,
            shift: false,
            alt: false,
        }
    }

    pub const fn ctrl_shift(key: egui::Key) -> Self {
        Self {
            key,
            ctrl: true,
            shift: true,
            alt: false,
        }
    }

    /// Verdadeiro só quando a tecla acabou de ser pressionada **e** os
    /// modificadores batem exatamente (não "pelo menos esses") — sem essa
    /// exatidão, `Ctrl+Shift+Z` também dispararia a ação ligada a `Ctrl+Z`
    /// sozinho, já que os dois "contêm" `Ctrl+Z` como subconjunto de
    /// teclas fisicamente seguradas.
    pub fn pressed(&self, i: &egui::InputState) -> bool {
        i.key_pressed(self.key)
            && i.modifiers.command == self.ctrl
            && i.modifiers.shift == self.shift
            && i.modifiers.alt == self.alt
    }

    /// Texto pra mostrar na UI — `"Ctrl+Shift+Z"`, `"S"`, `"Space"` etc.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(self.key.name());
        parts.join("+")
    }
}

pub type KeyBindings = HashMap<Action, Shortcut>;

/// Atalhos originais do app (seção 82, inalterados) — o preset "Padrão"
/// entre os disponíveis, e a base sobre a qual os outros presets aplicam
/// só as diferenças deles.
pub fn default_bindings() -> KeyBindings {
    use egui::Key;
    HashMap::from([
        (Action::PlayPause, Shortcut::plain(Key::Space)),
        (Action::Split, Shortcut::plain(Key::S)),
        (Action::Delete, Shortcut::plain(Key::Delete)),
        (Action::Duplicate, Shortcut::ctrl(Key::D)),
        (Action::Reverse, Shortcut::plain(Key::R)),
        (Action::Undo, Shortcut::ctrl(Key::Z)),
        (Action::Redo, Shortcut::ctrl_shift(Key::Z)),
        (Action::Import, Shortcut::ctrl(Key::I)),
        (Action::Save, Shortcut::ctrl(Key::S)),
        (Action::Open, Shortcut::ctrl(Key::O)),
        (Action::Export, Shortcut::ctrl(Key::E)),
    ])
}

pub fn vegas_bindings() -> KeyBindings {
    use egui::Key;
    let mut b = default_bindings();
    b.insert(Action::Redo, Shortcut::ctrl(Key::Y));
    b
}

pub fn premiere_bindings() -> KeyBindings {
    use egui::Key;
    let mut b = default_bindings();
    b.insert(Action::Split, Shortcut::ctrl(Key::K));
    b.insert(Action::Export, Shortcut::ctrl(Key::M));
    b
}

pub fn capcut_bindings() -> KeyBindings {
    use egui::Key;
    let mut b = default_bindings();
    b.insert(Action::Split, Shortcut::ctrl(Key::B));
    b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Default,
    Vegas,
    Premiere,
    CapCut,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::Default,
        Preset::Vegas,
        Preset::Premiere,
        Preset::CapCut,
    ];

    pub fn bindings(self) -> KeyBindings {
        match self {
            Preset::Default => default_bindings(),
            Preset::Vegas => vegas_bindings(),
            Preset::Premiere => premiere_bindings(),
            Preset::CapCut => capcut_bindings(),
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Preset::Default => "preferences.preset_default",
            Preset::Vegas => "preferences.preset_vegas",
            Preset::Premiere => "preferences.preset_premiere",
            Preset::CapCut => "preferences.preset_capcut",
        }
    }

    /// Ícone só decorativo pra tela de Preferências (não é dado, fica aqui
    /// mesmo pra não espalhar "qual preset tem qual ícone" por dois
    /// arquivos).
    pub fn icon(self) -> &'static str {
        match self {
            Preset::Default => "⚙",
            Preset::Vegas => "🎬",
            Preset::Premiere => "🎞",
            Preset::CapCut => "✂",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_default_binding() {
        let bindings = default_bindings();
        for action in Action::ALL {
            assert!(bindings.contains_key(&action), "faltando {action:?}");
        }
    }

    #[test]
    fn exact_modifiers_do_not_leak_into_a_stricter_combo() {
        let undo = Shortcut::ctrl(egui::Key::Z);
        let redo = Shortcut::ctrl_shift(egui::Key::Z);
        assert_ne!(undo, redo);
    }

    #[test]
    fn label_formats_modifiers_in_a_stable_order() {
        assert_eq!(Shortcut::ctrl_shift(egui::Key::Z).label(), "Ctrl+Shift+Z");
        assert_eq!(Shortcut::plain(egui::Key::Space).label(), "Space");
    }

    #[test]
    fn presets_only_differ_from_default_where_documented() {
        fn allowed_diffs(preset: Preset) -> &'static [Action] {
            match preset {
                Preset::Default => &[],
                Preset::Vegas => &[Action::Redo],
                Preset::Premiere => &[Action::Split, Action::Export],
                Preset::CapCut => &[Action::Split],
            }
        }

        let default = default_bindings();
        for preset in Preset::ALL {
            let bindings = preset.bindings();
            let allowed = allowed_diffs(preset);
            for action in Action::ALL {
                if allowed.contains(&action) {
                    continue;
                }
                assert_eq!(
                    bindings[&action], default[&action],
                    "{action:?} ({preset:?})"
                );
            }
        }
    }

    #[test]
    fn key_bindings_round_trip_through_json() {
        // `Application::save`/`new` persistem `key_bindings` como JSON via
        // `eframe::Storage` — sem esse round-trip funcionar, um atalho
        // reatribuído não sobreviveria a fechar e reabrir o app.
        let original = default_bindings();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: KeyBindings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }
}
