# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```text
cargo build                              # build
cargo run                                # launch the app (opens desktop window)
cargo fmt                                # format (rustfmt.toml pins edition = 2024)
cargo clippy --all-targets               # lint
cargo test                               # run all tests
cargo test --test timeline_tests         # run one integration test file (tests/*.rs)
cargo test some_test_name                # run tests matching a name
```

Every task must leave `cargo fmt`, `cargo clippy --all-targets`, `cargo test`, and `cargo build` clean — this is the completion criterion for any change (spec §106/§108).

## Current state

Fases 0-20 are done and wired end-to-end (real UI, real FFmpeg process calls, not just data models) except Fase 17 (GPU), deliberately skipped per spec §102 ("só depois do pipeline CPU funcionar").

**FFmpeg**: provided by the `ffmpeg-sidecar` crate, not system libs. `main.rs` spawns a background thread that runs `ffmpeg_sidecar::download::auto_download()` at startup — downloads a static `ffmpeg`/`ffprobe` next to the executable only if not already found (fast no-op after the first run); don't remove this call without replacing it. Chosen over `ffmpeg-next`/FFI bindings because there's no system `ffmpeg`/dev libs available in the target environment. Consequence: no frame-level Rust access to decoded media — decoding is "spawn ffmpeg, read stdout", export is "hand ffmpeg a `-filter_complex` graph and let it composite+encode". This shapes the whole media/export architecture; don't bolt on FFI-style frame access without reconsidering it.

**`src/project/`**: `Project` (settings/media/tracks/`generated: HashMap<MediaId, GeneratedMediaEntry>`), `Track` (video or audio, `volume`/`pan`/`muted`/`clips`), `Clip` (shared struct for both video and audio clips — `opacity`/`volume`/`fade_in`/`fade_out`/`reversed`/source-time-mapping fields), `Selection`, `serialization.rs` (JSON via serde, `_version` checked on load, `missing_media` detection). A `Clip::source` `MediaId` can resolve to either `project.media` (real files) or `project.generated` (Solid Color / Audio Visualizer) — every consumer checks `generated` first. `Clip::source_time_at` is the Source Time Mapping core (spec §25/§78) that Trim/Split/Reverse/Fade all share — normal mapping is `source_start + local_time`, reverse is `source_end - local_time`. `Clip::transform: Transform` (spec §19 — `position_x/y`, `scale_x/y`, `rotation`) now has a real consumer: Pan/Crop (`ui/pan_crop.rs`, user-requested "modal de crop/panorâmica igual ao Vegas") — `scale` is zoom factor (1.0 = full frame), `position` is pan offset, both as fractions of the **original source**, not of any pre-cropped/pre-fit version of it. `Transform::crop_filter_expr()` builds the ffmpeg `crop=iw/scale:ih/scale:...` expression (`iw`/`ih` = ffmpeg's own vars, whatever feeds the filter) shared by preview decode and export — when set, it *replaces* the default crop-fit stage entirely rather than stacking after it. **A first version stacked them** (crop-fit first, Pan/Crop second, operating on the already-cropped image) — user-reported real bug: a 9:16 photo in a 16:9 project only ever showed the center strip the crop-fit had already kept, so Pan/Crop could never reach the parts it had discarded ("não tenho a imagem inteira pra definir o que eu quero"). Fixed by making the two mutually exclusive (`is_identity()` picks which one runs) and giving the dialog its own decode path, `decoder::decode_frame_letterboxed` (`scale=...:force_original_aspect_ratio=decrease,pad=...`), which shows the **entire** undistorted source (black bars, not crop) so every part of it is reachable. The dialog computes where the real image sits inside that letterbox (`ui/pan_crop.rs::letterbox_inner`, needs the source's real pixel dimensions from `Media.metadata.width/height`) and locks the drag rectangle's aspect ratio to a *combined* source+project ratio (not just the project's) so a resize never distorts the final crop regardless of how different the two aspect ratios are. **The dialog's canvas size is computed from the source's own aspect ratio** (up to a `MAX_CANVAS` cap), not a fixed box — a fixed generic-aspect canvas left a mismatched-aspect source (e.g. 9:16 in a wide dialog) occupying only a thin strip in the middle, user-reported as "a imagem... parece bem menor que a original... dificulta posicionar". A small always-visible icon (⛶) in each video clip's corner on the timeline (`ui/timeline.rs`, matching Vegas's own Pan/Crop clip icon) opens the same dialog — hit-tested against `response.clicked()`'s position rather than a separate `Sense` region, so it can't fight the clip's existing drag/fade/level hit-testing for the same pixels. **The dialog window itself then got too tall to use** — user-reported "modal ficou muito grande não consigo clicar nos botões" — because the canvas-sizing fix above can reach up to `MAX_CANVAS`'s 620px height on its own, and stacked with the hint label/zoom slider/button row that easily exceeded a real viewport, pushing Aplicar/Redefinir/Cancelar off-screen. Same bug class, same fix, as the Export dialog and Preferences dialog had earlier: `let max_window_height = (ctx.viewport_rect().height() - 80.0).max(240.0);` capping the `egui::Window`, everything except the final button row wrapped in `egui::ScrollArea::vertical().max_height(max_window_height - 90.0).auto_shrink([false, true])`, button row kept outside the scroll area so it's always visible regardless of canvas size. Also trimmed `MAX_CANVAS`'s height cap 620→520 to leave more headroom before that scroll kicks in.

**`src/media/`**: `metadata::probe` runs `ffmpeg -i <file> -f null -` and parses stderr (via `ffmpeg_sidecar::metadata::FfmpegMetadata`, no `ffprobe -print_format json`). Still images get a special-cased default duration (5s, since ffmpeg reports `Duration: N/A` for stills) and skip `-ss` entirely when decoding (a still has no time axis). `decoder::decode_frame_at_scaled` spawns a fresh `ffmpeg` process **per call**, no persistent decoder/cache — this is why playback isn't real-time. It scales with `force_original_aspect_ratio=increase,crop=WxH` (Vegas-style "Crop" fit), not a plain `scale=WxH` — a plain scale stretches/distorts any source whose aspect ratio doesn't match the project. That default crop-fit and a clip's Pan/Crop `Transform` are mutually exclusive, never stacked — see the `src/project/` entry above for why. `mod.rs` holds the extension allowlists (`VIDEO_EXTENSIONS`/`AUDIO_EXTENSIONS`/`IMAGE_EXTENSIONS`) used both by the file picker's filter and a defensive re-check after picking (never trust the OS picker blindly).

**`src/app/application.rs`**: `Application` is the app state — `project`, `selection`, `playhead`, `playing`, `timeline_zoom`, undo/redo (`Vec<Project>` snapshot clones on `push_undo()`, not a `Command` trait — simpler, cheap because `Project` never holds decoded media; revisit if `Project` ever grows something expensive to clone), export dialog state (`ExportSettings`-mirroring fields + `ExportJob` running in a background thread), `key_bindings`/`show_preferences`/`recording_shortcut` (see `app/shortcuts.rs`), `pending_preview` (see preview below). `app/commands.rs` holds the mutating operations (split/trim/move/delete/reverse, plus per-clip `set_clip_fade_in/out`/`set_clip_opacity`/`set_clip_volume` used by the timeline's draggable facilitators) as `impl Application` methods. `show_welcome: bool` (default `true` on boot) gates `Application::ui()` — while set, `ui()` renders only `ui::welcome::show` and returns, skipping shortcuts/audio-sync/panels entirely. User-requested: "necessario uma tela de boas vindas welcome antes da aplicação, para abrir, criar novo, recentes e nela definir qual template / dimensões para facilitar o crop e o projeto ja ficar coerente com a exportação" — the dialog (`ui/welcome.rs`) has a New Project column (template buttons + custom width/height/fps, writing into `new_project_settings: ProjectSettings` until "Criar Projeto" confirms via `Application::start_new_project`) and an Open/Recent column (`recent_projects: Vec<PathBuf>`, persisted via `eframe::Storage` like `key_bindings` — app preference, not project data). `start_new_project` mirrors the chosen `width`/`height`/`fps` into `export_width`/`export_height`/`export_fps` at creation time — that's the actual fix for "coerente com a exportação": the project (and therefore the default crop-fit every source gets scaled through, see the Pan/Crop entry below) is born at the resolution/aspect the user picked instead of always starting at the generic 1920x1080 default and drifting from whatever the user later sets for export. `open_project_dialog`/`save_project_as_dialog` and the welcome screen's recent-list clicks all funnel through `open_project_from_path`, which also updates `recent_projects` (dedup + move-to-front + cap at `RECENT_PROJECTS_MAX`, pure logic factored into `track_recent_in` so it's unit-testable without a real `eframe::CreationContext`); a recent entry whose file no longer resolves is dropped from the list on the failed attempt rather than kept around to fail again. File menu gained "Novo Projeto..." to reopen the welcome screen without restarting the app. **"Salvar" always meant "Salvar Como..."** — every save, even the second one on an already-saved project, opened the OS file picker again. User-reported: "salvar projeto sempre é 'salvar como...' tem que ter uma opção só de salvar quando o arquivo ja existe". Fixed with `current_project_path: Option<PathBuf>` (private, set by `open_project_from_path` and by any successful save, cleared by `start_new_project` since a fresh project has no file yet): `save_project` (bound to Ctrl+S / the "Salvar Projeto" menu item) writes straight to that path with no dialog when it's `Some`, and only falls back to the picker (`save_project_as_dialog`, also reachable directly via the new "Salvar Como..." menu item) when the project has never been saved. Both dialogs funnel through `write_project_to`, the one place that actually calls `serialization::save_project` and updates `current_project_path`/`recent_projects` on success.

**Menu bar gained an "Inserir" menu** (video/audio track, user-requested) sitting next to Arquivo/Editar/Exibir — its two actions (`Application::add_video_track`/`add_audio_track`, new `app/commands.rs` methods) just wrap `push_undo` + `project.next_{video,audio}_track_name` + `project.add_{video,audio}_track`, replacing what used to be duplicated inline in the timeline's "➕🎬"/"➕🔊" header buttons (those now call the same two methods). **Menu item shortcut hints were misaligned** — every item built its label as `format!("{label}\t{shortcut}")` inside a plain `ui.button()`, and egui's `\t` advances to the font's next fixed tab-stop rather than the menu's actual right edge, so the shortcut column drifted per item depending on label length. User-reported: "padronizar o atalho sempre o maximo a direita, ele fica desalinhado... se o texto for muito grande pode aumentar um pouco a largura". Fixed with `menu_row`/`menu_item`/`menu_static_row` (`app/application.rs`) — same manually-painted-row idiom already used by `export_preset_button`/`ui/welcome.rs::template_button` (allocate a fixed-width rect, paint label at `LEFT_CENTER`, shortcut at `RIGHT_CENTER`) instead of a single text string, so the shortcut is pinned to the row's real right edge regardless of label length. Row width is `ui.available_width().max(MENU_ITEM_MIN_WIDTH)` — `MENU_ITEM_MIN_WIDTH` is a fixed constant rather than a per-menu measured max, picked to comfortably fit this app's actual menu items; bump it if a future item (longer label, more verbose locale) overflows it. `menu_static_row` (no click sense, muted text) replaced the Split/Delete informational rows in the Edit menu, which were never real buttons — they're a shortcut reminder, not wired to an action, since both need a clip selection the menu itself can't provide. **First value (230.0) was itself too wide** for shorter-text locales (English) — user-reported "pode diminuir um pouco a largura do context ficou muito largo para idiomas com texto menor" — dropped to 200.0.

**A second, unrelated export bug reported in the same message**: "verifique a exportação de multiplas tracks a v1 e v2 estão certas no editor uma começando zero e outra começando 10, mas na exportação ambas começam no segundo zero" — real, reproduced with two synthetic clips (red @0s, blue @10s on separate tracks) and verified via ffprobe pixel sampling at multiple timestamps. Root cause: within a single track, multiple clips (or a lone clip with `timeline_start > 0`) get joined with ffmpeg's `concat` filter, which only knows how to butt streams together in sequence — it has zero notion of each clip's actual position on the absolute timeline. A track's first (or only) clip starting at 10s therefore played from 0s in the exported file regardless of its `timeline_start`, and any gap between two clips on the same track collapsed to nothing (the *existing* "gaps aren't filled" limitation was actually this same root cause, just for the between-clips case — a leading gap before the first/only clip was the sharper, previously-undocumented instance of it). Fixed by `gap_before_each_clip` (pure, unit-tested): walks a track's already-`sorted_clips` clips with a running `cursor`, and for any clip whose `timeline_start` is past the cursor, returns the gap duration that needs a filler segment *before* that clip enters the `concat` chain — video fillers are a `color=black` lavfi source immediately zeroed to full transparency (`format=yuva420p,colorchannelmixer=aa=0`, reusing `add_black_background`'s lavfi source since only the alpha differs) so the real background/lower tracks show through during the gap instead of black; audio fillers are `add_silent_audio` (already existed for the Audio Visualizer's no-real-audio case, now reused here too). Both the video and audio per-track loops call the same helper.

**`app/shortcuts.rs`**: configurable keyboard shortcuts. `Action` (10 editor actions) → `Shortcut { key, ctrl, shift, alt }` (exact-modifier match) → `KeyBindings = HashMap<Action, Shortcut>`. Presets (`Preset::{Default,Vegas,Premiere,CapCut}`) only override bindings actually confirmed via each app's real documented shortcuts (a test locks this — a preset can't silently gain an unsourced override). Persisted via `eframe::Storage` (JSON), separate from the project file — it's a user/app preference, not project data, so it's not part of undo/redo or save/load.

**Playback is not real-time**: `ui/preview.rs` decodes a frame (subprocess spawn) whenever the resolved `(MediaId, source_time, alpha)` key changes. Composition (multi-track alpha blending, bottom-to-top, RGBA so generated layers can have per-pixel transparency) now runs on a **background thread** (`dispatch_composite`, `std::sync::mpsc` channel) instead of the UI thread, so multi-layer decode no longer freezes the whole interface — but it never dispatches a new composite while one is already in flight (a coalescing bug here once froze playback entirely: don't "replace" an in-flight request by key, always wait for it to finish first). Frame-stepping is exact; continuous playback smoothness is still bounded by how fast `ffmpeg` can be spawned repeatedly (a persistent decoder would fix this for real, not done).

**Audio playback**: via `rodio` (`audio/playback.rs`, `None` if no output device, never a crash). `audio/mixer.rs::build_playback_buffer` decodes overlapping clips per non-muted audio track, applies reverse/fade/volume/track-`pan` (balance, not true mono→stereo panning — attenuates the opposite channel), sums tracks, clamps to `[-1,1]`. Built and handed to the player once per play/pause rising edge — scrubbing/editing while already playing does not resync until pause/play again. `audio/analyzer.rs` does the real FFT (pure-Rust `rustfft`) behind the Audio Visualizer generator, log-spaced bands, both in preview and in export (via ffmpeg's own `showfreqs`).

**`rodio`/`cpal` need ALSA dev headers at *build* time on Linux** — vendored into `vendor/alsa/` (not machine-dependent), `.cargo/config.toml` points `PKG_CONFIG_PATH` at it via a relative path. Only active on Linux. If ALSA linking is ever dropped, delete `vendor/alsa/` and that config line together.

**`src/export/exporter.rs`**: builds an ffmpeg `-filter_complex` graph — every non-muted video track becomes its own trim+reverse+fade+opacity+scale chain (same crop-fit scaling as the preview decoder, not a plain stretch), tracks `overlay` bottom-to-top; every non-muted audio track becomes its own volume+pan chain, mixed via `amix`. `VideoQuality` maps to CRF (constant-quality mode). `ExportFormat::{Mp4,Mov,Mkv,WebM}` — MP4/MOV/MKV share H.264+AAC (only the container differs), WebM is VP9+Opus (needs `-b:v 0` alongside `-crf` for true constant-quality mode, and a different CRF scale than x264's — `VideoQuality::crf_for(format)` handles that). Audio Visualizer clips render via ffmpeg's own `showfreqs`, not a flat-color approximation. Timeline gaps (a clip starting after `Time::ZERO`, or two non-touching clips on one track) ARE filled with a transparent/silent filler before `concat` — see `gap_before_each_clip` further down; this used to be a known limitation, now fixed. On a failed export, `export()`'s event loop used to only read `FfmpegEvent::Progress` and threw away every `Log`/`Error` event — the failure message the user actually saw was the hardcoded "ffmpeg encerrou com erro — ver stderr", with the real stderr never captured anywhere. **User reported exactly that useless message back verbatim** ("ao exportar: ffmpeg encerrou com erro - ver stderr"). Fixed by keeping the last 8 `Log`/`Error` lines in a ring buffer during the loop and joining them into `EditorError::ExportError` on failure — the actual ffmpeg reason (unknown encoder, bad filter, permission error, ...) now reaches `export.error`/the status bar instead of being silently discarded. That log tail immediately surfaced the user's *real* export failure: `Output ... same as Input ... - exiting` — the chosen output path was the same file as one of the project's imported media (reimporting a previous export back into the Media Browser, then exporting to that same path again). `export()` now checks this upfront, before spawning `ffmpeg` at all: `paths_refer_to_the_same_file` (canonicalize when both sides exist, falls back to direct `Path` comparison when the output doesn't exist yet, which it usually doesn't) compares `settings.output_path` against every `project.media` path and returns a clear `ExportError` naming the colliding file instead of letting `ffmpeg`'s generic "Invalid argument" reach the user. Next real failure the same fix surfaced: `Error reinitializing filters!` / `Could not open encoder before EOF` (both code -22/EINVAL) — `libx264`/`libvpx-vp9` require even width/height (`yuv420p` chroma subsampling), and neither the Export dialog's nor the welcome screen's width/height `DragValue` stop the user from typing an odd number. Fixed at the one place all export width/height ultimately flows through regardless of where it came from (UI, preset, or an old project file loaded from disk): `export()` rebinds its `settings` to a normalized copy (`even_dim`, rounds an odd dimension down, never up) whenever either dimension is odd, before anything else in the function reads `settings.width`/`settings.height`. Even after that fix the *same* stderr (`Error reinitializing filters!` / `Could not open encoder before EOF`) still reproduced with 3 media of differing aspect ratios on one track (one matching the project, one image, one video adjusted with Pan/Crop) — user reported it again verbatim. Real cause this time, found by capturing the actual `-filter_complex` string and running it through real `ffmpeg` directly (bypassing the 8-line stderr tail, which was truncating the one line that mattered): `Input link ... parameters (size 1920x1080, SAR 10239:10240) do not match ... (1920x1080, SAR 1:1)` — `scale=...:force_original_aspect_ratio=increase` leaves a residual, non-1:1 SAR (sample aspect ratio) whenever the source's aspect ratio doesn't exactly match the project's, and neither `scale`+`crop` nor Pan/Crop's own `scale` reset it back to square pixels afterward. `concat`/`overlay` both require *identical* SAR across every input, so any track mixing sources of different aspect ratios (the totally ordinary case: a correctly-sized clip next to a portrait photo next to a Pan/Crop'd clip) got a different residual SAR per clip and ffmpeg refused to reinitialize the filter graph. Fixed with `,setsar=1` appended right after the scale/crop stage in `video_clip_chain` (both the default-crop-fit and the Pan/Crop branch) — forces square pixels back before `format=yuva420p`, so every clip's output SAR is identical regardless of its source's aspect ratio. Once export actually finished, the next real bug the user hit had nothing to do with `ffmpeg` erroring at all: "a opacidade e o fadein fadeout não estão exportando". Root cause was the video-track compositing assembly (§43's `Background → V1 → V2 → ... → Final Frame` pipeline) — `final_video`'s loop used `track_video_labels[0]` directly as the `overlay` base and only looped `.skip(1)` over the rest, so the bottom track (`V1`, the common case: a project with a single video track) was never itself the *layer* of an `overlay` — nothing was ever composited under it, and its `yuva420p` alpha channel (carrying fade/opacity) got silently discarded by the final `format=yuv420p`. `ui/preview.rs::composite` never had this bug — it always initializes its buffer to black and alpha-blends *every* layer including the bottom one against it, which is why the same fades/opacity looked correct in preview and only broke on export. Fixed by giving export the same explicit black background preview already has: `FilterGraph::add_black_background` (a `color=c=black:s=WxH:d=<total timeline duration>:r=<fps>` lavfi input) is now the base of the *first* overlay, and every track in `track_video_labels` (not `.skip(1)`) is composited as a layer on top of it — extracted into `compose_final_video` so the assembly is unit-testable without spawning `ffmpeg`. Verified with a real export + ffprobe pixel sampling at three timestamps (start of fade, mid-fade, post-fade-at-opacity-plateau) showing the expected progressive brightness ramp instead of the flat full-brightness output the bug produced.

**`src/ui/timeline.rs`**: custom-drawn timeline (no built-in widget). Clip drag has multiple modes (`DragMode::{TrimLeft,TrimRight,Move,FadeIn,FadeOut,Level}`) decided by `hit_test_drag_mode` from where the drag *started* (`press_origin`, not the current pointer position — using the current position let the pointer drift outside the small hit-test zone before egui recognizes a drag, breaking the fade handles). Snapping, adaptive ruler tick spacing, Ctrl+scroll zoom (`ctx.input(|i| i.zoom_delta())`, cursor-anchored), waveform cache, drag-and-drop from the media panel (including auto-creating a track when none of the right type exist yet). `HEADER_WIDTH`/`TRACK_HEIGHT`/`AUDIO_TRACK_HEIGHT` constants and `time_to_x`/`x_to_time` (pure, no header offset — the ruler adds `HEADER_WIDTH` itself at its own click site) are the pixel-math foundation everything else assumes.

**i18n**: every UI string goes through `crate::i18n::tr("key")`. Translations in `locales/app.yml` (`rust-i18n` v4, `_version: 2`, one entry per key with all 10 locales inline: en/pt-BR/es/zh-CN/ja/hi/id/fr/it/de). `rust_i18n::i18n!("locales", ...)` **must** stay in `main.rs` (crate root) — the generated `t!` macro only resolves from there. `i18n::tr()` doesn't support `%{arg}` interpolation directly — call sites do `.replace("%{name}", &value)`. Startup locale: last saved (`eframe` storage) → OS locale → `en` fallback. CJK/Devanagari fonts are loaded from common system paths at runtime (not vendored — a machine without them degrades to tofu for those locales only).

**`eframe`/`egui` are pinned to 0.35.0** (not newer — needs a Rust toolchain this project doesn't have). API differs from newer docs/training data: `eframe::App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`, not `update(&mut self, ctx, ...)`. `TopBottomPanel`/`SidePanel` are unified into `egui::Panel` (`Panel::top/bottom/left/right(id)`), both taking `.show(ui, ...)` not `.show(ctx, ...)`; `CentralPanel` is still separate and must be shown last. Menu bar is `egui::MenuBar::new().ui(ui, |ui| {...})`. When in doubt, check the installed crate's source under `~/.cargo/registry/src/.../egui-0.35.0/`.

Track type labels (V1/V2/A1/A2), timecodes, and keyboard shortcut hints are deliberately left untranslated (universal NLE convention).

## Architecture

The rest of this file (§1 onward) is the full technical spec: philosophy, module responsibilities, the Project/Timeline/Clip data model, the Source Time Mapping design that Trim/Split/Reverse/Speed/Fade all share (§78, §117 — this is the core to preserve when extending playback/export), the video and audio pipelines (§80-81), and the phase-by-phase roadmap (§85-105) with the agent task contract (§106-110). Read it before making structural changes — don't restate it here.

# Editor de Vídeo em Rust

## Especificação Técnica, Arquitetura e Roadmap de Desenvolvimento

**Versão:** 1.0
**Status:** Especificação inicial
**Objetivo:** Criar um editor de vídeo desktop simples, modular e extensível, desenvolvido prioritariamente em Rust.

---

# 1. Objetivo

Criar um editor de vídeo desktop simples, escrito principalmente em **Rust**, com uma interface de edição baseada em timeline.

O aplicativo deverá permitir:

* Importar vídeos.
* Importar arquivos de áudio.
* Visualizar mídia em um player.
* Trabalhar com uma timeline baseada em tempo.
* Separar faixas de vídeo e áudio.
* Criar múltiplas faixas.
* Cortar clips de vídeo e áudio.
* Fazer trim de clips.
* Mover clips na timeline.
* Fazer fade-in em vídeo.
* Fazer fade-out em vídeo.
* Fazer fade-in em áudio.
* Fazer fade-out em áudio.
* Controlar o volume das faixas de áudio.
* Mutar faixas.
* Inverter clips de vídeo.
* Inverter clips de áudio.
* Criar uma camada de cor sólida.
* Criar um visualizador de áudio/equalizador.
* Pré-visualizar o resultado.
* Salvar projetos.
* Carregar projetos.
* Desfazer/refazer operações.
* Exportar o resultado para MP4.

O projeto deve começar pequeno, mas possuir arquitetura suficiente para futuramente receber:

* Texto.
* Transições.
* Filtros.
* Transformações.
* Color grading.
* Efeitos de áudio.
* GPU rendering.
* Proxy media.
* Mais codecs.
* Mais formatos de exportação.

---

# 2. Filosofia do projeto

O projeto deverá seguir os seguintes princípios.

## 2.1 Rust first

A lógica principal do editor deverá ser implementada em Rust.

Não criar uma aplicação onde a lógica principal fique em JavaScript, Python ou outra linguagem.

---

## 2.2 Arquitetura modular

Separar:

```text
UI
Project
Timeline
Media
Video
Audio
Effects
Renderer
Export
```

A UI não deve conhecer detalhes internos do FFmpeg.

O FFmpeg não deve controlar a lógica da timeline.

O renderer não deve depender diretamente dos widgets da UI.

---

## 2.3 Não destrutivo

A edição deverá ser não destrutiva.

Se o usuário cortar:

```text
[================ VIDEO ================]
```

em:

```text
[========][================]
```

o arquivo original não deve ser modificado.

Os clips continuam apontando para a mesma mídia original.

---

## 2.4 Preview e Export são sistemas diferentes

O preview precisa ser rápido.

O export precisa ser preciso.

Portanto:

```text
Preview Renderer
```

e:

```text
Offline Export Renderer
```

podem compartilhar o mesmo modelo de timeline, mas não precisam utilizar exatamente o mesmo pipeline de renderização.

---

# 3. Stack tecnológica

## 3.1 Linguagem

Utilizar:

```text
Rust Stable
```

---

# 4. GUI

A recomendação inicial é:

```text
egui
eframe
```

Motivos:

* Integração direta com Rust.
* Desenvolvimento rápido.
* Bom para ferramentas desktop.
* Fácil criação de widgets customizados.
* Adequado para uma timeline customizada.

Alternativas que podem ser avaliadas posteriormente:

```text
iced
Slint
Tauri
```

Porém, o MVP deverá preferencialmente utilizar:

```text
Rust + egui + eframe
```

---

# 5. Media Backend

Utilizar **FFmpeg** como backend principal para mídia.

O FFmpeg poderá ser responsável por:

* Decodificação de vídeo.
* Decodificação de áudio.
* Encoding.
* Muxing.
* Conversão.
* H.264.
* AAC.
* MP4.
* Outros codecs suportados.

Não implementar um decoder H.264/H.265 do zero.

---

# 6. GPU

A arquitetura deverá permitir futuramente utilizar:

```text
wgpu
```

para:

* Composição de vídeo.
* Alpha blending.
* Escala.
* Transformações.
* Visualizador de áudio.
* Efeitos.
* Shaders.

Não é obrigatório implementar GPU rendering no primeiro estágio.

Primeiro fazer o pipeline funcionar corretamente em CPU.

---

# 7. Estrutura de diretórios

Estrutura inicial recomendada:

```text
editor/
│
├── Cargo.toml
├── README.md
│
├── src/
│   ├── main.rs
│   │
│   ├── app/
│   │   ├── mod.rs
│   │   ├── application.rs
│   │   └── commands.rs
│   │
│   ├── project/
│   │   ├── mod.rs
│   │   ├── project.rs
│   │   ├── timeline.rs
│   │   ├── track.rs
│   │   ├── clip.rs
│   │   ├── media.rs
│   │   └── serialization.rs
│   │
│   ├── media/
│   │   ├── mod.rs
│   │   ├── decoder.rs
│   │   ├── video_decoder.rs
│   │   ├── audio_decoder.rs
│   │   ├── metadata.rs
│   │   └── cache.rs
│   │
│   ├── video/
│   │   ├── mod.rs
│   │   ├── frame.rs
│   │   ├── compositor.rs
│   │   ├── renderer.rs
│   │   └── playback.rs
│   │
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── mixer.rs
│   │   ├── analyzer.rs
│   │   ├── waveform.rs
│   │   └── playback.rs
│   │
│   ├── effects/
│   │   ├── mod.rs
│   │   ├── fade.rs
│   │   ├── solid_color.rs
│   │   └── audio_visualizer.rs
│   │
│   ├── export/
│   │   ├── mod.rs
│   │   ├── exporter.rs
│   │   ├── renderer.rs
│   │   └── ffmpeg.rs
│   │
│   └── ui/
│       ├── mod.rs
│       ├── timeline.rs
│       ├── preview.rs
│       ├── media_panel.rs
│       ├── inspector.rs
│       ├── toolbar.rs
│       └── project_panel.rs
│
└── tests/
    ├── timeline_tests.rs
    ├── clip_tests.rs
    ├── audio_tests.rs
    ├── video_tests.rs
    └── serialization_tests.rs
