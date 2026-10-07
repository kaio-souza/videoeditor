use std::path::Path;

use ffmpeg_sidecar::command::FfmpegCommand;

use crate::error::{EditorError, Result};
use crate::project::{Time, Transform};

pub struct DecodedFrame {
    pub rgb: Vec<u8>,
}

/// Decodifica um único frame de vídeo no instante `time` do arquivo em
/// `path`, já escalado pra `size = (width, height)` — todas as camadas da
/// composição (seção 9/43) precisam sair do mesmo tamanho pra dar blend.
///
/// Sem Pan/Crop (`transform` identidade): `scale=WxH:
/// force_original_aspect_ratio=increase,crop=WxH` — preenche o frame do
/// tamanho do projeto sem distorcer (estilo "Crop" do Vegas), mesmo default
/// de sempre. **Com** Pan/Crop: usa só a região exata que o usuário
/// escolheu no diálogo (seção 19, `Transform::crop_filter_expr`, direto no
/// source original — `iw`/`ih` cru, não uma versão já pré-cortada) — as
/// duas coisas não se empilham. Empilhar (crop-fit primeiro, Pan/Crop
/// depois) foi uma primeira tentativa real que quebrou fontes com aspect
/// ratio diferente do projeto (ex: foto 9:16 num projeto 16:9): o crop-fit
/// já descartava as bordas antes do Pan/Crop nem ver o frame inteiro, então
/// a região que sobrava pra escolher já vinha cortada — usuário relatou
/// "não tenho a imagem inteira pra definir o que eu quero, o zoom fica
/// parcial no que já esticou".
pub fn decode_frame_at_scaled(
    path: &Path,
    time: Time,
    size: (u32, u32),
    transform: Transform,
) -> Result<DecodedFrame> {
    let (w, h) = size;
    let vf = match transform.crop_filter_expr() {
        Some(crop) => format!("{crop},scale={w}:{h}"),
        None => format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"),
    };
    decode_with_filter(path, time, &vf)
}

/// Decodifica o source inteiro sem cortar nada, encaixado (letterbox —
/// barras pretas, não crop) dentro de `canvas`, pro diálogo de Pan/Crop
/// mostrar o frame completo (seção 19) — é o que permite escolher uma
/// região que o crop-fit padrão de `decode_frame_at_scaled` teria
/// descartado. `pad` garante um buffer de saída sempre do tamanho exato de
/// `canvas`, então o resto do pipeline de leitura do rawvideo não muda.
pub fn decode_frame_letterboxed(
    path: &Path,
    time: Time,
    canvas: (u32, u32),
) -> Result<DecodedFrame> {
    let (w, h) = canvas;
    let vf = format!(
        "scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black"
    );
    decode_with_filter(path, time, &vf)
}

fn decode_with_filter(path: &Path, time: Time, vf: &str) -> Result<DecodedFrame> {
    let mut cmd = FfmpegCommand::new();
    cmd.hide_banner();
    // Seeking a still image past its one frame yields zero decoded frames.
    if !crate::media::is_image_path(path) {
        cmd.arg("-ss")
            .arg(format!("{:.6}", time.as_secs_f64().max(0.0)));
    }
    let mut child = cmd
        .input(path.to_string_lossy())
        .args(["-vframes", "1", "-vf", vf])
        .rawvideo()
        .spawn()
        .map_err(|e| EditorError::DecoderError(e.to_string()))?;

    let frame = child
        .iter()
        .map_err(|e| EditorError::DecoderError(e.to_string()))?
        .filter_frames()
        .next();

    let _ = child.wait();

    let frame = frame.ok_or_else(|| {
        EditorError::DecoderError(format!("no frame decoded from {}", path.display()))
    })?;

    Ok(DecodedFrame { rgb: frame.data })
}
