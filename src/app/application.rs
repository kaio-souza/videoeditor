use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::i18n::{self, tr};
use crate::project::{Project, Selection, Time};
use crate::ui;
use crate::ui::theme;
use egui::{Button, Color32, CornerRadius, Frame, Margin, RichText};

const LOCALE_STORAGE_KEY: &str = "locale";
const KEY_BINDINGS_STORAGE_KEY: &str = "key_bindings";
const RECENT_PROJECTS_STORAGE_KEY: &str = "recent_projects";

/// Export rodando numa thread separada (seção 65-66 — Progress/Cancel): o
/// `ffmpeg` de um vídeo de alguns minutos leva segundos, e travar a UI
/// inteira nesse meio tempo não seria aceitável.
pub struct ExportJob {
    progress: Arc<Mutex<f32>>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<std::result::Result<(), String>>>>,
}

pub struct Application {
    last_locale: String,
    pub project: Project,
    pub selection: Selection,
    pub playhead: Time,
    pub playing: bool,
    pub timeline_zoom: f32,
    pub preview_texture: Option<egui::TextureHandle>,
    /// `(MediaId, source_time, opacity*fade bits)` de cada camada ativa da
    /// última composição — recompõe só quando isso muda (seção 9/43).
    pub preview_key: Option<Vec<(crate::project::MediaId, Time, u32)>>,
    pub pending_preview: Option<crate::ui::preview::PendingComposite>,
    pub solid_color: [u8; 3],
    pub solid_color_duration: f64,
    /// Customização do gerador de Audio Visualizer (botão único, seção
    /// 37-40): cor, altura (fração 0.0-1.0 do frame), margem lateral e
    /// alinhamento base/centro persistem entre usos do botão, igual
    /// `solid_color`.
    pub visualizer_color: [u8; 3],
    pub visualizer_height: f32,
    pub visualizer_padding: f32,
    pub visualizer_bar_count: u32,
    pub visualizer_alignment: crate::project::VisualizerAlignment,
    pub export_job: Option<ExportJob>,
    pub show_export_dialog: bool,
    pub export_width: u32,
    pub export_height: u32,
    pub export_fps: crate::project::Rational,
    pub export_quality: crate::export::VideoQuality,
    pub export_format: crate::export::ExportFormat,
    /// Última mensagem de status (erro ou aviso) pra mostrar na barra de
    /// menu — seção 113: import/decode/save/load nunca devem falhar em
    /// silêncio, mesmo quando o erro em si é recuperável (ex: metadados
    /// não lidos, projeto não carregado).
    pub status_message: Option<String>,
    pub key_bindings: crate::app::shortcuts::KeyBindings,
    pub show_preferences: bool,
    /// Ação esperando a próxima tecla pra virar seu novo atalho, enquanto a
    /// tela de Preferências está aberta — `None` fora desse modo de
    /// gravação. Ver `ui/preferences.rs`.
    pub recording_shortcut: Option<crate::app::shortcuts::Action>,
    /// Clipe sendo editado na tela de Pan/Crop (seção 19, estilo Sony
    /// Vegas) — `None` fecha o diálogo. Ver `ui/pan_crop.rs`.
    pub pan_crop_clip: Option<crate::project::ClipId>,
    /// Rascunho editado no diálogo — só grava em `clip.transform` de
    /// verdade se o usuário confirmar ("Aplicar"), igual todo outro
    /// diálogo modal deste app (export, preferências).
    pub pan_crop_draft: crate::project::Transform,
    /// Frame fonte decodificado uma vez quando o diálogo abre, reusado a
    /// cada frame de UI enquanto o usuário arrasta o retângulo — decodificar
    /// de novo a cada frame de arrasto (`ffmpeg` síncrono) travaria o
    /// diálogo.
    pub pan_crop_texture: Option<egui::TextureHandle>,
    /// Resolução real do source (`Media.metadata.width/height`) — o
    /// diálogo mostra o frame inteiro sem cortar (letterbox), então
    /// precisa saber o aspect ratio verdadeiro do source pra calcular onde
    /// a imagem visível cai dentro do canvas e travar o retângulo de
    /// Pan/Crop na mesma região.
    pub pan_crop_source_size: (u32, u32),
    /// Picos de forma de onda por `(MediaId, source_start, source_duration)`
    /// (seção 46) — essa chave já invalida sozinha quando um trim muda o
    /// trecho do source que o clip mostra. Nunca removido (sem pressão de
    /// memória séria pro tamanho de projeto que isso aguenta hoje).
    pub waveform_cache:
        std::collections::HashMap<(crate::project::MediaId, Time, Time), std::sync::Arc<Vec<f32>>>,
    /// Tela de boas-vindas (`ui/welcome.rs`) — usuário pediu explicitamente
    /// uma tela antes do editor pra abrir/criar novo/recentes e escolher
    /// template/dimensões, "para o crop e o projeto ja ficar coerente com
    /// a exportação". `true` no boot; some assim que um projeto é criado
    /// ou aberto, volta com "Novo Projeto..." no menu Arquivo.
    pub show_welcome: bool,
    /// Rascunho de `ProjectSettings` editado na tela de boas-vindas antes
    /// de confirmar "Criar Projeto" — templates e os campos de largura/
    /// altura/fps custom escrevem aqui, nunca direto em `project.settings`.
    pub new_project_settings: crate::project::ProjectSettings,
    /// Projetos abertos/salvos recentemente (mais recente primeiro, máx.
    /// `RECENT_PROJECTS_MAX`), persistido via `eframe::Storage` igual
    /// `key_bindings` — não é dado de projeto, é preferência do app.
    pub recent_projects: Vec<std::path::PathBuf>,
    /// Caminho do projeto atual no disco, se ele já foi salvo/aberto uma
    /// vez — `None` num projeto novo que nunca passou por "Salvar Como".
    /// É o que diferencia "Salvar" (grava direto aqui, sem diálogo) de
    /// "Salvar Como..." (sempre pergunta): usuário relatou "salvar
    /// projeto sempre é 'salvar como...' tem que ter uma opção só de
    /// salvar quando o arquivo ja existe".
    current_project_path: Option<std::path::PathBuf>,
    audio_engine: Option<crate::audio::playback::AudioEngine>,
    was_playing: bool,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
}

