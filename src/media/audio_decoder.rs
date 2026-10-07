use std::io::Read;
use std::path::Path;

use ffmpeg_sidecar::command::FfmpegCommand;

use crate::error::{EditorError, Result};

/// Decodifica PCM `f32` intercalado (seção 63 — "Decode PCM") de um trecho
/// `[start, start+duration)` do arquivo, já reamostrado pro `sample_rate`/
/// `channels` pedido: `ffmpeg -ss <start> -i <path> -t <duration> -f f32le
/// -ar <sample_rate> -ac <channels> -`.
pub fn decode_pcm(
    path: &Path,
    start_secs: f64,
    duration_secs: f64,
    sample_rate: u32,
    channels: u16,
) -> Result<Vec<f32>> {
    if duration_secs <= 0.0 {
        return Ok(Vec::new());
    }

    let mut child = FfmpegCommand::new()
        .hide_banner()
        .arg("-ss")
        .arg(format!("{start_secs:.6}"))
        .input(path.to_string_lossy())
        .arg("-t")
        .arg(format!("{duration_secs:.6}"))
        .args([
            "-f",
            "f32le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-",
        ])
        .spawn()
        .map_err(|e| EditorError::DecoderError(e.to_string()))?;

    let mut stdout = child
        .take_stdout()
        .ok_or_else(|| EditorError::DecoderError("sem stdout do ffmpeg".into()))?;
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(|e| EditorError::DecoderError(e.to_string()))?;
    let _ = child.wait();

    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}
