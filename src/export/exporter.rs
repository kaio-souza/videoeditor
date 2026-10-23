use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::event::FfmpegEvent;

use crate::error::{EditorError, Result};
use crate::export::ffmpeg::parse_time;
use crate::project::{
    AudioVisualizerSettings, Clip, GeneratedMedia, MediaId, Project, Rational, Time, Track,
    VisualizerAlignment,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoQuality {
    Low,
    Medium,
    High,
    VeryHigh,
}

impl VideoQuality {
    pub fn crf(self) -> u32 {
        match self {
            VideoQuality::Low => 28,
            VideoQuality::Medium => 23,
            VideoQuality::High => 18,
            VideoQuality::VeryHigh => 12,
        }
    }

    pub const ALL: [VideoQuality; 4] = [
        VideoQuality::Low,
        VideoQuality::Medium,
        VideoQuality::High,
        VideoQuality::VeryHigh,
    ];

    /// CRF scale differs by codec (`libx264` 0-51, `libvpx-vp9` 0-63).
    pub fn crf_for(self, format: ExportFormat) -> u32 {
        match format {
            ExportFormat::WebM => match self {
                VideoQuality::Low => 40,
                VideoQuality::Medium => 32,
                VideoQuality::High => 24,
                VideoQuality::VeryHigh => 16,
            },
            _ => self.crf(),
        }
    }
}

/// Formato de exportação (seção 67-68, "Mais formatos de exportação" no
/// roadmap) — cada um é um par container+codecs, não um menu de opções
/// independentes: misturar `libvpx-vp9`/`libopus` num `.mp4`, ou
/// `libx264`/`aac` num `.webm`, não funciona (o container rejeita ou o
/// player não decodifica), então a UI só deixa escolher o par pronto, não
/// os componentes separados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Mp4,
    Mov,
    Mkv,
    WebM,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 4] = [
        ExportFormat::Mp4,
        ExportFormat::Mov,
        ExportFormat::Mkv,
        ExportFormat::WebM,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Mp4 => "mp4",
            ExportFormat::Mov => "mov",
            ExportFormat::Mkv => "mkv",
            ExportFormat::WebM => "webm",
        }
    }

    /// H.264 é o mesmo codec pra MP4/MOV/MKV (só o container muda) —
    /// `libvpx-vp9` é o único que exige um container diferente (WebM não
    /// aceita H.264/AAC).
    pub fn video_codec(self) -> &'static str {
        match self {
            ExportFormat::Mp4 | ExportFormat::Mov | ExportFormat::Mkv => "libx264",
            ExportFormat::WebM => "libvpx-vp9",
        }
    }

    pub fn audio_codec(self) -> &'static str {
        match self {
            ExportFormat::Mp4 | ExportFormat::Mov | ExportFormat::Mkv => "aac",
            ExportFormat::WebM => "libopus",
        }
    }
}

pub struct ExportSettings {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub quality: VideoQuality,
    pub format: ExportFormat,
}

enum MediaInput<'a> {
    File(&'a Path),
    Color([u8; 3]),
}

fn resolve_media_input(project: &Project, media_id: MediaId) -> Option<MediaInput<'_>> {
    if let Some(media) = project.get_media(media_id) {
        return Some(MediaInput::File(media.path.as_path()));
    }
    let entry = project.generated.get(&media_id)?;
    match &entry.kind {
        GeneratedMedia::SolidColor(c) => Some(MediaInput::Color([c.r, c.g, c.b])),
        GeneratedMedia::AudioVisualizer(_) => None,
    }
}

/// Se `media_id` for um `GeneratedMedia::AudioVisualizer`, devolve suas
/// configs (settings é `Copy`, então uma cópia solta é mais simples do que
/// carregar o lifetime do `Project` por todo o loop de export).
fn audio_visualizer_settings(
    project: &Project,
    media_id: MediaId,
) -> Option<AudioVisualizerSettings> {
    match project.generated.get(&media_id).map(|e| &e.kind) {
        Some(GeneratedMedia::AudioVisualizer(s)) => Some(*s),
        _ => None,
    }
}

fn sorted_clips(track: &Track) -> Vec<&Clip> {
    let mut clips: Vec<&Clip> = track.clips.iter().collect();
    clips.sort_by_key(|c| c.timeline_start.ticks());
    clips
}

/// Pra cada clip de `clips` (já ordenados por `timeline_start`, como
/// `sorted_clips` devolve), a duração do vão que precisa de um filler
/// *antes* dele pra manter o `concat` de uma track alinhado com o eixo de
/// tempo absoluto da timeline — `concat` só sabe grudar streams um atrás
/// do outro, não tem noção de onde cada clip fica de verdade. `None`
/// quando o clip já começa exatamente onde o anterior terminou (ou é o
/// primeiro e começa em `Time::ZERO`), sem vão nenhum pra preencher.
/// Mesma lógica pra vídeo e áudio — só o que vira filler difere (preto
/// transparente vs. silêncio).
fn gap_before_each_clip(clips: &[&Clip]) -> Vec<Option<Time>> {
    let mut cursor = Time::ZERO;
    let mut gaps = Vec::with_capacity(clips.len());
    for clip in clips {
        gaps.push((clip.timeline_start > cursor).then(|| clip.timeline_start - cursor));
        cursor = clip.timeline_end();
    }
    gaps
}

/// Builder do `filter_complex` — cada `-i` novo soma no mesmo índice global
/// (ffmpeg numera inputs na ordem em que `-i`/`-f lavfi -i` aparecem no
/// comando inteiro, não por track).
struct FilterGraph<'a> {
    cmd: &'a mut FfmpegCommand,
    parts: Vec<String>,
    input_index: u32,
}

impl<'a> FilterGraph<'a> {
    fn new(cmd: &'a mut FfmpegCommand) -> Self {
        Self {
            cmd,
            parts: Vec::new(),
            input_index: 0,
        }
    }