```

A estrutura poderá ser modificada caso a implementação exija, mas a separação de responsabilidades deve ser preservada.

---

# 8. Modelo geral da aplicação

Arquitetura:

```text
                         ┌──────────────┐
                         │      UI      │
                         │    egui      │
                         └──────┬───────┘
                                │
                                ▼
                         ┌──────────────┐
                         │  App State   │
                         └──────┬───────┘
                                │
                  ┌─────────────┴─────────────┐
                  │                           │
                  ▼                           ▼
           ┌──────────────┐            ┌──────────────┐
           │   Project    │            │   Playback   │
           └──────┬───────┘            └──────┬───────┘
                  │                           │
                  ▼                           ▼
           ┌──────────────┐            ┌──────────────┐
           │   Timeline   │            │ Media Engine │
           └──────┬───────┘            └──────┬───────┘
                  │                           │
           ┌──────┴──────┐              ┌─────┴─────┐
           ▼             ▼              ▼           ▼
       Video Tracks  Audio Tracks    Video       Audio
                                     Decoder     Decoder
           │             │              │           │
           ▼             ▼              └─────┬─────┘
      Compositor       Mixer                  │
           │             │                    │
           └──────┬──────┴────────────────────┘
                  │
                  ▼
           Render Pipeline
                  │
             ┌────┴────┐
             ▼         ▼
          Preview    Export
             │         │
             ▼         ▼
           Screen     FFmpeg
                         │
                         ▼
                       MP4