const RECENT_PROJECTS_MAX: usize = 8;

/// Lógica pura por trás de `Application::track_recent` — mais recente vai
/// pro início, uma entrada já existente é movida em vez de duplicada, e a
/// lista nunca passa de `RECENT_PROJECTS_MAX`. Extraído de `Application`
/// pra dar pra testar sem precisar de um `eframe::CreationContext` real.
fn track_recent_in(list: &mut Vec<std::path::PathBuf>, path: &std::path::Path) {
    let path = path.to_path_buf();
    list.retain(|p| p != &path);
    list.insert(0, path);
    list.truncate(RECENT_PROJECTS_MAX);
}

impl Application {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let saved_locale = cc
            .storage
            .and_then(|storage| storage.get_string(LOCALE_STORAGE_KEY));
        i18n::set_locale(i18n::resolve_startup_locale(saved_locale.as_deref()));

        let mut key_bindings = crate::app::shortcuts::default_bindings();
        if let Some(saved) = cc
            .storage
            .and_then(|storage| storage.get_string(KEY_BINDINGS_STORAGE_KEY))
            .and_then(|s| serde_json::from_str::<crate::app::shortcuts::KeyBindings>(&s).ok())
        {
            key_bindings.extend(saved);
        }

        let recent_projects = cc
            .storage
            .and_then(|storage| storage.get_string(RECENT_PROJECTS_STORAGE_KEY))
            .and_then(|s| serde_json::from_str::<Vec<std::path::PathBuf>>(&s).ok())
            .unwrap_or_default();

        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::Title(tr("app.title")));

        let mut project = Project::default();
        project.add_video_track("V1");
        project.add_audio_track("A1");
        let settings = project.settings;

        Self {
            last_locale: i18n::current_locale(),
            project,
            selection: Selection::default(),
            playhead: Time::ZERO,
            playing: false,
            timeline_zoom: 40.0,
            preview_texture: None,
            preview_key: None,
            pending_preview: None,
            solid_color: [0x80, 0x80, 0x80],
            solid_color_duration: 5.0,
            visualizer_color: [0xff, 0x2d, 0x78],
            visualizer_height: 1.0,
            visualizer_padding: 0.08,
            visualizer_bar_count: 32,
            visualizer_alignment: crate::project::VisualizerAlignment::Base,
            export_job: None,
            show_export_dialog: false,
            export_width: settings.width,
            export_height: settings.height,
            export_fps: settings.fps,
            export_quality: crate::export::VideoQuality::Medium,
            export_format: crate::export::ExportFormat::Mp4,
            status_message: None,
            key_bindings,
            show_preferences: false,
            recording_shortcut: None,
            pan_crop_clip: None,
            pan_crop_draft: crate::project::Transform::default(),
            pan_crop_texture: None,
            pan_crop_source_size: (1, 1),
            waveform_cache: std::collections::HashMap::new(),
            show_welcome: true,
            new_project_settings: settings,
            recent_projects,
            current_project_path: None,
            audio_engine: crate::audio::playback::AudioEngine::new(),
            was_playing: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Reinicia a reprodução de áudio a partir do `playhead` atual (seção
    /// 12). Só é chamado na borda de subida de `playing` — arrastar o
    /// playhead ou editar a timeline enquanto já está tocando não ressincroniza
    /// o áudio até pausar/tocar de novo (mesma limitação do vídeo, ver
    /// CLAUDE.md).
    fn restart_audio(&mut self) {
        let Some(engine) = &self.audio_engine else {
            return;
        };
        let samples = crate::audio::mixer::build_playback_buffer(
            &self.project,
            self.playhead,
            engine.sample_rate(),
            engine.channels(),
        );
        engine.play_buffer(samples);
    }

    fn sync_audio_playback(&mut self) {
        if self.playing && !self.was_playing {
            self.restart_audio();
        } else if !self.playing
            && self.was_playing
            && let Some(engine) = &self.audio_engine
        {
            engine.pause();
        }
        self.was_playing = self.playing;
    }

    /// Atalhos globais (seção 82), configuráveis (`key_bindings`, tela de
    /// Preferências). Ignora entrada quando o foco está num campo de texto
    /// (`wants_keyboard_input`), pra não roubar `S`/`Delete` de um
    /// `TextEdit`, e enquanto a Preferências está gravando um atalho novo
    /// (senão a tecla usada pra gravar também disparava a ação antiga).
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.recording_shortcut.is_some() {
            return;
        }

        use crate::app::shortcuts::Action;
        let triggered: Vec<Action> = ctx.input(|i| {
            Action::ALL
                .into_iter()
                .filter(|a| {
                    self.key_bindings
                        .get(a)
                        .is_some_and(|shortcut| shortcut.pressed(i))
                })
                .collect()
        });
        // `Delete`/`Backspace` continuam ambos apagando (o `Backspace`
        // nunca foi configurável — atalho "físico" adicional de
        // conveniência, não uma `Action` própria).
        let backspace = ctx.input(|i| i.key_pressed(egui::Key::Backspace));

        for action in triggered {
            match action {
                Action::PlayPause => self.playing = !self.playing,
                Action::Split => self.split_selected_at_playhead(),
                Action::Delete => self.delete_selected_clips(),
                Action::Duplicate => self.duplicate_selected_clips(),
                Action::Reverse => self.toggle_reverse_selected(),
                Action::Undo => self.undo(),
                Action::Redo => self.redo(),
                Action::Import => ui::media_panel::import_media(self),
                Action::Save => self.save_project(),
                Action::Open => self.open_project_dialog(),
                Action::Export => self.open_export_dialog(),
            }
        }
        if backspace {
            self.delete_selected_clips();
        }
    }

    pub fn shortcut_label(&self, action: crate::app::shortcuts::Action) -> String {
        self.key_bindings
            .get(&action)
            .map(crate::app::shortcuts::Shortcut::label)
            .unwrap_or_default()
    }

    /// Picos de forma de onda do trecho `[source_start, source_start +
    /// source_duration)` de `media_id`, computando (e cacheando) na
    /// primeira vez que esse trecho exato é pedido (seção 46).
    pub fn waveform_for(
        &mut self,
        media_id: crate::project::MediaId,
        source_start: Time,
        source_duration: Time,
    ) -> std::sync::Arc<Vec<f32>> {
        let key = (media_id, source_start, source_duration);
        if let Some(peaks) = self.waveform_cache.get(&key) {
            return peaks.clone();
        }
        let peaks = self
            .project
            .get_media(media_id)
            .map(|media| {
                crate::media::waveform::peaks(
                    &media.path,
                    source_start.as_secs_f64(),
                    source_duration.as_secs_f64(),
                )
            })
            .unwrap_or_default();
        let peaks = std::sync::Arc::new(peaks);
        self.waveform_cache.insert(key, peaks.clone());
        peaks
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let locale = i18n::current_locale();
        if locale != self.last_locale {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(tr("app.title")));
            self.last_locale = locale;
        }
    }

    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.project.clone());
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack
                .push(std::mem::replace(&mut self.project, prev));
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack
                .push(std::mem::replace(&mut self.project, next));
        }
    }

    /// "Salvar" de verdade: grava direto em cima do arquivo já conhecido
    /// (`current_project_path`) sem pedir nada ao usuário. Só cai pro
    /// diálogo quando o projeto ainda não tem um caminho — primeiro save
    /// de um projeto novo continua precisando perguntar onde.
    fn save_project(&mut self) {
        let Some(path) = self.current_project_path.clone() else {
            self.save_project_as_dialog();
            return;
        };
        self.write_project_to(&path);
    }

    /// "Salvar Como...": sempre pergunta o caminho, mesmo quando o
    /// projeto já tinha um — é o único jeito de trocar de arquivo ou
    /// duplicar o projeto atual sob outro nome.
    fn save_project_as_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Editor de Vídeo", &["myeditor"])
            .set_file_name("project.myeditor")
            .save_file()
        else {
            return;
        };
        self.write_project_to(&path);
    }

    fn write_project_to(&mut self, path: &std::path::Path) {
        self.status_message = Some(
            match crate::project::serialization::save_project(&self.project, path) {
                Ok(()) => {
                    self.track_recent(path);
                    self.current_project_path = Some(path.to_path_buf());
                    tr("status.saved").replace("%{path}", &path.display().to_string())
                }
                Err(e) => tr("status.save_error").replace("%{error}", &e.to_string()),
            },
        );
    }

    pub(crate) fn open_project_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Editor de Vídeo", &["myeditor"])
            .pick_file()
        else {
            return;
        };
        self.open_project_from_path(&path);
    }

    /// Abre um `.myeditor` já sabendo o caminho — usado tanto pelo picker
    /// de arquivo (`open_project_dialog`) quanto pela lista de "Recentes"
    /// da tela de boas-vindas (`ui/welcome.rs`), que já tem o `PathBuf` em
    /// mãos e não deve passar de novo pelo diálogo do sistema.
    pub(crate) fn open_project_from_path(&mut self, path: &std::path::Path) {
        match crate::project::serialization::load_project(path) {
            Ok(project) => {
                self.push_undo();
                self.project = project;
                self.selection.clear();
                self.playhead = Time::ZERO;
                self.show_welcome = false;
                self.track_recent(path);
                self.current_project_path = Some(path.to_path_buf());

                // Relink de mídia (seção 59) tem UI em `ui/media_panel.rs`
                // (botão "Locate File" por mídia faltando) — aqui só avisa
                // quais faltam, não trava a abertura do projeto.
                let missing: Vec<String> =
                    crate::project::serialization::missing_media(&self.project)
                        .into_iter()
                        .map(|p| p.display().to_string())
                        .collect();
                self.status_message = Some(if missing.is_empty() {
                    tr("status.project_opened").replace("%{path}", &path.display().to_string())
                } else {
                    tr("status.project_opened_missing_media")
                        .replace("%{files}", &missing.join(", "))
                });
            }
            Err(e) => {
                // Projeto recente pode ter sido movido/apagado desde a
                // última vez — some da lista em vez de ficar reoferecendo
                // um caminho que sempre vai falhar.
                self.recent_projects.retain(|p| p != path);
                self.status_message =
                    Some(tr("status.open_error").replace("%{error}", &e.to_string()))
            }
        }
    }

    fn track_recent(&mut self, path: &std::path::Path) {
        track_recent_in(&mut self.recent_projects, path);
    }

    /// Cria um projeto novo a partir do rascunho escolhido na tela de
    /// boas-vindas (template ou dimensões custom) — usuário pediu
    /// explicitamente que isso deixe "o projeto ja ficar coerente com a
    /// exportação", então as configurações de export (seção 60) já nascem
    /// espelhando as do projeto em vez do fallback genérico 1920x1080.
    pub(crate) fn start_new_project(&mut self, settings: crate::project::ProjectSettings) {
        let mut project = Project::new(settings);
        project.add_video_track("V1");
        project.add_audio_track("A1");
        self.push_undo();
        self.project = project;
        self.selection.clear();
        self.playhead = Time::ZERO;
        self.playing = false;
        self.preview_key = None;
        self.pending_preview = None;
        self.export_width = settings.width;
        self.export_height = settings.height;
        self.export_fps = settings.fps;
        self.show_welcome = false;
        self.status_message = None;
        // Projeto novo, nunca salvo — próximo "Salvar" precisa perguntar
        // o caminho, não pode reescrever o arquivo do projeto anterior.
        self.current_project_path = None;
    }

    fn open_export_dialog(&mut self) {
        if self.export_job.is_some() {
            return;
        }
        self.export_width = self.project.settings.width;
        self.export_height = self.project.settings.height;
        self.export_fps = self.project.settings.fps;
        self.export_format = crate::export::ExportFormat::Mp4;
        self.show_export_dialog = true;
    }

    fn show_export_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.show_export_dialog {
            return;
        }

        let mut open = self.show_export_dialog;
        let mut do_export = false;
        let mut cancel_clicked = false;

        let max_window_height = (ctx.viewport_rect().height() - 80.0).max(240.0);

        egui::Window::new(tr("export.dialog_title"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .max_height(max_window_height)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(600.0);

                // Conteúdo (presets + detalhes) rola se não couber; a barra
                // de ação embaixo (Exportar/Cancelar) fica *fora* do scroll
                // — sempre visível, nunca cortada pela borda da janela.
                egui::ScrollArea::vertical()
                    .max_height(max_window_height - 90.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            // Coluna dos presets (seção 60 — atalho pra
                            // combinação resolução+fps+qualidade "conhecida"
                            // em vez de montar cada campo na mão toda vez).
                            ui.allocate_ui(egui::vec2(190.0, ui.available_height()), |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(190.0);
                                    ui.label(
                                        RichText::new(tr("export.presets").to_uppercase())
                                            .small()
                                            .color(theme::MUTED)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);

                                    let project_preset = ExportPreset {
                                        name: "export.preset_project",
                                        icon: "📁",
                                        width: self.project.settings.width,
                                        height: self.project.settings.height,
                                        fps: self.project.settings.fps,
                                        quality: crate::export::VideoQuality::Medium,
                                        format: crate::export::ExportFormat::Mp4,
                                    };
                                    self.export_preset_button(ui, &project_preset);
                                    ui.add_space(4.0);
                                    ui.separator();
                                    ui.add_space(4.0);
                                    for preset in EXPORT_PRESETS {
                                        self.export_preset_button(ui, preset);
                                    }
                                });
                            });

                            ui.separator();

                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());

                                theme::collapsing_section(
                                    ui,
                                    "export_resolution",
                                    &tr("export.resolution"),
                                    true,
                                    |ui| {
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::DragValue::new(&mut self.export_width)
                                                    .range(16..=7680),
                                            );
                                            ui.label("×");
                                            ui.add(
                                                egui::DragValue::new(&mut self.export_height)
                                                    .range(16..=4320),
                                            );
                                        });
                                    },
                                );

                                theme::collapsing_section(
                                    ui,
                                    "export_format",
                                    &tr("export.format"),
                                    true,
                                    |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            for format in crate::export::ExportFormat::ALL {
                                                if ui
                                                    .selectable_label(
                                                        self.export_format == format,
                                                        format_label(format),
                                                    )
                                                    .clicked()
                                                {
                                                    self.export_format = format;
                                                }
                                            }
                                        });
                                    },
                                );

                                theme::collapsing_section(
                                    ui,
                                    "export_fps",
                                    &tr("export.fps"),
                                    true,
                                    |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            for fps in [24, 25, 30, 50, 60] {
                                                if ui
                                                    .selectable_label(
                                                        self.export_fps
                                                            == crate::project::Rational::new(
                                                                fps, 1,
                                                            ),
                                                        fps.to_string(),
                                                    )
                                                    .clicked()
                                                {
                                                    self.export_fps =
                                                        crate::project::Rational::new(fps, 1);
                                                }
                                            }
                                        });
                                    },
                                );

                                theme::collapsing_section(
                                    ui,
                                    "export_quality",
                                    &tr("export.quality"),
                                    true,
                                    |ui| {
                                        ui.horizontal(|ui| {
                                            for quality in crate::export::VideoQuality::ALL {
                                                let label = quality_label(quality);
                                                if ui
                                                    .selectable_label(
                                                        self.export_quality == quality,
                                                        label,
                                                    )
                                                    .clicked()
                                                {
                                                    self.export_quality = quality;
                                                }
                                            }
                                        });
                                    },
                                );

                                ui.add_space(8.0);
                                Frame::new()
                                    .fill(theme::ACCENT.gamma_multiply(0.12))
                                    .corner_radius(6)
                                    .inner_margin(Margin::symmetric(10, 8))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "🎬 {}×{} · {} fps",
                                                self.export_width,
                                                self.export_height,
                                                self.export_fps
                                            ))
                                            .strong(),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "⚙ {} (CRF {}) · {} · {}",
                                                quality_label(self.export_quality),
                                                self.export_quality.crf_for(self.export_format),
                                                codec_summary(self.export_format),
                                                self.export_format.extension().to_uppercase(),
                                            ))
                                            .small()
                                            .color(theme::MUTED),
                                        );
                                    });
                            });
                        });
                    });

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let export_label = RichText::new(tr("menu.file.export"))
                        .color(theme::ON_ACCENT)
                        .strong();
                    if ui
                        .add(Button::new(export_label).fill(theme::ACCENT))
                        .clicked()
                    {
                        do_export = true;
                    }
                    if ui.button(tr("common.cancel")).clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        self.show_export_dialog = open && !cancel_clicked;
        if do_export {
            self.show_export_dialog = false;
            self.run_export();
        }
    }

    /// Um botão de preset na coluna esquerda do diálogo de export — vem
    /// destacado (cor de seleção) quando os campos atuais batem exatamente
    /// com ele, igual um `selectable_label`, mas com ícone+nome numa
    /// largura cheia (visual de item de lista, não botão solto).
    fn export_preset_button(&mut self, ui: &mut egui::Ui, preset: &ExportPreset) {
        let active = self.export_width == preset.width
            && self.export_height == preset.height
            && self.export_fps == preset.fps
            && self.export_quality == preset.quality
            && self.export_format == preset.format;

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
            format!("{} {}", preset.icon, tr(preset.name)),
            egui::FontId::proportional(12.0),
            text_color,
        );

        if response.clicked() {
            self.export_width = preset.width;
            self.export_height = preset.height;
            self.export_fps = preset.fps;
            self.export_quality = preset.quality;
            self.export_format = preset.format;
        }
        ui.add_space(2.0);
    }

    /// Abre o `FileDialog` de destino e dispara o export numa thread
    /// separada (seção 65-66 — Progress/Cancel), usando as opções já
    /// confirmadas em `show_export_dialog_window`.
    fn run_export(&mut self) {
        if self.export_job.is_some() {
            return;
        }
        self.playing = false;
        if let Some(engine) = &self.audio_engine {
            engine.stop();
        }
        let ext = self.export_format.extension();
        let Some(output_path) = rfd::FileDialog::new()
            .add_filter(format_label(self.export_format), &[ext])
            .set_file_name(format!("output.{ext}"))
            .save_file()
        else {
            return;
        };
        let output_path = if output_path.extension().and_then(|e| e.to_str()) == Some(ext) {
            output_path
        } else {
            output_path.with_extension(ext)
        };

        let settings = crate::export::ExportSettings {
            output_path,
            width: self.export_width,
            height: self.export_height,
            fps: self.export_fps,
            quality: self.export_quality,
            format: self.export_format,
        };
        let project = self.project.clone();

        let progress = Arc::new(Mutex::new(0.0_f32));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));

        let progress_thread = Arc::clone(&progress);
        let cancel_thread = Arc::clone(&cancel);
        let result_thread = Arc::clone(&result);

        std::thread::spawn(move || {
            let outcome = crate::export::export(&project, &settings, &cancel_thread, |p| {
                *progress_thread.lock().unwrap() = p;
            });
            *result_thread.lock().unwrap() = Some(outcome.map_err(|e| e.to_string()));
        });

        self.export_job = Some(ExportJob {
            progress,
            cancel,
            result,
        });
    }

    /// Overlay de progresso do export (seção 65-66). Chamado todo frame
    /// enquanto `export_job` existe.
    fn show_export_overlay(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.export_job else {
            return;
        };

        let progress = *job.progress.lock().unwrap();
        let finished = job.result.lock().unwrap().is_some();

        let mut close = false;
        let mut cancel_clicked = false;

        egui::Window::new(tr("menu.file.export"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(280.0);
                if finished {
                    let result = job.result.lock().unwrap();
                    match result.as_ref().unwrap() {
                        Ok(()) => {
                            ui.label(format!("✅ {}", tr("export.success")));
                        }
                        Err(e) => {
                            ui.colored_label(
                                theme::ACCENT,
                                tr("export.error").replace("%{error}", e),
                            );
                        }
                    }
                    if ui.button(tr("common.ok")).clicked() {
                        close = true;
                    }
                } else {
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    if ui.button(tr("common.cancel")).clicked() {
                        cancel_clicked = true;
                    }
                }
            });

        if cancel_clicked {
            job.cancel.store(true, Ordering::Relaxed);
        }
        if close {
            self.export_job = None;
        }
        if !finished {
            ctx.request_repaint();
        }
    }

    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        use crate::app::shortcuts::Action;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("menu.file"), |ui| {
                if menu_item(ui, &tr("menu.file.new_project"), "") {
                    self.new_project_settings = self.project.settings;
                    self.show_welcome = true;
                    ui.close();
                }
                ui.separator();
                if menu_item(
                    ui,
                    &tr("common.import_media"),
                    &self.shortcut_label(Action::Import),
                ) {
                    ui::media_panel::import_media(self);
                    ui.close();
                }
                if menu_item(
                    ui,
                    &tr("menu.file.save_project"),
                    &self.shortcut_label(Action::Save),
                ) {
                    self.save_project();
                    ui.close();
                }
                if menu_item(ui, &tr("menu.file.save_project_as"), "") {
                    self.save_project_as_dialog();
                    ui.close();
                }
                if menu_item(
                    ui,
                    &tr("menu.file.open_project"),
                    &self.shortcut_label(Action::Open),
                ) {
                    self.open_project_dialog();
                    ui.close();
                }
                ui.separator();
                if menu_item(
                    ui,
                    &tr("menu.file.export"),
                    &self.shortcut_label(Action::Export),
                ) {
                    self.open_export_dialog();
                    ui.close();
                }
            });
            ui.menu_button(tr("menu.edit"), |ui| {
                if menu_item(ui, &tr("common.undo"), &self.shortcut_label(Action::Undo)) {
                    self.undo();
                    ui.close();
                }
                if menu_item(ui, &tr("common.redo"), &self.shortcut_label(Action::Redo)) {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                menu_static_row(ui, &tr("common.split"), &self.shortcut_label(Action::Split));
                menu_static_row(
                    ui,
                    &tr("common.delete_clip"),
                    &self.shortcut_label(Action::Delete),
                );
                // Diferente de Split/Delete acima (reminders estáticos —
                // Split depende do playhead, e nenhum dos dois virou
                // clicável por escolha original), Duplicate foi pedido
                // explicitamente como item funcional do menu Editar
                // "quando algo estiver selecionado" — daí clicável, mas
                // desabilitado sem seleção em vez de reminder mudo.
                ui.add_enabled_ui(!self.selection.is_empty(), |ui| {
                    if menu_item(
                        ui,
                        &tr("common.duplicate_clip"),
                        &self.shortcut_label(Action::Duplicate),
                    ) {
                        self.duplicate_selected_clips();
                        ui.close();
                    }
                });
                ui.separator();
                if menu_item(ui, &tr("menu.edit.preferences"), "") {
                    self.show_preferences = true;
                    ui.close();
                }
            });
            ui.menu_button(tr("menu.insert"), |ui| {
                if menu_item(ui, &tr("menu.insert.video_track"), "") {
                    self.add_video_track();
                    ui.close();
                }
                if menu_item(ui, &tr("menu.insert.audio_track"), "") {
                    self.add_audio_track();
                    ui.close();
                }
            });
            ui.menu_button(tr("menu.view"), |ui| {
                if ui.button(tr("menu.view.zoom_in")).clicked() {
                    self.timeline_zoom = (self.timeline_zoom * 1.25).min(400.0);
                    ui.close();
                }
                if ui.button(tr("menu.view.zoom_out")).clicked() {
                    self.timeline_zoom = (self.timeline_zoom / 1.25).max(4.0);
                    ui.close();
                }
                ui.separator();
                ui.menu_button(tr("menu.view.language"), |ui| {
                    let current = i18n::current_locale();
                    for (code, label) in i18n::LOCALES {
                        if ui.selectable_label(current == *code, *label).clicked() {
                            i18n::set_locale(code);
                            ui.close();
                        }
                    }
                });
            });

            ui.add_space(ui.available_width().max(0.0) - 40.0);
            ui.weak(env!("CARGO_PKG_VERSION"));
        });
    }
}