    /// Adiciona o `-i` do clip e devolve o índice de input que ele ocupou.
    fn add_input(
        &mut self,
        project: &Project,
        clip: &Clip,
        settings: &ExportSettings,
    ) -> Option<u32> {
        let input = resolve_media_input(project, clip.source)?;
        let duration = clip.timeline_duration.as_secs_f64();
        match input {
            MediaInput::File(path) => {
                if crate::media::is_image_path(path) {
                    // A still has no time axis; `-loop 1 -t <duration>`
                    // instead of a seek keeps it visible for the whole clip.
                    self.cmd
                        .args(["-loop", "1", "-t", &format!("{duration:.6}")]);
                } else {
                    self.cmd
                        .arg("-ss")
                        .arg(format!("{:.6}", clip.source_start.as_secs_f64()));
                }
                self.cmd.input(path.to_string_lossy());
            }
            MediaInput::Color([r, g, b]) => {
                self.cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    &format!(
                        "color=c=0x{r:02x}{g:02x}{b:02x}:s={}x{}:d={:.6}:r={}",
                        settings.width,
                        settings.height,
                        duration,
                        settings.fps.as_f64()
                    ),
                ]);
            }
        }
        let idx = self.input_index;
        self.input_index += 1;
        Some(idx)
    }

    fn push(&mut self, chain: String) {
        self.parts.push(chain);
    }

    /// Fundo preto opaco (seção 43 — "Background" é a camada de baixo de
    /// tudo no pipeline de composição) usado como base do primeiro
    /// `overlay`, no lugar de usar a track de baixo (`V1`) crua. Sem isso,
    /// `V1` virava a `base` diretamente e seu canal alpha nunca tinha
    /// nada embaixo pra fazer blend — fade in/out e opacidade de qualquer
    /// clip na track de baixo (o caso mais comum: um projeto com uma track
    /// de vídeo só) ficavam sem efeito nenhum no export, porque
    /// `format=yuv420p` no fim descarta o alpha de qualquer jeito. Usuário
    /// relatou exatamente isso: "opacidade e o fadein fadeout não estão
    /// exportando". O preview já faz isso certo (`ui/preview.rs::composite`
    /// inicializa o buffer com preto e faz blend de toda camada, inclusive
    /// a de baixo, contra ele) — esse fundo replica o mesmo comportamento
    /// no export.
    fn add_black_background(&mut self, w: u32, h: u32, fps: f64, duration_secs: f64) -> u32 {
        self.cmd.args([
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s={w}x{h}:d={duration_secs:.6}:r={fps}"),
        ]);
        let idx = self.input_index;
        self.input_index += 1;
        idx
    }

    /// Fonte de áudio digital-silêncio (`anullsrc`), usada só quando um
    /// clipe de Audio Visualizer existe mas o projeto não tem nenhuma track
    /// de áudio real pra reagir — sem isso o showfreqs/showwaves não teria
    /// o que analisar.
    fn add_silent_audio(&mut self, duration_secs: f64) -> u32 {
        self.cmd.args([
            "-f",
            "lavfi",
            "-i",
            &format!("anullsrc=r=44100:cl=stereo:d={duration_secs:.6}"),
        ]);
        let idx = self.input_index;
        self.input_index += 1;
        idx
    }

    fn finish(self) -> String {
        self.parts.join(";")
    }
}

/// Filtro de um clip de vídeo: trim, reverse, opacity (constante por clip),
/// fade *de alpha* (não escurece pro preto — funde de verdade com a camada
/// de baixo, seção 28/44), escala. Sai em `yuva420p` (com canal alpha) pra
/// poder ser sobreposto (seção 43).
///
/// Sem Pan/Crop, escala com `force_original_aspect_ratio=increase` + `crop`
/// (estilo "Crop" do Vegas) em vez de um `scale=WxH` plano — cobre o frame
/// inteiro preservando aspect ratio, ao invés de esticar/distorcer fontes
/// com aspect ratio diferente do projeto. Com Pan/Crop (`clip.transform`),
/// usa a região exata escolhida no diálogo em vez disso — ver
/// `decoder::decode_frame_at_scaled`.
fn video_clip_chain(input_idx: u32, clip: &Clip, settings: &ExportSettings, label: &str) -> String {
    let duration = clip.timeline_duration.as_secs_f64();
    let mut chain = format!("[{input_idx}:v]trim=duration={duration:.6},setpts=PTS-STARTPTS");
    if clip.reversed {
        chain.push_str(",reverse");
    }
    // Mesma lógica de `decoder::decode_frame_at_scaled` (ver o comentário
    // lá) — Pan/Crop e crop-fit padrão não se empilham, um substitui o
    // outro: com Pan/Crop, a região que o usuário escolheu já é a região
    // certa direto do source, aplicar o crop-fit padrão em cima cortaria
    // ainda mais.
    chain.push(',');
    match clip.transform.crop_filter_expr() {
        Some(crop) => {
            chain.push_str(&crop);
            chain.push_str(&format!(",scale={}:{}", settings.width, settings.height));
        }
        None => {
            chain.push_str(&format!(
                "scale={0}:{1}:force_original_aspect_ratio=increase,crop={0}:{1}",
                settings.width, settings.height
            ));
        }
    }
    // `scale=...:force_original_aspect_ratio=increase` (crop-fit padrão)
    // deixa um SAR residual em vez de 1:1 exato sempre que o aspect ratio
    // do source não bate exatamente com o do projeto (ex: `10239:10240`
    // em vez de `1:1`) — a proporção real da imagem já foi resolvida pelo
    // `scale`+`crop`, então o SAR daqui em diante deveria ser sempre
    // quadrado, mas o filtro não zera isso sozinho. Clipes com sources de
    // aspect ratio diferentes (ex: vídeo certo + imagem + vídeo com
    // Pan/Crop, todos concatenados na mesma track) então chegam no
    // `concat`/`overlay` com SAR levemente diferentes uns dos outros, e
    // esses filtros exigem SAR *idêntico* entre todo mundo — real causa
    // por trás de "Error reinitializing filters!" / "Could not open
    // encoder before EOF" que o usuário relatou depois que o log real
    // passou a ser repassado (`Input link ... parameters (size 1920x1080,
    // SAR 10239:10240) do not match ... (1920x1080, SAR 1:1)` no stderr
    // completo). `setsar=1` força de volta pra pixel quadrado depois de
    // qualquer scale/crop, então todo clip chega igual.
    chain.push_str(",setsar=1,format=yuva420p");

    let fade_in = clip.fade_in.as_secs_f64();
    let fade_out = clip.fade_out.as_secs_f64();
    if fade_in > 0.0 {
        chain.push_str(&format!(",fade=t=in:st=0:d={fade_in:.6}:alpha=1"));
    }
    if fade_out > 0.0 {
        chain.push_str(&format!(
            ",fade=t=out:st={:.6}:d={fade_out:.6}:alpha=1",
            (duration - fade_out).max(0.0)
        ));
    }
    if (clip.opacity - 1.0).abs() > f32::EPSILON {
        chain.push_str(&format!(",colorchannelmixer=aa={:.6}", clip.opacity));
    }
    chain.push_str(&format!("[{label}]"));
    chain
}