```

---

# 9. Conceito de projeto

O projeto deverá possuir:

```text
Project
│
├── Settings
│
├── Media
│
├── Video Tracks
│
├── Audio Tracks
│
└── Generated Layers
```

Exemplo:

```text
Project
│
├── Media
│   ├── video.mp4
│   ├── music.mp3
│   └── voice.wav
│
├── Video Track 1
│   ├── Video A
│   └── Video B
│
├── Video Track 2
│   └── Solid Color
│
├── Audio Track 1
│   └── Music
│
└── Audio Track 2
    └── Voice
```

---

# 10. Project Settings

O projeto deverá possuir:

```rust
ProjectSettings {
    width: u32,
    height: u32,
    fps: Rational,
    sample_rate: u32,
    channels: u16,
}
```

Configuração inicial sugerida:

```text
1920x1080
30 FPS
48000 Hz
Stereo
```

Posteriormente suportar:

```text
1280x720
1920x1080
2560x1440
3840x2160
24 FPS
25 FPS
30 FPS
50 FPS
60 FPS
```

---

# 11. Sistema de tempo

Não utilizar `f32` como representação principal do tempo.

Preferir uma estrutura baseada em ticks inteiros ou números racionais.

Exemplo:

```rust
struct Time {
    ticks: i64,
}
```

Ou:

```rust
struct Rational {
    numerator: i64,
    denominator: i64,
}
```

O sistema deve representar precisamente:

```text
00:00:00.000
00:00:00.500
00:00:01.000
```

e converter corretamente entre:

```text
Time
Frames
FPS
```

---

# 12. Media

Cada mídia deverá possuir:

```rust
Media {
    id: MediaId,
    path: PathBuf,
    metadata: MediaMetadata,
}
```

Metadata:

```rust
MediaMetadata {
    duration: Time,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<Rational>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
}
```

---

# 13. Media IDs

Não identificar mídia apenas pelo nome do arquivo.

Usar:

```rust
MediaId(Uuid)
```

Isso permite que arquivos com nomes iguais existam no projeto.

---

# 14. Tracks

Tipos:

```rust
enum TrackType {
    Video,
    Audio,
}
```

Track:

```rust
Track {
    id: TrackId,
    name: String,
    track_type: TrackType,
    muted: bool,
    locked: bool,
    clips: Vec<Clip>,
}
```

---

# 15. Video Tracks

Tracks de vídeo são compostas de baixo para cima.

Exemplo:

```text
V3  [TITLE]
V2  [SOLID COLOR]
V1  [VIDEO]
```

A ordem de composição será:

```text
Background
    ↓
