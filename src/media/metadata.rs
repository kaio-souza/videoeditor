use std::path::Path;

use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::event::FfmpegEvent;
use ffmpeg_sidecar::metadata::FfmpegMetadata;

use crate::error::{EditorError, Result};
use crate::project::{MediaMetadata, Rational, Time};

pub fn probe(path: &Path) -> Result<MediaMetadata> {
    let mut child = FfmpegCommand::new()
        .hide_banner()
        .input(path.to_string_lossy())
        .args(["-f", "null", "-"])
        .spawn()
        .map_err(|e| EditorError::DecoderError(e.to_string()))?;

    let iter = child
        .iter()
        .map_err(|e| EditorError::DecoderError(e.to_string()))?;

    let mut metadata = FfmpegMetadata::new();
    for event in iter {
        let is_error = matches!(event, FfmpegEvent::Error(_) | FfmpegEvent::LogEOF);
        let _ = metadata.handle_event(&Some(event));
        if metadata.is_completed() || is_error {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if metadata.input_streams.is_empty() {
        return Err(EditorError::UnsupportedFormat(format!(
            "no readable streams in {}",
            path.display()
        )));
    }

    // ffmpeg reports `Duration: N/A` for stills; without this they'd map
    // to `Time::ZERO`.
    const DEFAULT_IMAGE_DURATION_SECS: f64 = 5.0;
    let duration = if super::is_image_path(path) {
        Time::from_secs_f64(DEFAULT_IMAGE_DURATION_SECS)
    } else {
        metadata
            .duration()
            .map(Time::from_secs_f64)
            .unwrap_or(Time::ZERO)
    };

    let video = metadata.input_streams.iter().find_map(|s| s.video_data());
    let audio = metadata.input_streams.iter().find_map(|s| s.audio_data());

    Ok(MediaMetadata {
        duration,
        width: video.map(|v| v.width),
        height: video.map(|v| v.height),
        fps: video.map(|v| fps_to_rational(v.fps)),
        sample_rate: audio.map(|a| a.sample_rate),
        channels: audio.map(|a| channel_layout_to_count(&a.channels)),
    })
}

fn fps_to_rational(fps: f32) -> Rational {
    // ffmpeg-sidecar já devolve o fps como float (parseado do log) — não dá
    // pra recuperar o numerador/denominador exatos originais (ex: 30000/1001)
    // só a partir disso, então aproxima com denominador 1000.
    Rational::new((fps * 1000.0).round() as i64, 1000)
}

fn channel_layout_to_count(layout: &str) -> u16 {
    match layout {
        "mono" => 1,
        "stereo" => 2,
        "5.1" | "5.1(side)" => 6,
        "7.1" => 8,
        other => other
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|s| s.parse::<u16>().ok())
            .sum::<u16>()
            .max(1),
    }
}