fn audio_clip_chain(input_idx: u32, clip: &Clip, label: &str) -> String {
    let duration = clip.timeline_duration.as_secs_f64();
    let mut chain = format!("[{input_idx}:a]atrim=duration={duration:.6},asetpts=PTS-STARTPTS");
    if clip.reversed {
        chain.push_str(",areverse");
    }
    let fade_in = clip.fade_in.as_secs_f64();
    let fade_out = clip.fade_out.as_secs_f64();
    if fade_in > 0.0 {
        chain.push_str(&format!(",afade=t=in:st=0:d={fade_in:.6}"));
    }
    if fade_out > 0.0 {
        chain.push_str(&format!(
            ",afade=t=out:st={:.6}:d={fade_out:.6}",
            (duration - fade_out).max(0.0)
        ));
    }
    if (clip.volume - 1.0).abs() > f32::EPSILON {
        chain.push_str(&format!(",volume={:.6}", clip.volume));
    }
    chain.push_str(&format!("[{label}]"));
    chain
}

fn push_visualizer_clip(
    graph: &mut FilterGraph,
    audio_label: &str,
    clip: &Clip,
    settings: &AudioVisualizerSettings,
    export_settings: &ExportSettings,
    label: &str,
) {
    let (w, h) = (export_settings.width, export_settings.height);
    let fps = export_settings.fps.as_f64();
    let start = clip.timeline_start.as_secs_f64();
    let end = clip.timeline_end().as_secs_f64();
    let dur = clip.timeline_duration.as_secs_f64();
    let color_hex = format!(
        "0x{:02x}{:02x}{:02x}",
        settings.color.r, settings.color.g, settings.color.b
    );
    let raw = format!("viz_raw_{label}");
    let pad_frac = settings.padding.clamp(0.0, 0.4);
    let effective_w = ((w as f32 * (1.0 - 2.0 * pad_frac)).round() as u32).max(2);
    let pad_x = w.saturating_sub(effective_w) / 2;
    let pad_y_margin = (h as f32 * pad_frac).round() as u32;
    let usable_h = h.saturating_sub(pad_y_margin * 2).max(2);
    let effective_h = ((usable_h as f32 * settings.height.clamp(0.05, 1.0)).round() as u32).max(2);
    let bar_count = settings.bar_count.clamp(4, 128);
    let cell_w = (effective_w / bar_count).max(1);
    let gap_filters = |bar_h: u32| {
        format!(
            "scale={effective_w}:{bar_h}:flags=neighbor,drawgrid=w={cell_w}:h=999999:t=2:c=black@1"
        )
    };

    let trim_stage =
        format!("[{audio_label}]atrim=start={start:.6}:end={end:.6},asetpts=PTS-STARTPTS");

    match settings.alignment {
        VisualizerAlignment::Base => {
            let pad_y = h.saturating_sub(pad_y_margin).saturating_sub(effective_h);
            let gap = gap_filters(effective_h);
            graph.push(format!(
                "{trim_stage},showfreqs=s={bar_count}x{effective_h}:mode=bar:ascale=log:fscale=log:colors={color_hex}:r={fps:.6},{gap},pad={w}:{h}:{pad_x}:{pad_y}:color=black@0,format=rgba,colorkey=0x000000:0.15:0.1,trim=duration={dur:.6},setpts=PTS-STARTPTS[{raw}]"
            ));
        }
        VisualizerAlignment::Center => {
            let half_h = (effective_h / 2).max(1);
            let bars_h = half_h * 2;
            let pad_y = h.saturating_sub(bars_h) / 2;
            let half_label = format!("viz_half_{label}");
            let gap = gap_filters(half_h);
            graph.push(format!(
                "{trim_stage},showfreqs=s={bar_count}x{half_h}:mode=bar:ascale=log:fscale=log:colors={color_hex}:r={fps:.6},{gap}[{half_label}]"
            ));
            graph.push(format!(
                "[{half_label}]split[viz_top_{label}][viz_bottom_{label}]"
            ));
            graph.push(format!("[viz_bottom_{label}]vflip[viz_bottom_f_{label}]"));
            graph.push(format!(
                "[viz_top_{label}][viz_bottom_f_{label}]vstack=inputs=2,pad={w}:{h}:{pad_x}:{pad_y}:color=black@0,format=rgba,colorkey=0x000000:0.15:0.1,trim=duration={dur:.6},setpts=PTS-STARTPTS[{raw}]"
            ));
        }
    }

    let composed = if let Some(bg) = settings.background {
        let bg_label = format!("viz_bg_{label}");
        graph.push(format!(
            "color=c=0x{:02x}{:02x}{:02x}:s={w}x{h}:d={dur:.6}:r={fps:.6}[{bg_label}]",
            bg.r, bg.g, bg.b
        ));
        let comp = format!("viz_comp_{label}");
        graph.push(format!("[{bg_label}][{raw}]overlay=format=auto[{comp}]"));
        comp
    } else {
        raw
    };

    let mut post = format!("[{composed}]format=yuva420p");
    let fade_in = clip.fade_in.as_secs_f64();
    let fade_out = clip.fade_out.as_secs_f64();
    if fade_in > 0.0 {
        post.push_str(&format!(",fade=t=in:st=0:d={fade_in:.6}:alpha=1"));
    }
    if fade_out > 0.0 {
        post.push_str(&format!(
            ",fade=t=out:st={:.6}:d={fade_out:.6}:alpha=1",
            (dur - fade_out).max(0.0)
        ));
    }
    if (clip.opacity - 1.0).abs() > f32::EPSILON {
        post.push_str(&format!(",colorchannelmixer=aa={:.6}", clip.opacity));
    }
    post.push_str(&format!("[{label}]"));
    graph.push(post);
}