V1
    ↓
V2
    ↓
V3
```

A camada superior deve ser composta sobre a inferior.

---

# 16. Audio Tracks

Cada track de áudio deverá possuir:

```rust
AudioTrack {
    volume: f32,
    muted: bool,
}
```

O volume deverá futuramente ser representado em decibéis.

A UI poderá mostrar:

```text
Volume
[-inf] --------●--------- 0 dB
```

---

# 17. Clip

Estrutura base:

```rust
Clip {
    id: ClipId,

    source: MediaId,

    timeline_start: Time,
    timeline_duration: Time,

    source_start: Time,
    source_duration: Time,

    transform: Transform,

    opacity: f32,

    fade_in: Time,
    fade_out: Time,

    reversed: bool,
}
```

---

# 18. Audio Clip

Para áudio:

```rust
AudioClip {
    clip: Clip,

    volume: f32,

    fade_in: Time,

    fade_out: Time,
}
```

---

# 19. Transform

Preparar a arquitetura para:

```rust
Transform {
    position_x: f32,
    position_y: f32,

    scale_x: f32,
    scale_y: f32,

    rotation: f32,
}
```

No MVP:

```text
Position
Scale
```

podem ficar parcialmente implementados.

---

# 20. Split

O usuário poderá dividir um clip.

Original:

```text
[==============================]
0s                            10s
```

Split em 4 segundos:

```text
[============][================]
0s           4s               10s
```

Os dois clips devem apontar para a mesma mídia original.

Não duplicar o arquivo.

---

# 21. Trim

Suportar:

```text
Trim Left
Trim Right
```

Exemplo:

```text
Original:

[==============================]

Trim:

       [==================]
```

A mídia original não deve ser modificada.

---

# 22. Reverse Clip

O editor deverá possuir uma função para inverter um clip.

Nome da operação:

```text
Reverse Clip
```

ou propriedade:

```rust
reversed: bool
```

---

# 23. Reverse de vídeo

Exemplo:

```text
Original:

A → B → C → D → E

Reverse:

E → D → C → B → A
```

Na timeline:

```text
0s                         10s
│---------------------------│

Normal:
A B C D E F G H I J

Reverse:
J I H G F E D C B A
```

---

# 24. Reverse de áudio

O mesmo comportamento deverá existir para áudio.

Original:

```text
Audio:
A → B → C → D
```

Reverse:

```text
D → C → B → A
```

O áudio deverá ser efetivamente reproduzido de trás para frente.

---

# 25. Reverse não destrutivo

O reverse não deve criar obrigatoriamente um novo arquivo.

O sistema deverá calcular a posição no source.

Normal:

```text
source_time = source_start + local_time
```

Reverse:

```text
source_time = source_end - local_time
```

Conceitualmente:

```text
Clip
 ↓
Timeline Time
 ↓
Source Time Mapping
 ↓
Reverse?
 ├── Não → normal mapping
 └── Sim → reverse mapping
 ↓
Decoder
```

---

# 26. Reverse + Split

O reverse deve continuar funcionando depois de split.

Exemplo:

```text
Original:

[==========================]

Split:

[==========][==============]

Reverse apenas segundo clip:

[==========][REVERSED======]
```

---

# 27. Reverse + Trim

Também deverá funcionar com trim.

Exemplo:

```text
Source:
A B C D E F G H I J

Trim:
      C D E F G H

Reverse:
      H G F E D C
```

---

# 28. Fade de vídeo

Cada clip de vídeo deverá possuir:

```text
Fade In
Fade Out
```

Exemplo:

```text
Opacity

1.0 ────────────────
     /
    /
0 ─/
```

Fade-in:

```text
opacity = progress
```

Fade-out:

```text
opacity = 1.0 - progress
```

---

# 29. Fade de áudio

Cada clip de áudio deverá possuir:

```text
Fade In
Fade Out
```

Exemplo:

```text
Audio Gain

1.0 ────────────────
     /
    /
0 ─/
```

Fade-in:

```text
gain = progress
```

Fade-out:

```text
gain = 1.0 - progress
```

---

# 30. Pipeline de áudio

A ordem deverá ser:

```text
Audio File
    ↓
Decoder
    ↓
Source Time Mapping
    ↓
Reverse?
    ↓
Fade In / Fade Out
    ↓
Clip Volume
    ↓
Track Volume
    ↓
Track Mixer
    ↓
Master
    ↓
Output / Encoder
```

O fade deverá ser aplicado antes do mixer final.

---

# 31. Fade de áudio e Reverse

As duas funções devem funcionar simultaneamente.

Exemplo:

```text
Clip:

[====================]

Reverse:
ON

Fade In:
2s

Fade Out:
3s
```

O resultado deverá ser:

```text
Timeline

0s              2s                    7s
│---------------│---------------------│

Gain
0 → 1           1                     1 → 0
```

O conteúdo sonoro, porém, será reproduzido em reverse.

---

# 32. Fade de vídeo e Reverse

Também devem funcionar simultaneamente.

Exemplo:

```text
Reverse = ON
Fade In = 2s
Fade Out = 2s
```

O vídeo começa pelo último frame do source, enquanto a opacidade começa em zero.

---

# 33. Volume

Cada AudioClip deverá possuir:

```rust
volume: f32
```

E cada AudioTrack:

```rust
volume: f32
```

O ganho final será aproximadamente:

```text
clip_gain * track_gain
```

Posteriormente adicionar:

```text
Master Volume
```

---

# 34. Mute

Cada faixa deverá possuir:

```text
Mute
```

Quando:

```text
muted = true
```

o áudio não deverá contribuir para o mixer.

---

# 35. Media Generator

Criar um sistema:

```rust
enum GeneratedMedia {
    SolidColor(SolidColor),
    AudioVisualizer(AudioVisualizer),
}
```

Esses elementos deverão funcionar como clips normais.

---

# 36. Solid Color

O usuário poderá selecionar:

```text
Generate
    ↓
Solid Color
```

Propriedades:

```rust
SolidColor {
    color: Color,
}
```

UI:

```text
Color: [████████]
Duration: 10s
```

A camada deverá poder ser adicionada a qualquer Video Track.

---

# 37. Audio Visualizer

Criar um visualizador semelhante aos players de música antigos.

Exemplo:

```text
        █
        █
   █    █
   █    █       █
   █    █       █
 █ █    █   █   █
 █ █    █   █   █
 █ █ █  █ █ █   █
------------------------
```

---

# 38. Pipeline do visualizador

```text
Audio
  ↓
Decoder
  ↓
PCM
  ↓
FFT
  ↓
Frequency Bands
  ↓
Smoothing
  ↓
Bar Heights
  ↓
Video Renderer
```

---

# 39. FFT

O analisador deverá transformar áudio em frequências.

Exemplo de bandas:

```text
20Hz - 60Hz
60Hz - 120Hz
120Hz - 250Hz
250Hz - 500Hz
500Hz - 1kHz
1kHz - 2kHz
2kHz - 4kHz
4kHz - 8kHz
8kHz - 16kHz
```

O número de bandas deverá ser configurável.

---

# 40. Visualizer Settings

Propriedades:

```text
Bar Count
Bar Width
Gap
Sensitivity
Smoothing
Color
Background
Mirror
Orientation
```

No MVP:

```text
Bar Count
Sensitivity
Smoothing
Color
Background
```

---

# 41. Preview

Interface:

```text
┌────────────────────────────────┐
│                                │
│                                │
│             VIDEO              │
│                                │
│                                │
└────────────────────────────────┘

          ▶ 00:04.230 / 00:20.000
```

Controles:

```text
Play
Pause
Stop
Previous Frame
Next Frame
```

Posteriormente:

```text
Loop
Playback Speed
Fullscreen
```

---

# 42. Playback

O playback deve usar a timeline como fonte de verdade.

Fluxo:

```text
Timeline Time
      ↓
Find Active Clips
      ↓
Resolve Source Time
      ↓
Reverse Mapping
      ↓
Decode
      ↓
Effects
      ↓
Composition
      ↓
Preview
```

---

# 43. Compositor de vídeo

Receber:

```text
Video Track 1
Video Track 2
Video Track 3
Generated Layers
```

e gerar:

```text
Final Video Frame
```

Pipeline:

```text
Background
    ↓
Track 1
    ↓
Track 2
    ↓
Track 3
    ↓
Final Frame
```

---

# 44. Alpha Blending

Cada clip poderá ter:

```rust
opacity: f32
```

O compositor deverá fazer alpha blending.

Isso também será utilizado pelos fades.

---

# 45. Frame Cache

Criar cache para frames decodificados.

Arquitetura:

```text
Media
  ↓
Decoder
  ↓
Frame Cache
  ↓
Renderer
```

Posteriormente utilizar:

```text
LRU Cache
```

---

# 46. Waveform

A timeline deverá ser preparada para mostrar waveform.

Exemplo:

```text
Audio

 /\     /\/\       /\      /\
/  \___/    \_____/  \____/  \__
```

Não é obrigatório no primeiro MVP, mas a arquitetura deverá permitir sua implementação.

---

# 47. Media Browser

Interface:

```text
┌─────────────────────┐
│ Media               │
├─────────────────────┤
│ video.mp4           │
│ music.mp3           │
│ voice.wav           │
│ background.mp4      │
└─────────────────────┘
```

Operações:

```text
Import Media
Remove
Reveal in Explorer
Add to Timeline
```

---

# 48. Drag and Drop

Suportar:

```text
Media Browser
      │
      │ drag
      ▼
Timeline
```

Exemplo:

```text
video.mp4

      ↓

V1 [========== video.mp4 ==========]
```

---

# 49. Inspector

Ao selecionar um clip:

```text
┌──────────────────────────┐
│ Inspector                │
├──────────────────────────┤
│ Clip                     │
│                          │
│ Position                 │
│ X [0]                    │
│ Y [0]                    │
│                          │
│ Scale                    │
│ [100%]                   │
│                          │
│ Opacity                  │
│ [100%]                   │
│                          │
│ Fade In                  │
│ [0.0s]                   │
│                          │
│ Fade Out                 │
│ [0.0s]                   │
│                          │
│ Playback                 │
│                          │
│ Speed                    │
│ [100%]                   │
│                          │
│ [ Reverse Clip ]         │
└──────────────────────────┘
```

Para áudio:

```text
┌──────────────────────────┐
│ Audio                    │
├──────────────────────────┤
│ Volume                   │
│ [-inf]──────●─────0 dB   │
│                          │
│ Fade In                  │
│ [0.0s]                   │
│                          │
│ Fade Out                 │
│ [0.0s]                   │
│                          │
│ [ Reverse Clip ]         │
└──────────────────────────┘
```

---

# 50. Timeline

Estrutura visual:

```text
00:00       00:05       00:10       00:15
│-----------│-----------│-----------│-----------

V3  [      SOLID COLOR      ]

V2  [========= VIDEO A ==========][VIDEO B]

V1  [=========== BACKGROUND VIDEO ============]

A2  [========= VOICE ============]

A1  [================ MUSIC ==================]

                    ▲
                  Playhead
```

---

# 51. Timeline Zoom

Implementar zoom horizontal.

```text
Zoom Out
    ◀──────────────▶
                 Zoom In
```

O zoom altera a quantidade de pixels por unidade de tempo, não os dados do projeto.

---

# 52. Playhead

O playhead representa:

```text
current_time: Time
```

Todos os sistemas devem utilizar esse mesmo valor.

---

# 53. Snap

Posteriormente:

```text
Snap to clips
Snap to playhead
Snap to start
Snap to end
Snap to markers
```

---

# 54. Selection

Suportar:

```text
Selected Clip
Selected Track
Selected Multiple Clips
```

Posteriormente:

```text
Shift Select
Drag Select
Multi-select
```

---

# 55. Command System

As operações de edição deverão ser comandos.

Exemplo:

```rust
trait Command {
    fn execute(&mut self, project: &mut Project);
    fn undo(&mut self, project: &mut Project);
}
```

Comandos:

```text
AddClip
DeleteClip
MoveClip
TrimClip
SplitClip

SetVolume
SetTrackVolume

SetVideoFadeIn
SetVideoFadeOut