impl eframe::App for Application {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(LOCALE_STORAGE_KEY, i18n::current_locale());
        if let Ok(json) = serde_json::to_string(&self.key_bindings) {
            storage.set_string(KEY_BINDINGS_STORAGE_KEY, json);
        }
        if let Ok(json) = serde_json::to_string(&self.recent_projects) {
            storage.set_string(RECENT_PROJECTS_STORAGE_KEY, json);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.sync_window_title(ui.ctx());

        // Tela de boas-vindas (seção 47/60 combinadas: usuário pediu abrir/
        // criar novo/recentes + escolha de template/dimensões antes de
        // qualquer clip existir) substitui o editor inteiro enquanto ativa
        // — nada de atalhos/playback/painéis rodando por trás dela.
        if self.show_welcome {
            ui::welcome::show(ui, self);
            return;
        }

        self.handle_shortcuts(ui.ctx());
        self.sync_audio_playback();

        if self.playing {
            let dt = ui.input(|i| i.stable_dt) as f64;
            self.playhead += Time::from_secs_f64(dt);
        }

        let panel_frame = Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(Margin::symmetric(12, 10));

        egui::Panel::top("menu_bar")
            .frame(
                Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(Margin::symmetric(8, 4)),
            )
            .show_separator_line(true)
            .show(ui, |ui| self.show_menu_bar(ui));

        egui::Panel::top("toolbar")
            .frame(
                Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(Margin::symmetric(10, 8)),
            )
            .show(ui, |ui| ui::toolbar::show(ui, self));

        if let Some(message) = self.status_message.clone() {
            let mut dismiss = false;
            egui::Panel::top("status_bar")
                .frame(
                    Frame::new()
                        .fill(theme::CHROME)
                        .inner_margin(Margin::symmetric(10, 4)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.weak(message);
                        if ui.small_button("✕").clicked() {
                            dismiss = true;
                        }
                    });
                });
            if dismiss {
                self.status_message = None;
            }
        }

        egui::Panel::bottom("timeline")
            .frame(panel_frame)
            .resizable(true)
            .default_size(260.0)
            .min_size(140.0)
            .show(ui, |ui| ui::timeline::show(ui, self));

        egui::Panel::left("media_panel")
            .frame(panel_frame)
            .resizable(true)
            .default_size(240.0)
            .min_size(180.0)
            .show(ui, |ui| ui::media_panel::show(ui, self));

        egui::Panel::right("inspector")
            .frame(panel_frame)
            .resizable(true)
            .default_size(280.0)
            .min_size(220.0)
            .show(ui, |ui| ui::inspector::show(ui, self));

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::CANVAS)
                    .inner_margin(Margin::same(16))
                    .corner_radius(CornerRadius::same(0)),
            )
            .show(ui, |ui| ui::preview::show(ui, self));

        if self.playing || self.pending_preview.is_some() {
            ui.ctx().request_repaint();
        }

        self.show_export_dialog_window(ui.ctx());
        self.show_export_overlay(ui.ctx());
        ui::preferences::show(self, ui.ctx());
        ui::pan_crop::show(self, ui.ctx());
    }
}