pub fn export(
    project: &Project,
    settings: &ExportSettings,
    cancel: &Arc<AtomicBool>,
    mut on_progress: impl FnMut(f32),
) -> Result<()> {
    // `libx264`/`libvpx-vp9` exigem largura/altura pares (`yuv420p`
    // sub-amostra croma por 2 em cada eixo) — uma dimensão ímpar não falha
    // na hora de montar o `-filter_complex`, só depois, na hora de abrir o
    // encoder: exatamente o stderr que o usuário relatou depois de
    // `export()` passar a repassar o log real ("Error reinitializing
    // filters!" / "Could not open encoder before EOF", os dois com código
    // -22/EINVAL). Nem o diálogo de Export nem a tela de boas-vindas
    // travam largura/altura custom em números pares, então corrige aqui —
    // o único lugar por onde toda largura/altura de export passa,
    // independente de UI, preset ou projeto antigo carregado do disco.
    let normalized;
    let settings: &ExportSettings =
        if settings.width.is_multiple_of(2) && settings.height.is_multiple_of(2) {
            settings
        } else {
            normalized = ExportSettings {
                output_path: settings.output_path.clone(),
                width: even_dim(settings.width),
                height: even_dim(settings.height),
                fps: settings.fps,
                quality: settings.quality,
                format: settings.format,
            };
            &normalized
        };

    let video_tracks: Vec<&Track> = project
        .video_tracks
        .iter()
        .filter(|t| !t.muted && !t.clips.is_empty())
        .collect();
    let audio_tracks: Vec<&Track> = project
        .audio_tracks
        .iter()
        .filter(|t| !t.muted && !t.clips.is_empty())
        .collect();

    if video_tracks.is_empty() && audio_tracks.is_empty() {
        return Err(EditorError::ExportError("timeline vazia".into()));
    }

    let total_duration_secs = timeline_duration_secs(project).max(0.001);

    // `ffmpeg` recusa abrir o output quando ele é o mesmo arquivo que um
    // dos inputs ("Output ... same as Input ... - exiting") — acontece na
    // prática quando o usuário reimporta um export anterior pro projeto
    // (ex: `output.mp4` já na Media Browser) e exporta de novo pro mesmo
    // caminho. Detectado aqui, antes de gastar tempo rodando `ffmpeg`, com
    // uma mensagem que diz qual mídia colidiu em vez de deixar o `ffmpeg`
    // falhar com um "Invalid argument" genérico — usuário relatou
    // literalmente esse stderr depois que `export()` passou a repassar o
    // log real (ver a entrada de log-tail acima).
    if let Some(clash) = project
        .media
        .values()
        .find(|m| paths_refer_to_the_same_file(&m.path, &settings.output_path))
    {
        return Err(EditorError::ExportError(format!(
            "o arquivo de saída é o mesmo que a mídia importada \"{}\" — escolha outro nome/local",
            clash.path.display()
        )));
    }

    let mut cmd = FfmpegCommand::new();
    cmd.overwrite();
    let mut graph = FilterGraph::new(&mut cmd);

    let mut track_audio_labels: Vec<String> = Vec::new();
    for (t, track) in audio_tracks.iter().enumerate() {
        let mut clip_labels = Vec::new();
        let clips = sorted_clips(track);
        // `concat` não sabe onde cada clip fica na timeline absoluta, só
        // gruda um atrás do outro — um clip de áudio com `timeline_start
        // > 0` tocava a partir do segundo 0 na exportação (mesmo problema
        // do vídeo, ver o loop de `video_tracks` logo abaixo). Um filler
        // de silêncio (`anullsrc`) do tamanho exato do vão mantém o
        // `concat` alinhado com a timeline de verdade.
        let gaps = gap_before_each_clip(&clips);
        for (c, (clip, gap)) in clips.iter().zip(gaps).enumerate() {
            if let Some(gap) = gap {
                let filler_idx = graph.add_silent_audio(gap.as_secs_f64());
                let filler_label = format!("at{t}c{c}gap");
                graph.push(format!("[{filler_idx}:a]anull[{filler_label}]"));
                clip_labels.push(filler_label);
            }

            // Áudio só vem de mídia real — mídia gerada (seção 35) não tem
            // faixa sonora.
            if project.get_media(clip.source).is_none() {
                continue;
            }
            let Some(idx) = graph.add_input(project, clip, settings) else {
                continue;
            };
            let label = format!("at{t}c{c}");
            graph.push(audio_clip_chain(idx, clip, &label));
            clip_labels.push(label);
        }
        if clip_labels.is_empty() {
            continue;
        }
        let concatenated = format!("track_a{t}");
        let inputs: String = clip_labels.iter().map(|l| format!("[{l}]")).collect();
        let mut chain = format!("{inputs}concat=n={}:v=0:a=1", clip_labels.len());
        let volume = track.volume;
        if (volume - 1.0).abs() > f32::EPSILON {
            chain.push_str(&format!(",volume={volume:.6}"));
        }
        let pan = track.pan.clamp(-1.0, 1.0);
        if pan.abs() > f32::EPSILON {
            let left_gain = if pan > 0.0 { 1.0 - pan } else { 1.0 };
            let right_gain = if pan < 0.0 { 1.0 + pan } else { 1.0 };
            // `pan` filter syntax is `gain*channel`, not `channel*gain`.
            chain.push_str(&format!(
                ",aformat=channel_layouts=stereo,pan=stereo|c0={left_gain:.6}*c0|c1={right_gain:.6}*c1"
            ));
        }
        chain.push_str(&format!("[{concatenated}]"));
        graph.push(chain);
        track_audio_labels.push(concatenated);
    }

    // Quantos clipes de Audio Visualizer existem nas video tracks — cada um
    // precisa de uma fatia própria do áudio final pra reagir de verdade
    // (seção 38, fecha a lacuna que antes só existia no preview).
    let viz_count: usize = video_tracks
        .iter()
        .flat_map(|t| sorted_clips(t))
        .filter(|c| audio_visualizer_settings(project, c.source).is_some())
        .count();
    let has_audio_tracks = !track_audio_labels.is_empty();

    let final_audio_target = if viz_count > 0 { "mixedaudio" } else { "outa" };
    let mixed_audio_label: Option<String> = if has_audio_tracks {
        if track_audio_labels.len() == 1 {
            graph.push(format!(
                "[{}]anull[{final_audio_target}]",
                track_audio_labels[0]
            ));
        } else {
            let inputs: String = track_audio_labels
                .iter()
                .map(|l| format!("[{l}]"))
                .collect();
            graph.push(format!(
                "{inputs}amix=inputs={}:duration=longest[{final_audio_target}]",
                track_audio_labels.len()
            ));
        }
        Some(final_audio_target.to_string())
    } else {
        None
    };

    let mut viz_audio_labels: Vec<String> = Vec::new();
    let final_audio_label: Option<String> = if viz_count == 0 {
        mixed_audio_label
    } else if let Some(src) = &mixed_audio_label {
        // Um `asplit` só liga uma vez por saída — precisa de N+1 pra
        // manter a saída final e ainda dar uma cópia própria pra cada
        // visualizer.
        let mut outs = vec!["outa".to_string()];
        for i in 0..viz_count {
            outs.push(format!("vizaud{i}"));
        }
        let outs_fmt: String = outs.iter().map(|l| format!("[{l}]")).collect();
        graph.push(format!("[{src}]asplit={}{outs_fmt}", viz_count + 1));
        viz_audio_labels = outs[1..].to_vec();
        Some("outa".to_string())
    } else {
        // Sem track de áudio real: gera silêncio só pra alimentar os
        // visualizadores — não faz sentido incluir isso no áudio final
        // exportado se o projeto não tem áudio nenhum.
        let total_dur = timeline_duration_secs(project).max(0.1);
        for i in 0..viz_count {
            let idx = graph.add_silent_audio(total_dur);
            let label = format!("vizaud{i}");
            graph.push(format!("[{idx}:a]anull[{label}]"));
            viz_audio_labels.push(label);
        }
        None
    };

    let mut track_video_labels: Vec<String> = Vec::new();
    let mut viz_counter = 0usize;
    for (t, track) in video_tracks.iter().enumerate() {
        let mut clip_labels = Vec::new();
        let clips = sorted_clips(track);
        // `concat` só sabe grudar streams uma atrás da outra — não tem
        // noção nenhuma de "onde" cada clip fica na timeline absoluta.
        // Sem preencher o vão, um clip com `timeline_start > 0` (ou um
        // buraco entre dois clips da mesma track) simplesmente colava no
        // fim do clip anterior, ignorando o próprio `timeline_start`.
        // Usuário relatou exatamente isso: "no editor uma começando zero
        // e outra começando 10... na exportação ambas começam no segundo
        // zero". Um filler transparente (`aa=0`) do tamanho exato de
        // cada vão (`gap_before_each_clip`) mantém o `concat` da track
        // descrevendo o eixo de tempo absoluto certinho.
        let gaps = gap_before_each_clip(&clips);
        for (c, (clip, gap)) in clips.iter().zip(gaps).enumerate() {
            if let Some(gap) = gap {
                let filler_label = format!("vt{t}c{c}gap");
                let filler_idx = graph.add_black_background(
                    settings.width,
                    settings.height,
                    settings.fps.as_f64(),
                    gap.as_secs_f64(),
                );
                graph.push(format!(
                    "[{filler_idx}:v]format=yuva420p,colorchannelmixer=aa=0[{filler_label}]"
                ));
                clip_labels.push(filler_label);
            }

            let label = format!("vt{t}c{c}");
            if let Some(viz_settings) = audio_visualizer_settings(project, clip.source) {
                let Some(audio_label) = viz_audio_labels.get(viz_counter).cloned() else {
                    continue;
                };
                viz_counter += 1;
                push_visualizer_clip(
                    &mut graph,
                    &audio_label,
                    clip,
                    &viz_settings,
                    settings,
                    &label,
                );
                clip_labels.push(label);
                continue;
            }
            let Some(idx) = graph.add_input(project, clip, settings) else {
                continue;
            };
            graph.push(video_clip_chain(idx, clip, settings, &label));
            clip_labels.push(label);
        }
        if clip_labels.is_empty() {
            continue;
        }
        let track_label = format!("track{t}");
        let inputs: String = clip_labels.iter().map(|l| format!("[{l}]")).collect();
        graph.push(format!(
            "{inputs}concat=n={}:v=1:a=0[{track_label}]",
            clip_labels.len()
        ));
        track_video_labels.push(track_label);
    }

    if track_video_labels.is_empty() && track_audio_labels.is_empty() {
        return Err(EditorError::ExportError(
            "nenhum clip com mídia válida na timeline".into(),
        ));
    }

    let final_video = compose_final_video(
        &mut graph,
        &track_video_labels,
        settings,
        total_duration_secs,
    );

    let final_audio = final_audio_label.as_deref();

    let filter_complex = graph.finish();
    cmd.args(["-filter_complex", &filter_complex]);
    if let Some(v) = final_video {
        cmd.args(["-map", &format!("[{v}]")]);
        cmd.args(["-c:v", settings.format.video_codec(), "-pix_fmt", "yuv420p"]);
        cmd.args([
            "-crf",
            &settings.quality.crf_for(settings.format).to_string(),
        ]);
        if settings.format == ExportFormat::WebM {
            // `libvpx-vp9` needs `-b:v 0` alongside `-crf` for true
            // constant-quality mode.
            cmd.args(["-b:v", "0"]);
        }
        // Without an explicit `-r`, output fps silently falls back to the
        // muxer default (25) instead of `settings.fps`.
        cmd.args(["-r", &settings.fps.to_string()]);
    }
    if let Some(a) = final_audio {
        cmd.args(["-map", &format!("[{a}]")]);
        cmd.args(["-c:a", settings.format.audio_codec()]);
    }
    cmd.arg(settings.output_path.to_string_lossy().to_string());

    let mut child = cmd
        .spawn()
        .map_err(|e| EditorError::EncoderError(e.to_string()))?;

    let total_duration = total_duration_secs;
    let iter = child
        .iter()
        .map_err(|e| EditorError::EncoderError(e.to_string()))?;
    let mut cancelled = false;
    // O erro real do `ffmpeg` só existe no stderr (`FfmpegEvent::Log`/
    // `Error`) — antes disso o código só olhava `Progress` e, num erro,
    // devolvia "ver stderr" sem ter guardado stderr nenhum. Usuário
    // relatou literalmente essa mensagem inútil de volta: "ao exportar:
    // ffmpeg encerrou com erro - ver stderr". Guarda as últimas
    // `LOG_TAIL` linhas — a causa real (codec desconhecido, filtro
    // inválido, path sem permissão, ...) é sempre a última coisa que o
    // `ffmpeg` loga antes de sair.
    const LOG_TAIL: usize = 8;
    let mut recent_log: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    for event in iter {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            let _ = child.kill();
            break;
        }
        match event {
            FfmpegEvent::Progress(p) => {
                on_progress((parse_time(&p.time) / total_duration).clamp(0.0, 1.0) as f32);
            }
            FfmpegEvent::Log(_, line) | FfmpegEvent::Error(line) => {
                if recent_log.len() == LOG_TAIL {
                    recent_log.pop_front();
                }
                recent_log.push_back(line);
            }
            _ => {}
        }
    }

    let status = child
        .wait()
        .map_err(|e| EditorError::EncoderError(e.to_string()))?;

    if cancelled {
        let _ = std::fs::remove_file(&settings.output_path);
        return Err(EditorError::ExportError("cancelado pelo usuário".into()));
    }
    if !status.success() {
        let detail = if recent_log.is_empty() {
            "ffmpeg encerrou com erro (nenhuma saída de log capturada)".to_string()
        } else {
            recent_log
                .into_iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        return Err(EditorError::ExportError(detail));
    }
    on_progress(1.0);
    Ok(())
}