SetAudioFadeIn
SetAudioFadeOut

SetClipReversed

AddTrack
DeleteTrack
```

---

# 56. Undo/Redo

Todas as operações importantes deverão suportar:

```text
Undo
Redo
```

Atalhos:

```text
Ctrl + Z
Ctrl + Shift + Z
```

---

# 57. Save/Load

Criar um formato próprio:

```text
.myeditor
```

Inicialmente pode utilizar JSON.

Exemplo:

```json
{
  "version": 1,
  "settings": {
    "width": 1920,
    "height": 1080,
    "fps": 30
  },
  "media": [],
  "tracks": []
}
```

---

# 58. Versionamento do projeto

O arquivo deverá possuir:

```json
{
  "version": 1
}
```

Quando o formato mudar:

```text
version 2
version 3
...
```

Criar migrações quando necessário.

---

# 59. Relink de mídia

Se uma mídia desaparecer:

```text
Media Missing

video.mp4
```

mostrar:

```text
[ Locate File ]
```

O usuário deverá conseguir apontar para o novo local.

---

# 60. Exportação

O editor deverá permitir:

```text
Export
```

Interface:

```text
┌────────────────────────────┐
│ Export                     │
├────────────────────────────┤
│ Format: MP4                │
│ Video Codec: H.264         │
│ Audio Codec: AAC           │
│ Resolution: 1920x1080      │
│ FPS: 30                    │
│ Bitrate: Auto              │
│                            │
│ [ Export ]                 │
└────────────────────────────┘
```

---

# 61. Pipeline de export

```text
Project
   ↓
Timeline Evaluation
   ↓
Video Renderer
   ↓
Audio Renderer
   ↓
FFmpeg Encoder
   ↓
H.264 + AAC
   ↓
MP4
```

---

# 62. Video Export

Para cada frame:

```text
Frame N
  ↓
Timeline Timestamp
  ↓
Active Clips
  ↓
Source Time Mapping
  ↓
Reverse?
  ↓
Decode
  ↓
Fade / Opacity
  ↓
Composition
  ↓
Final Frame
  ↓
H.264 Encoder
```

---

# 63. Audio Export

Para cada segmento:

```text
Timeline
  ↓
Active Audio Clips
  ↓
Source Time Mapping
  ↓
Reverse?
  ↓
Decode PCM
  ↓
Fade In
  ↓
Fade Out
  ↓
Volume
  ↓
Track Mixer
  ↓
AAC Encoder
```

---

# 64. Sincronização

Vídeo e áudio devem utilizar a mesma referência:

```text
Timeline Time
```

Não sincronizar utilizando simplesmente:

```text
sleep()
```

O relógio da timeline é a fonte de verdade.

---

# 65. Export Progress

Durante export mostrar:

```text
Exporting...

██████████████░░░░░░░░ 65%

00:13 / 00:20

[ Cancel ]
```

---

# 66. Cancelamento

O usuário deverá poder cancelar o export.

O processo deverá:

```text
Detect Cancel
    ↓
Stop Renderer
    ↓
Stop Encoder
    ↓
Cleanup
```

Não deixar processos FFmpeg órfãos.

---

# 67. Formatos de entrada

O editor deverá tentar suportar pelo backend:

```text
MP4
MOV
MKV
WebM
AVI
WAV
MP3
AAC
FLAC
```

A disponibilidade real dependerá da build do FFmpeg.

---

# 68. Formato inicial de export

O MVP deverá suportar:

```text
Container:
MP4

Video:
H.264

Audio:
AAC
```

---

# 69. Sistema de erros

Não utilizar `unwrap()` em caminhos críticos.

Preferir:

```rust
Result<T, EditorError>
```

e uma estrutura de erro centralizada.

Exemplo:

```rust
enum EditorError {
    MediaNotFound,
    UnsupportedFormat,
    DecoderError,
    EncoderError,
    InvalidTimeline,
    ExportError,
}
```

---

# 70. Logging

Utilizar logging estruturado.

Categorias:

```text
[APP]
[UI]
[PROJECT]
[MEDIA]
[VIDEO]
[AUDIO]
[TIMELINE]
[RENDER]
[EXPORT]
[GPU]
```

Exemplo:

```text
[MEDIA] Loading video.mp4
[MEDIA] Duration: 00:01:32.500
[MEDIA] Resolution: 1920x1080
[VIDEO] Decoder initialized
```

---

# 71. Testes

Testar principalmente a lógica.

## Timeline

```text
Add Clip
Move Clip
Split Clip
Trim Clip
Delete Clip
```

## Reverse

```text
Reverse Clip
Reverse after Split
Reverse after Trim
Reverse video
Reverse audio
```

## Fade

```text
Video Fade In
Video Fade Out
Audio Fade In
Audio Fade Out
Fade + Reverse
```

## Audio

```text
Volume
Track Volume
Mute
Mixer
Fade
Reverse
```

## Serialization

```text
Save
Load
Round Trip
```

---

# 72. Testes de Split

Testar:

```text
Split no início
Split no final
Split no meio
Split de clip invertido
Split de clip com fade
```

---

# 73. Testes de Reverse

Exemplo:

```text
Source duration = 10s

Normal:

timeline 0s → source 0s
timeline 5s → source 5s
timeline 10s → source 10s
```

Reverse:

```text
timeline 0s → source 10s
timeline 5s → source 5s
timeline 10s → source 0s
```

Os detalhes de endpoint devem ser definidos com precisão de acordo com o sistema de timestamps usado pelo decoder.

---

# 74. Testes de Fade

Fade-in de 2 segundos:

```text
t = 0.0 → gain 0.0
t = 1.0 → gain 0.5
t = 2.0 → gain 1.0
```

Fade-out de 2 segundos:

```text
t = end-2.0 → gain 1.0
t = end-1.0 → gain 0.5
t = end     → gain 0.0
```

Os valores exatos podem utilizar uma curva diferente no futuro, mas o MVP deve começar com interpolação linear.

---

# 75. Testes de Export

Criar projetos pequenos:

```text
5 segundos
1920x1080
30 FPS
```

Verificar:

```text
Arquivo existe
Vídeo existe
Áudio existe quando esperado
Duração correta
FPS correto
Codec correto
Quantidade de frames aproximadamente correta
```

---

# 76. Performance

Prioridade:

## P0

```text
Correto
Estável
Sem crashes
Sem corrupção
Sem dessincronização
```

## P1

```text
Preview 1080p razoavelmente fluido
Timeline responsiva
Seek razoável
```

## P2

```text
GPU
Cache avançado
Proxy
4K
Decoding acelerado
```

---

# 77. Cache

O sistema deverá eventualmente possuir:

```text
Decoded Frame Cache
Audio Cache
Waveform Cache
FFT Cache
Thumbnail Cache
```

Não implementar tudo no primeiro estágio.

---

# 78. Arquitetura de Source Time Mapping

Esta é uma parte crítica do editor.

Todo clip deverá transformar:

```text
Timeline Time
```

em:

```text
Source Time
```

Normal:

```text
source_time = source_start + local_time
```

Reverse:

```text
source_time = source_end - local_time
```

Futuramente isso poderá permitir:

```text
Speed 50%
Speed 200%
Reverse
Time Remapping
```

sem precisar modificar a arquitetura.

---

# 79. Playback Speed

Embora speed não seja obrigatório no MVP, preparar o modelo para:

```rust
speed: f64
```

Exemplos:

```text
0.5x
1.0x
2.0x
```

Reverse poderá ser conceitualmente tratado posteriormente como velocidade negativa:

```text
+1.0 = normal
-1.0 = reverse
```

Porém, no MVP, pode continuar existindo:

```rust
reversed: bool
```

---

# 80. Pipeline final de vídeo

```text
                 Media
                   │
                   ▼
                Decoder
                   │
                   ▼
             Source Mapping
                   │
            ┌──────┴──────┐
            │             │
          Normal        Reverse
            │             │
            └──────┬──────┘
                   ▼
               Transform
                   │
                   ▼
             Fade / Opacity
                   │
                   ▼
              Compositor
                   │
                   ▼
              Final Frame
```

---

# 81. Pipeline final de áudio

```text
                 Media
                   │
                   ▼
                Decoder
                   │
                   ▼
             Source Mapping
                   │
            ┌──────┴──────┐
            │             │
          Normal        Reverse
            │             │
            └──────┬──────┘
                   ▼
              Fade In
                   │
                   ▼
              Fade Out
                   │
                   ▼
             Clip Volume
                   │
                   ▼
             Track Volume
                   │
                   ▼
                Mixer
                   │
                   ▼
                 Master
                   │
                   ▼
              Final Audio