/// Largura mínima de um item de menu com atalho — sem isso, o texto do
/// atalho (`Ctrl+S`, `Ctrl+Shift+Z`, ...) fica colado ao rótulo em itens
/// curtos e desalinhado entre itens de tamanhos diferentes (usuário
/// relatou: "os menus... padronizar o atalho sempre o maximo a direita,
/// ele fica desalinhado"). Um valor fixo generoso — em vez de medir o
/// texto mais largo de cada menu — porque o problema original era o
/// `\t` do `egui` avançando pro próximo tab-stop da fonte (uma posição
/// fixa que ignora o layout do menu), não a largura do popup em si.
/// **170.0** — 230.0 e depois 200.0 ainda deixavam largo demais pra
/// idiomas com texto curto (inglês); usuário pediu duas vezes pra
/// diminuir ("pode diminuir um pouco a largura"; depois "diminua mais a
/// largura do menu"). Aumentar aqui só se um rótulo muito comprido
/// (locale mais verboso, ex: alemão) algum dia estourar essa largura.
const MENU_ITEM_MIN_WIDTH: f32 = 170.0;

/// Uma linha de menu com o rótulo à esquerda e o atalho (se houver)
/// sempre colado na borda direita, não importa o tamanho do rótulo —
/// substitui o truque antigo de `format!("{label}\t{shortcut}")` num
/// `ui.button()` normal, que dependia do tab-stop da fonte (posição fixa)
/// em vez da largura real do item, e por isso desalinhava. Mesmo idioma
/// visual de "linha com texto secundário na direita" já usado em
/// `export_preset_button`/`ui/welcome.rs::template_button` (pintura
/// manual num retângulo alocado, não um widget de texto único).
fn menu_row(ui: &mut egui::Ui, label: &str, shortcut: &str, interactive: bool) -> bool {
    // Largura fixa, não `ui.available_width().max(...)` — um popup de
    // menu recém-aberto reporta uma largura disponível bem maior que o
    // conteúdo real (do tamanho da janela toda, não do menu), então usar
    // isso como piso deixava CADA linha gigante em vez de só garantir um
    // mínimo. Usuário relatou: "file edit e insert continuam gigantes do
    // mesmo jeito, só esta certo o ultimo" — "o último" (Exibir) nunca
    // passou por `menu_row`, só os botões normais do `egui`, que aí sim
    // encolhem pro tamanho do texto.
    let width = MENU_ITEM_MIN_WIDTH;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::click());
    let clicked = interactive && response.clicked();

    if interactive {
        let hover_t = theme::animate(ui, response.id, response.hovered());
        if hover_t > 0.0 {
            ui.painter()
                .rect_filled(rect, 2.0, theme::MUTED.gamma_multiply(0.18 * hover_t));
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    let text_color = if interactive {
        ui.visuals().text_color()
    } else {
        theme::MUTED
    };
    // 12.0/10.0 pra bater com o resto do app — `theme::apply` define
    // `Body`/`Button` como 12.0 (ver `ui/theme.rs`), e itens com texto
    // secundário à direita (`export_preset_button`, `welcome.rs::
    // template_button`) já usam 12.0/10.0 pro mesmo par label+detalhe.
    // Um valor diferente aqui (13.0/11.0) fazia o menu parecer com fonte
    // maior que o resto — usuário relatou "os menus parecem com a fonte
    // maior que o resto".
    ui.painter().text(
        rect.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_color,
    );
    if !shortcut.is_empty() {
        ui.painter().text(
            rect.right_center() - egui::vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            shortcut,
            egui::FontId::proportional(10.0),
            theme::MUTED,
        );
    }
    clicked
}

/// Item clicável (a maioria dos itens de menu).
fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
    menu_row(ui, label, shortcut, true)
}