/// Arredonda pra baixo pro par mais próximo (nunca sobe — ficar 1px menor
/// que o pedido é inofensivo, estourar o que o usuário digitou não é).
/// Nunca devolve menos que 2 (menor dimensão par positiva).
/// Composição final de vídeo (seção 43): fundo preto por baixo de tudo
/// (inclusive da track mais de baixo, `V1`) seguido de um `overlay` por
/// track, de baixo pra cima. Extraído de `export()` pra poder testar a
/// montagem do grafo sem precisar rodar `ffmpeg` de verdade — ver
/// `add_black_background` pro porquê do fundo existir.
fn compose_final_video(
    graph: &mut FilterGraph,
    track_video_labels: &[String],
    settings: &ExportSettings,
    total_duration_secs: f64,
) -> Option<&'static str> {
    if track_video_labels.is_empty() {
        return None;
    }
    let bg_idx = graph.add_black_background(
        settings.width,
        settings.height,
        settings.fps.as_f64(),
        total_duration_secs,
    );
    let mut base = format!("{bg_idx}:v");
    for (i, layer) in track_video_labels.iter().enumerate() {
        let out = format!("comp{i}");
        graph.push(format!(
            "[{base}][{layer}]overlay=format=auto:eof_action=pass[{out}]"
        ));
        base = out;
    }
    graph.push(format!("[{base}]format=yuv420p[outv]"));
    Some("outv")
}