```

---

# 82. Atalhos

Implementar:

```text
Space       Play/Pause
S           Split
Delete      Delete Clip
Ctrl+Z      Undo
Ctrl+Shift+Z Redo
Ctrl+S      Save
Ctrl+O      Open
Ctrl+I      Import
Ctrl+E      Export
```

Opcional:

```text
R           Reverse Clip
```

---

# 83. MVP

O primeiro MVP deverá conter:

```text
✓ Janela desktop
✓ Interface básica
✓ Media Browser
✓ Importação de vídeo
✓ Importação de áudio
✓ Video Tracks
✓ Audio Tracks
✓ Timeline
✓ Playhead
✓ Drag and Drop
✓ Seleção
✓ Split
✓ Trim
✓ Play
✓ Pause
✓ Seek
✓ Video Preview
✓ Fade-in de vídeo
✓ Fade-out de vídeo
✓ Fade-in de áudio
✓ Fade-out de áudio
✓ Volume de áudio
✓ Mute
✓ Reverse de vídeo
✓ Reverse de áudio
✓ Solid Color
✓ Audio Visualizer
✓ Video Composition
✓ Audio Mixer
✓ Export MP4
✓ H.264
✓ AAC
✓ Save Project
✓ Load Project
✓ Undo
✓ Redo
```

---

# 84. Funcionalidades fora do MVP

Não implementar inicialmente:

```text
✗ Text editor avançado
✗ Legendas
✗ Chroma Key
✗ Motion Tracking
✗ Color Grading avançado
✗ 3D
✗ Multicam
✗ Cloud
✗ Colaboração
✗ Streaming
✗ Proxy avançado
✗ Efeitos complexos
```

---

**Status real (não editar os checklists de fase abaixo — são o histórico da spec original; ver "Current state" no topo do arquivo pro estado atual de verdade):** Fases 0-20 implementadas e funcionando de ponta a ponta, exceto Fase 17 (GPU), pulada de propósito.

# 85. Fase 0 — Bootstrap

Objetivo:

Criar a aplicação básica.

Tarefas:

```text
- [ ] Criar repositório.
- [ ] Criar Cargo project/workspace.
- [ ] Configurar Rust stable.
- [ ] Configurar rustfmt.
- [ ] Configurar Clippy.
- [ ] Configurar testes.
- [ ] Criar janela.
- [ ] Criar aplicação egui.
```

Resultado:

```text
Aplicação abre uma janela desktop.
```

---

# 86. Fase 1 — UI

```text
- [ ] Criar layout principal.
- [ ] Criar menu.
- [ ] Criar Media Browser.
- [ ] Criar Preview.
- [ ] Criar Timeline.
- [ ] Criar Inspector.
- [ ] Criar Toolbar.
```

Resultado:

```text
┌──────────────────────────────────────┐
│ Menu                                 │
├───────────┬──────────────────────────┤
│ Media     │ Preview                  │
│           │                          │
├───────────┴──────────────────────────┤
│ Timeline                             │
│                                      │
└──────────────────────────────────────┘
```

---

# 87. Fase 2 — Project Model

```text
- [ ] Project
- [ ] ProjectSettings
- [ ] Media
- [ ] MediaId
- [ ] Track
- [ ] TrackId
- [ ] Clip
- [ ] ClipId
- [ ] Time
- [ ] Selection
```

Resultado:

```text
O projeto pode existir sem mídia real.
```

---

# 88. Fase 3 — Serialization

```text
- [ ] Save project.
- [ ] Load project.
- [ ] Version.
- [ ] Validation.
- [ ] Missing media detection.
```

Resultado:

```text
Projeto pode ser salvo e reaberto.
```

---

# 89. Fase 4 — Media Import

```text
- [ ] File picker.
- [ ] Import video.
- [ ] Import audio.
- [ ] Metadata.
- [ ] Duration.
- [ ] Resolution.
- [ ] FPS.
- [ ] Codec.
- [ ] Sample rate.
```

---

# 90. Fase 5 — Timeline

```text
- [ ] Tracks.
- [ ] Add Clip.
- [ ] Move Clip.
- [ ] Resize Clip.
- [ ] Selection.
- [ ] Playhead.
- [ ] Ruler.
- [ ] Zoom.
```

---

# 91. Fase 6 — Split e Trim

```text
- [ ] Split.
- [ ] Trim Left.
- [ ] Trim Right.
- [ ] Preserve source.
- [ ] Tests.
```

---

# 92. Fase 7 — Video Decoder

```text
- [ ] FFmpeg integration.
- [ ] Open file.
- [ ] Initialize decoder.
- [ ] Decode frame.
- [ ] Convert frame.
- [ ] Display frame.
- [ ] Seek.
```

---

# 93. Fase 8 — Playback

```text
- [ ] Play.
- [ ] Pause.
- [ ] Stop.
- [ ] Seek.
- [ ] Playhead synchronization.
- [ ] Frame stepping.
- [ ] FPS synchronization.
```

---

# 94. Fase 9 — Video Composition

```text
- [ ] Active clip evaluation.
- [ ] Multiple video tracks.
- [ ] Alpha blending.
- [ ] Opacity.
- [ ] Background.
```

---

# 95. Fase 10 — Fade de Vídeo

```text
- [ ] Fade-in.
- [ ] Fade-out.
- [ ] Inspector controls.
- [ ] Preview.
- [ ] Export.
- [ ] Tests.
```

---

# 96. Fase 11 — Reverse de Vídeo

```text
- [ ] reversed property.
- [ ] Source time mapping.
- [ ] Reverse playback.
- [ ] Reverse + Split.
- [ ] Reverse + Trim.
- [ ] Reverse + Fade.
- [ ] Preview.
- [ ] Export.
- [ ] Tests.
```

---

# 97. Fase 12 — Audio

```text
- [ ] Audio decoder.
- [ ] PCM.
- [ ] Audio tracks.
- [ ] Audio clips.
- [ ] Audio playback.
- [ ] Audio mixer.
- [ ] Volume.
- [ ] Track volume.
- [ ] Mute.
```

---

# 98. Fase 13 — Fade de Áudio

```text
- [ ] Audio Fade-in.
- [ ] Audio Fade-out.
- [ ] Inspector.
- [ ] Linear gain.
- [ ] Preview.
- [ ] Export.
- [ ] Tests.
```

---

# 99. Fase 14 — Reverse de Áudio

```text
- [ ] Reverse audio mapping.
- [ ] Reverse PCM.
- [ ] Reverse playback.
- [ ] Reverse + Fade.
- [ ] Reverse + Volume.
- [ ] Preview.
- [ ] Export.
- [ ] Tests.
```

---

# 100. Fase 15 — Solid Color

```text
- [ ] GeneratedMedia.
- [ ] SolidColor.
- [ ] Color picker.
- [ ] Duration.
- [ ] Add to timeline.
- [ ] Preview.
- [ ] Export.
```

---

# 101. Fase 16 — Audio Visualizer

```text
- [ ] Audio analyzer.
- [ ] FFT.
- [ ] Frequency bands.
- [ ] Smoothing.
- [ ] Bar renderer.
- [ ] Color.
- [ ] Background.
- [ ] Preview.
- [ ] Export.
```

---

# 102. Fase 17 — GPU

Somente depois do pipeline básico funcionar:

```text
- [ ] Integrar wgpu.
- [ ] Texture pipeline.
- [ ] Frame upload.
- [ ] GPU composition.
- [ ] Alpha blending.
- [ ] Visualizer rendering.
- [ ] Shaders.
```

---

# 103. Fase 18 — Export

```text
- [ ] Export dialog.
- [ ] Offline renderer.
- [ ] Video renderer.
- [ ] Audio renderer.
- [ ] H.264 encoder.
- [ ] AAC encoder.
- [ ] MP4 muxer.
- [ ] Progress.
- [ ] Cancel.
- [ ] Error handling.
```

---

# 104. Fase 19 — Undo/Redo

```text
- [ ] Command system.
- [ ] Undo stack.
- [ ] Redo stack.
- [ ] Split.
- [ ] Trim.
- [ ] Move.
- [ ] Delete.
- [ ] Volume.
- [ ] Fade.
- [ ] Reverse.
```

---

# 105. Fase 20 — Polish

```text
- [ ] Keyboard shortcuts.
- [ ] Better timeline.
- [ ] Snap.
- [ ] Waveform.
- [ ] Thumbnails.
- [ ] Better error messages.
- [ ] Project recovery.
- [ ] Performance improvements.
```

---

# 106. Contrato para agentes

Todo agente deverá:

```text
- [ ] Ler este documento antes de trabalhar.
- [ ] Entender a arquitetura existente.
- [ ] Não reescrever módulos sem necessidade.
- [ ] Não adicionar dependências desnecessárias.
- [ ] Criar testes para lógica nova.
- [ ] Executar cargo fmt.
- [ ] Executar cargo clippy.
- [ ] Executar cargo test.
- [ ] Verificar cargo build.
- [ ] Não quebrar funcionalidades existentes.
- [ ] Documentar decisões arquiteturais relevantes.
```

---

# 107. Cada tarefa deve ser pequena

Os agentes não devem receber:

```text
Implementar todo o editor.
```

Preferir:

```text
Implementar SplitClip.
```

ou:

```text
Implementar Audio Fade In.
```

ou:

```text
Implementar Source Time Mapping para Reverse.
```

---

# 108. Template de tarefa para agentes

Utilizar este formato:

```text
Tarefa:

Implementar [NOME DA FUNCIONALIDADE].

Objetivo:

[DESCRIÇÃO]

Arquivos que podem ser modificados:

[ARQUIVOS]

Requisitos:

- [REQUISITO]
- [REQUISITO]
- [REQUISITO]

Não fazer:

- [RESTRIÇÃO]
- [RESTRIÇÃO]

Testes obrigatórios:

- [TESTE]
- [TESTE]

Critério de conclusão:

- cargo fmt
- cargo clippy
- cargo test
- cargo build
```

---

# 109. Exemplo de tarefa — Reverse

```text
Tarefa:

Implementar reverse de clips.

Objetivo:

Permitir que clips de vídeo e áudio sejam reproduzidos de trás para frente.

Requisitos:

- Adicionar propriedade reversed.
- Não modificar o arquivo original.
- Criar source time mapping.
- Suportar vídeo.
- Suportar áudio.
- Suportar split.
- Suportar trim.
- Suportar fade.
- Suportar undo/redo.

Não fazer:

- Não criar arquivo intermediário permanente.
- Não duplicar mídia original.
- Não alterar Media.
```

---

# 110. Exemplo de tarefa — Audio Fade

```text
Tarefa:

Implementar fade-in e fade-out de áudio.

Objetivo:

Permitir controlar gradualmente o ganho de um AudioClip.

Requisitos:

- fade_in: Time
- fade_out: Time
- ganho linear inicialmente
- funcionar no preview
- funcionar no export
- funcionar com reverse
- funcionar com volume
- suportar undo/redo

Testes:

- fade-in de 2 segundos
- fade-out de 2 segundos
- fade + reverse
- fade + volume
```

---

# 111. Estratégia de commits

Preferir commits pequenos:

```text
feat: add project model
feat: add timeline tracks
feat: add clip selection
feat: add split command
feat: add trim command
feat: add video decoder
feat: add preview playback
feat: add video composition
feat: add video fade
feat: add reverse clip
feat: add audio decoder
feat: add audio mixer
feat: add audio volume
feat: add audio fade
feat: add audio reverse
feat: add solid color generator
feat: add audio visualizer
feat: add mp4 exporter
feat: add undo redo
```

Evitar:

```text
feat: implement entire video editor
```

---

# 112. Dependências

Adicionar dependências somente quando necessárias.

Categorias esperadas:

```text
GUI
egui
eframe

Media
FFmpeg bindings / integration

GPU
wgpu

Serialization
serde
serde_json

IDs
uuid

Errors
thiserror

Logging
tracing

FFT
FFT crate apropriada

Audio Playback
backend apropriado
```

Os agentes deverão verificar as versões atuais e compatibilidade das crates antes de adicionar dependências.

---

# 113. Segurança e estabilidade

Arquivos de mídia são entradas externas.

O sistema deve:

```text
- [ ] Tratar arquivos corrompidos.
- [ ] Tratar codecs desconhecidos.
- [ ] Tratar arquivos sem áudio.
- [ ] Tratar arquivos sem vídeo.
- [ ] Tratar duração inválida.
- [ ] Tratar FPS inválido.
- [ ] Tratar caminhos inválidos.
- [ ] Evitar crashes.
- [ ] Não modificar arquivos originais.
- [ ] Não sobrescrever o arquivo de origem durante export.
```

---

# 114. Critério de sucesso do MVP

O MVP será considerado funcional quando o usuário conseguir executar este workflow:

```text
1. Abrir o aplicativo.

2. Importar:

   video.mp4
   music.mp3

3. Arrastar o vídeo para V1.

4. Arrastar música para A1.

5. Cortar o vídeo.

6. Remover uma parte.

7. Inverter um dos clips.

8. Ajustar fade-in do vídeo.

9. Ajustar fade-out do vídeo.

10. Ajustar fade-in do áudio.

11. Ajustar fade-out do áudio.

12. Ajustar volume.

13. Inverter o áudio.

14. Adicionar uma camada Solid Color.

15. Adicionar Audio Visualizer.

16. Pressionar Play.

17. Visualizar o resultado sincronizado.

18. Salvar o projeto.

19. Fechar o aplicativo.

20. Abrir novamente o projeto.

21. Exportar para:

    final.mp4

22. Abrir final.mp4 em um player externo.

23. Verificar que vídeo e áudio estão sincronizados.
```

---

# 115. Resultado esperado

Ao final do MVP:

```text
                    EDITOR
                       │
       ┌───────────────┼────────────────┐
       │               │                │
       ▼               ▼                ▼
   Import Media     Generator        Project
       │               │                │
       ▼               ▼                ▼
     Video        Solid Color       Save/Load
     Audio        Visualizer
       │               │
       └───────┬───────┘
               ▼
            Timeline
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
    Cut       Fade     Reverse
     │         │         │
     └─────────┼─────────┘
               ▼
          Composition
               │
               ▼
            Preview
               │
               ▼
             Export
               │
               ▼
            output.mp4
```

---

# 116. Arquitetura final

```text
                    ┌─────────────────┐
                    │       UI        │
                    │      egui       │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │    App State    │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │     Project     │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │    Timeline     │
                    └────────┬────────┘
                             │
              ┌──────────────┴──────────────┐
              │                             │
              ▼                             ▼
       ┌──────────────┐             ┌──────────────┐
       │ Video Tracks │             │ Audio Tracks │
       └──────┬───────┘             └──────┬───────┘
              │                            │
              ▼                            ▼
       ┌──────────────┐             ┌──────────────┐
       │Video Engine  │             │ Audio Engine │
       └──────┬───────┘             └──────┬───────┘
              │                            │
              ▼                            ▼
       ┌──────────────┐             ┌──────────────┐
       │Source Time   │             │Source Time   │
       │Mapping       │             │Mapping       │
       └──────┬───────┘             └──────┬───────┘
              │                            │
        ┌─────┴─────┐                ┌─────┴─────┐
        ▼           ▼                ▼           ▼
     Normal      Reverse          Normal      Reverse
        │           │                │           │
        └─────┬─────┘                └─────┬─────┘
              │                            │
              ▼                            ▼
       ┌──────────────┐             ┌──────────────┐
       │ Fade/Opacity │             │ Fade/Volume  │
       └──────┬───────┘             └──────┬───────┘
              │                            │
              ▼                            ▼
       ┌──────────────┐             ┌──────────────┐
       │  Compositor  │             │     Mixer    │
       └──────┬───────┘             └──────┬───────┘
              │                            │
              └────────────┬───────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │Render Engine │
                    └──────┬───────┘
                           │
                    ┌──────┴──────┐
                    ▼             ▼
                 Preview        Export
                    │             │
                    ▼             ▼
                  Screen        FFmpeg
                                  │
                                  ▼
                                MP4
```

---

# 117. Regra mais importante

A implementação deverá seguir esta prioridade:

```text
CORREÇÃO
    ↓
ESTABILIDADE
    ↓
ARQUITETURA
    ↓
FUNCIONALIDADE
    ↓
PERFORMANCE
    ↓
POLIMENTO
```

Não otimizar antes de possuir um pipeline funcional.

Não implementar GPU antes de o pipeline CPU funcionar.

Não implementar efeitos avançados antes de a timeline estar correta.

Não modificar mídia original.

Não duplicar arquivos sem necessidade.

O **Project + Timeline + Source Time Mapping** deve ser tratado como o núcleo do sistema.

Especialmente, todas as funcionalidades relacionadas a:

```text
Trim
Split
Reverse
Speed
Fade
```

devem trabalhar através do mapeamento:

```text
Timeline Time
        ↓
Clip Time
        ↓
Source Time
```

Isso permitirá que o editor evolua posteriormente para recursos mais avançados sem precisar reescrever o motor de mídia.

---

# 118. Roadmap futuro

Após o MVP:

## Edição

```text
Ripple Edit
Roll Edit
Slip
Slide
Snapping avançado
Markers
Time Remapping
Speed Ramping
```

## Vídeo

```text
Crop
Rotate
Scale
Position
Blur
Brightness
Contrast
Saturation
Transitions
Masks
```

## Texto

```text
Text Layer
Fonts
Color
Stroke
Shadow
Position
Animation
```

## Áudio

```text
EQ
Compressor
Limiter
Normalization
Waveform
Noise Reduction
```

## Visualizer

```text
Spectrum
Circular Visualizer
Waveform Visualizer
Particles
Shaders
Custom Visualizers
```

## Performance

```text
Proxy Media
GPU Decoding
GPU Rendering
Parallel Export
Background Decoding
Smart Cache
```

---

# 119. Definição final do produto

O produto final desta especificação é um editor de vídeo desktop com:

```text
                    ┌──────────────────────┐
                    │      MEDIA           │
                    │                      │
                    │ Video / Audio        │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │      TIMELINE        │
                    │                      │
                    │ V3 ─ Generated       │
                    │ V2 ─ Video           │
                    │ V1 ─ Video           │
                    │ A2 ─ Voice            │
                    │ A1 ─ Music            │
                    └──────────┬───────────┘
                               │
          ┌────────────────────┼────────────────────┐
          │                    │                    │
          ▼                    ▼                    ▼
        CUT                   FADE                REVERSE
          │                    │                    │
          └────────────────────┼────────────────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │      COMPOSER        │
                    │                      │
                    │ Video + Audio        │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │       PREVIEW        │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │        EXPORT        │
                    │                      │
                    │ H.264 + AAC → MP4    │
                    └──────────────────────┘
```

**Objetivo final:** construir um editor pequeno e confiável em Rust, cuja arquitetura permita começar com edição simples de vídeo e áudio e evoluir gradualmente para um editor completo sem precisar substituir o núcleo do projeto.