/// Linha informativa, não clicável — usada pro Split/Delete do menu
/// Editar, que só existem ali como lembrete do atalho (a ação de verdade
/// depende de uma seleção que o menu não tem como disparar sozinho).
fn menu_static_row(ui: &mut egui::Ui, label: &str, shortcut: &str) {
    menu_row(ui, label, shortcut, false);
}

struct ExportPreset {
    /// Chave de locale (não texto direto — precisa passar por `tr()`).
    name: &'static str,
    icon: &'static str,
    width: u32,
    height: u32,
    fps: crate::project::Rational,
    quality: crate::export::VideoQuality,
    format: crate::export::ExportFormat,
}

const EXPORT_PRESETS: &[ExportPreset] = &[
    ExportPreset {
        name: "export.preset_youtube_4k",
        icon: "▶",
        width: 3840,
        height: 2160,
        fps: crate::project::Rational::new(30, 1),
        quality: crate::export::VideoQuality::High,
        format: crate::export::ExportFormat::Mp4,
    },
    ExportPreset {
        name: "export.preset_youtube_1080p",
        icon: "▶",
        width: 1920,
        height: 1080,
        fps: crate::project::Rational::new(30, 1),
        quality: crate::export::VideoQuality::High,
        format: crate::export::ExportFormat::Mp4,
    },
    ExportPreset {
        name: "export.preset_youtube_720p",
        icon: "▶",
        width: 1280,
        height: 720,
        fps: crate::project::Rational::new(30, 1),
        quality: crate::export::VideoQuality::Medium,
        format: crate::export::ExportFormat::Mp4,
    },
    ExportPreset {
        name: "export.preset_social_vertical",
        icon: "📱",
        width: 1080,
        height: 1920,
        fps: crate::project::Rational::new(30, 1),
        quality: crate::export::VideoQuality::High,
        format: crate::export::ExportFormat::Mp4,
    },
    ExportPreset {
        name: "export.preset_web_compact",
        icon: "🌐",
        width: 1280,
        height: 720,
        fps: crate::project::Rational::new(30, 1),
        quality: crate::export::VideoQuality::Low,
        format: crate::export::ExportFormat::WebM,
    },
];