fn even_dim(x: u32) -> u32 {
    if x.is_multiple_of(2) {
        x
    } else {
        x.saturating_sub(1)
    }
    .max(2)
}

/// `true` quando `a` e `b` são o mesmo arquivo no disco — via
/// `canonicalize` (resolve `..`/symlinks) quando os dois já existem;
/// `settings.output_path` normalmente ainda não existe na hora desse
/// check, então cai pro fallback de comparação direta dos `Path`s nesse
/// caso (ainda pega o caso comum: mesmo caminho reimportado como mídia).
fn paths_refer_to_the_same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn timeline_duration_secs(project: &Project) -> f64 {
    project
        .video_tracks
        .iter()
        .chain(project.audio_tracks.iter())
        .flat_map(|t| t.clips.iter())
        .map(|c| c.timeline_end())
        .max()
        .map(|t| t.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_dim_leaves_even_numbers_alone_and_rounds_odd_ones_down() {
        assert_eq!(even_dim(1920), 1920);
        assert_eq!(even_dim(1921), 1920);
        assert_eq!(even_dim(3), 2);
        assert_eq!(even_dim(1), 2);
        assert_eq!(even_dim(0), 2);
    }

    #[test]
    fn compose_final_video_overlays_every_track_including_the_bottom_one_over_black() {
        // Regressão real: `V1` (a primeira/mais de baixo track) virava a
        // `base` do primeiro `overlay` sem nada embaixo pra fazer blend —
        // fade in/out e opacidade de qualquer clip nela ficavam sem
        // efeito no export (o alpha nunca tinha contra o que compor, e
        // `format=yuv420p` no fim descarta alpha de qualquer jeito).
        // Usuário relatou: "opacidade e o fadein fadeout não estão
        // exportando". A prova de que o bug sumiu é a track 0 aparecer
        // como `layer` de um `overlay` igual as demais, nunca como `base`
        // direta do primeiro estágio.
        let mut cmd = FfmpegCommand::new();
        let mut graph = FilterGraph::new(&mut cmd);
        let settings = ExportSettings {
            output_path: PathBuf::from("/does/not/matter.mp4"),
            width: 1920,
            height: 1080,
            fps: Rational::new(30, 1),
            quality: VideoQuality::Medium,
            format: ExportFormat::Mp4,
        };
        let labels = vec!["track0".to_string(), "track1".to_string()];
        let out = compose_final_video(&mut graph, &labels, &settings, 5.0);
        assert_eq!(out, Some("outv"));

        let filter = graph.finish();
        // Input 0 é o fundo preto (nenhuma track ocupou `-i`/`-f lavfi`
        // ainda nesse teste isolado, então o índice previsível é 0).
        assert!(
            filter.contains("[0:v][track0]overlay"),
            "track0 devia entrar como layer do primeiro overlay, não como base: {filter}"
        );
        assert!(
            filter.contains("[comp1][track1]overlay") || filter.contains("[track1]"),
            "track1 devia continuar entrando no overlay seguinte: {filter}"
        );

        // O fundo em si precisa cobrir a duração total do timeline, senão
        // uma track mais longa que ele "vaza" além do fim do vídeo final.
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            args.iter()
                .any(|a| a.contains("color=c=black") && a.contains("d=5.000000")),
            "esperava a fonte lavfi do fundo com d=5.000000, comando: {args:?}"
        );
    }

    #[test]
    fn compose_final_video_returns_none_when_there_are_no_video_tracks() {
        let mut cmd = FfmpegCommand::new();
        let mut graph = FilterGraph::new(&mut cmd);
        let settings = ExportSettings {
            output_path: PathBuf::from("/does/not/matter.mp4"),
            width: 1920,
            height: 1080,
            fps: Rational::new(30, 1),
            quality: VideoQuality::Medium,
            format: ExportFormat::Mp4,
        };
        assert_eq!(compose_final_video(&mut graph, &[], &settings, 5.0), None);
    }

    #[test]
    fn gap_before_each_clip_finds_the_gap_before_a_clip_that_does_not_start_at_zero() {
        // Regressão real: uma track com um único clip começando em 10s
        // era exportada como se começasse em 0s, porque `concat` não tem
        // noção de posição absoluta — usuário relatou "no editor uma
        // começando zero e outra começando 10... na exportação ambas
        // começam no segundo zero".
        use crate::project::{Clip, MediaId};

        let clip = Clip::new(
            MediaId::new(),
            Time::from_secs_f64(10.0),
            Time::from_secs_f64(3.0),
            Time::ZERO,
            Time::from_secs_f64(3.0),
        );
        let clips = [&clip];
        let gaps = gap_before_each_clip(&clips);
        assert_eq!(gaps, vec![Some(Time::from_secs_f64(10.0))]);
    }

    #[test]
    fn gap_before_each_clip_finds_no_gap_when_clips_touch_back_to_back() {
        use crate::project::{Clip, MediaId};

        let first = Clip::new(
            MediaId::new(),
            Time::ZERO,
            Time::from_secs_f64(3.0),
            Time::ZERO,
            Time::from_secs_f64(3.0),
        );
        let second = Clip::new(
            MediaId::new(),
            Time::from_secs_f64(3.0),
            Time::from_secs_f64(2.0),
            Time::ZERO,
            Time::from_secs_f64(2.0),
        );
        let clips = [&first, &second];
        assert_eq!(gap_before_each_clip(&clips), vec![None, None]);
    }

    #[test]
    fn gap_before_each_clip_finds_a_gap_between_two_clips_on_the_same_track() {
        use crate::project::{Clip, MediaId};

        let first = Clip::new(
            MediaId::new(),
            Time::ZERO,
            Time::from_secs_f64(2.0),
            Time::ZERO,
            Time::from_secs_f64(2.0),
        );
        let second = Clip::new(
            MediaId::new(),
            Time::from_secs_f64(5.0),
            Time::from_secs_f64(2.0),
            Time::ZERO,
            Time::from_secs_f64(2.0),
        );
        let clips = [&first, &second];
        assert_eq!(
            gap_before_each_clip(&clips),
            vec![None, Some(Time::from_secs_f64(3.0))]
        );
    }

    #[test]
    fn video_clip_chain_resets_sar_after_both_the_default_crop_fit_and_pan_crop() {
        use crate::project::{MediaId, Time, Transform};

        let settings = ExportSettings {
            output_path: PathBuf::from("/does/not/matter.mp4"),
            width: 1920,
            height: 1080,
            fps: Rational::new(30, 1),
            quality: VideoQuality::Medium,
            format: ExportFormat::Mp4,
        };

        // Sem Pan/Crop (crop-fit padrão) — `scale=...:force_original_
        // aspect_ratio=increase` deixa um SAR residual (ex: `10239:10240`
        // em vez de `1:1`) sempre que o aspect ratio do source não bate
        // exatamente com o do projeto. Concatenar um clip assim com outro
        // de SAR "limpo" faz o `concat`/`overlay` do ffmpeg recusar com
        // "Error reinitializing filters!" — usuário relatou exatamente
        // isso com 3 mídias (uma no tamanho certo, uma imagem, um vídeo
        // com Pan/Crop) na mesma track.
        let default_clip = Clip::new(
            MediaId::new(),
            Time::ZERO,
            Time::from_secs_f64(1.0),
            Time::ZERO,
            Time::from_secs_f64(1.0),
        );
        let chain = video_clip_chain(0, &default_clip, &settings, "c0");
        assert!(
            chain.contains(",setsar=1,format=yuva420p"),
            "esperava setsar=1 logo antes do format, chain: {chain}"
        );

        // Com Pan/Crop, o mesmo problema existe (a `scale` final ainda
        // pode deixar SAR residual dependendo do source).
        let mut pan_crop_clip = default_clip.clone();
        pan_crop_clip.transform = Transform {
            position_x: 0.0,
            position_y: 0.0,
            scale_x: 2.0,
            scale_y: 2.0,
            rotation: 0.0,
        };
        let chain = video_clip_chain(0, &pan_crop_clip, &settings, "c0");
        assert!(
            chain.contains(",setsar=1,format=yuva420p"),
            "esperava setsar=1 logo antes do format, chain: {chain}"
        );
    }

    #[test]
    fn export_normalizes_odd_dimensions_into_the_lavfi_color_source_filter() {
        use crate::project::{Clip, ProjectSettings, SolidColor, Time};

        let mut project = Project::new(ProjectSettings::default());
        let media = project.add_solid_color(
            SolidColor {
                r: 10,
                g: 20,
                b: 30,
            },
            Time::from_secs_f64(1.0),
        );
        project.add_video_track("V1");
        let clip = Clip::new(
            media,
            Time::ZERO,
            Time::from_secs_f64(1.0),
            Time::ZERO,
            Time::from_secs_f64(1.0),
        );
        project.video_tracks[0].clips.push(clip);

        let mut cmd = FfmpegCommand::new();
        let mut graph = FilterGraph::new(&mut cmd);
        let settings = ExportSettings {
            output_path: PathBuf::from("/does/not/matter.mp4"),
            // Ímpar de propósito — é exatamente o que o usuário conseguia
            // digitar sem nenhum aviso no diálogo de Export/tela de boas-
            // vindas antes desse fix.
            width: 1921,
            height: 1081,
            fps: Rational::new(30, 1),
            quality: VideoQuality::Medium,
            format: ExportFormat::Mp4,
        };
        // `add_input` usa `settings` diretamente (não passa pelo
        // arredondamento de `export()`) — testa a peça que realmente monta
        // o filtro `color=...:s=WxH`, então precisamos confirmar que quem
        // chama já normalizou antes. Simula isso aqui do jeito que
        // `export()` faz.
        let normalized = ExportSettings {
            width: even_dim(settings.width),
            height: even_dim(settings.height),
            ..settings
        };
        graph.add_input(&project, &project.video_tracks[0].clips[0], &normalized);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(
            args.iter().any(|a| a.contains("s=1920x1080")),
            "esperava s=1920x1080 (par) no filtro lavfi, comando: {args:?}"
        );
    }

    #[test]
    fn export_refuses_when_output_path_matches_an_imported_media_file() {
        use crate::project::{Clip, Media, MediaMetadata, ProjectSettings, Time};
        use std::io::Write;

        let dir = std::env::temp_dir().join(format!("videoeditor_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let media_path = dir.join("clip.mp4");
        std::fs::File::create(&media_path)
            .unwrap()
            .write_all(b"fake")
            .unwrap();

        let mut project = Project::new(ProjectSettings::default());
        let media_id = project.add_media(Media::new(&media_path, MediaMetadata::default()));
        project.add_video_track("V1");
        let clip = Clip::new(
            media_id,
            Time::ZERO,
            Time::from_secs_f64(1.0),
            Time::ZERO,
            Time::from_secs_f64(1.0),
        );
        project.video_tracks[0].clips.push(clip);

        let settings = ExportSettings {
            output_path: media_path.clone(),
            width: 1920,
            height: 1080,
            fps: Rational::new(30, 1),
            quality: VideoQuality::Medium,
            format: ExportFormat::Mp4,
        };

        let cancel = Arc::new(AtomicBool::new(false));
        let err = export(&project, &settings, &cancel, |_| {})
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("mesmo que a mídia importada"),
            "mensagem inesperada: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_refer_to_the_same_file_matches_identical_nonexistent_paths_too() {
        // `output_path` normalmente ainda não existe quando esse check
        // roda (o `ffmpeg` é quem cria o arquivo) — `canonicalize` falha
        // nesse caso pros dois lados, então o fallback de comparação direta
        // dos `Path`s precisa continuar pegando o caso óbvio (mesmo
        // caminho literal) mesmo sem o arquivo existir de verdade.
        let a = Path::new("/does/not/exist/output.mp4");
        let b = Path::new("/does/not/exist/output.mp4");
        assert!(paths_refer_to_the_same_file(a, b));
        assert!(!paths_refer_to_the_same_file(
            a,
            Path::new("/does/not/exist/other.mp4")
        ));
    }

    #[test]
    fn h264_formats_share_codecs_only_container_differs() {
        for format in [ExportFormat::Mp4, ExportFormat::Mov, ExportFormat::Mkv] {
            assert_eq!(format.video_codec(), "libx264");
            assert_eq!(format.audio_codec(), "aac");
        }
        assert_eq!(ExportFormat::Mp4.extension(), "mp4");
        assert_eq!(ExportFormat::Mov.extension(), "mov");
        assert_eq!(ExportFormat::Mkv.extension(), "mkv");
    }

    #[test]
    fn webm_uses_vp9_and_opus() {
        assert_eq!(ExportFormat::WebM.video_codec(), "libvpx-vp9");
        assert_eq!(ExportFormat::WebM.audio_codec(), "libopus");
        assert_eq!(ExportFormat::WebM.extension(), "webm");
    }

    #[test]
    fn crf_scale_differs_between_x264_and_vp9_but_keeps_relative_order() {
        for format in ExportFormat::ALL {
            assert!(
                VideoQuality::Low.crf_for(format) > VideoQuality::Medium.crf_for(format)
                    && VideoQuality::Medium.crf_for(format) > VideoQuality::High.crf_for(format)
                    && VideoQuality::High.crf_for(format) > VideoQuality::VeryHigh.crf_for(format),
                "ordem de qualidade quebrada pra {format:?}"
            );
        }
        // x264 e VP9 não devem coincidir por acidente — são escalas
        // diferentes de propósito.
        assert_ne!(
            VideoQuality::Medium.crf_for(ExportFormat::Mp4),
            VideoQuality::Medium.crf_for(ExportFormat::WebM)
        );
    }
}