fn quality_label(quality: crate::export::VideoQuality) -> String {
    use crate::export::VideoQuality;
    match quality {
        VideoQuality::Low => tr("export.quality_low"),
        VideoQuality::Medium => tr("export.quality_medium"),
        VideoQuality::High => tr("export.quality_high"),
        VideoQuality::VeryHigh => tr("export.quality_veryhigh"),
    }
}

fn format_label(format: crate::export::ExportFormat) -> String {
    use crate::export::ExportFormat;
    match format {
        ExportFormat::Mp4 => "MP4".to_string(),
        ExportFormat::Mov => "MOV".to_string(),
        ExportFormat::Mkv => "MKV".to_string(),
        ExportFormat::WebM => "WebM".to_string(),
    }
}

fn codec_summary(format: crate::export::ExportFormat) -> &'static str {
    use crate::export::ExportFormat;
    match format {
        ExportFormat::Mp4 | ExportFormat::Mov | ExportFormat::Mkv => "H.264 + AAC",
        ExportFormat::WebM => "VP9 + Opus",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn track_recent_in_moves_an_existing_entry_to_the_front_instead_of_duplicating() {
        let mut list = vec![PathBuf::from("a.myeditor"), PathBuf::from("b.myeditor")];
        track_recent_in(&mut list, std::path::Path::new("b.myeditor"));
        assert_eq!(
            list,
            vec![PathBuf::from("b.myeditor"), PathBuf::from("a.myeditor")]
        );
    }

    #[test]
    fn track_recent_in_never_grows_past_the_max() {
        let mut list = Vec::new();
        for i in 0..(RECENT_PROJECTS_MAX + 3) {
            track_recent_in(&mut list, &PathBuf::from(format!("{i}.myeditor")));
        }
        assert_eq!(list.len(), RECENT_PROJECTS_MAX);
        // O mais recente (último inserido) continua no início.
        assert_eq!(
            list[0],
            PathBuf::from(format!("{}.myeditor", RECENT_PROJECTS_MAX + 2))
        );
    }
}
