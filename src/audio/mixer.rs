use crate::media::audio_decoder::decode_pcm;
use crate::project::{Project, Time};

pub fn build_playback_buffer(
    project: &Project,
    from: Time,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    let end = timeline_duration(project);
    mix_range(project, from, end, sample_rate, channels)
}

/// Mixa uma janela curta `[center - window/2, center + window/2)` — usada
/// pelo Audio Visualizer (seção 38-39) pra analisar o que está tocando
/// perto do playhead sem decodificar a timeline inteira.
pub fn mix_window(
    project: &Project,
    center: Time,
    window_secs: f64,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    let half = Time::from_secs_f64(window_secs / 2.0);
    let start = center.saturating_sub(half);
    mix_range(project, start, center + half, sample_rate, channels)
}

/// Soma todas as tracks de áudio não-mutadas (mixer, seção 34) no intervalo
/// `[start, end)`, com volume de track (seção 33), fade (seção 30) e
/// reverse (seção 81) aplicados por clip, e satura em `[-1, 1]`.
fn mix_range(
    project: &Project,
    start: Time,
    end: Time,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    if end <= start {
        return Vec::new();
    }
    let total_secs = (end - start).as_secs_f64();
    let total_frames = (total_secs * sample_rate as f64).round() as usize;
    let mut mix = vec![0.0_f32; total_frames * channels as usize];

    for track in project.audio_tracks.iter().filter(|t| !t.muted) {
        for clip in &track.clips {
            let clip_end = clip.timeline_end();
            if clip_end <= start || clip.timeline_start >= end {
                continue;
            }
            let Some(media) = project.get_media(clip.source) else {
                continue;
            };

            let window_start = clip.timeline_start.max(start);
            let window_end = clip_end.min(end);
            let local_start = (window_start - clip.timeline_start).as_secs_f64();
            let local_duration = (window_end - window_start).as_secs_f64();
            if local_duration <= 0.0 {
                continue;
            }

            let source_start_secs = if clip.reversed {
                let source_end =
                    clip.source_start.as_secs_f64() + clip.source_duration.as_secs_f64();
                (source_end - (local_start + local_duration)).max(0.0)
            } else {
                clip.source_start.as_secs_f64() + local_start
            };

            let Ok(mut samples) = decode_pcm(
                &media.path,
                source_start_secs,
                local_duration,
                sample_rate,
                channels,
            ) else {
                continue;
            };

            if clip.reversed {
                reverse_interleaved(&mut samples, channels as usize);
            }

            apply_fade(
                &mut samples,
                channels as usize,
                sample_rate,
                local_start,
                clip.fade_in.as_secs_f64(),
                clip.fade_out.as_secs_f64(),
                clip.timeline_duration.as_secs_f64(),
            );

            let gain = track.volume * clip.volume;
            let (left_gain, right_gain) = pan_gains(track.pan);
            let offset_frames =
                ((window_start - start).as_secs_f64() * sample_rate as f64).round() as usize;
            let offset = offset_frames * channels as usize;
            for (i, sample) in samples.iter().enumerate() {
                let channel_gain = if channels == 2 {
                    if i % 2 == 0 { left_gain } else { right_gain }
                } else {
                    1.0
                };
                if let Some(slot) = mix.get_mut(offset + i) {
                    *slot += sample * gain * channel_gain;
                }
            }
        }
    }

    for sample in &mut mix {
        *sample = sample.clamp(-1.0, 1.0);
    }
    mix
}

/// Ganho por canal (esquerda, direita) a partir do balanço `pan` em
/// `[-1.0, 1.0]` — atenua linearmente o canal oposto ao lado pra onde o
/// balanço se move, mantém o canal do próprio lado em 1.0 (mesma convenção
/// do controle "Pan" de track do Sony Vegas, que é balanço, não pan de
/// mono-pra-estéreo de verdade).
/// Balance, not true mono-to-stereo panning: attenuates the opposite
/// channel, near-side channel stays at 1.0.
fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let left = if pan > 0.0 { 1.0 - pan } else { 1.0 };
    let right = if pan < 0.0 { 1.0 + pan } else { 1.0 };
    (left, right)
}

fn reverse_interleaved(samples: &mut [f32], channels: usize) {
    if channels == 0 {
        return;
    }
    let frames = samples.len() / channels;
    for i in 0..frames / 2 {
        let a = i * channels;
        let b = (frames - 1 - i) * channels;
        for c in 0..channels {
            samples.swap(a + c, b + c);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_fade(
    samples: &mut [f32],
    channels: usize,
    sample_rate: u32,
    local_start: f64,
    fade_in: f64,
    fade_out: f64,
    clip_duration: f64,
) {
    if channels == 0 || (fade_in <= 0.0 && fade_out <= 0.0) {
        return;
    }
    let frames = samples.len() / channels;
    for i in 0..frames {
        let t = local_start + i as f64 / sample_rate as f64;
        let mut gain = 1.0_f64;
        if fade_in > 0.0 && t < fade_in {
            gain = gain.min(t / fade_in);
        }
        if fade_out > 0.0 && t > clip_duration - fade_out {
            gain = gain.min(((clip_duration - t) / fade_out).max(0.0));
        }
        let gain = gain as f32;
        for c in 0..channels {
            samples[i * channels + c] *= gain;
        }
    }
}

fn timeline_duration(project: &Project) -> Time {
    project
        .video_tracks
        .iter()
        .chain(project.audio_tracks.iter())
        .flat_map(|t| t.clips.iter())
        .map(|c| c.timeline_end())
        .max()
        .unwrap_or(Time::ZERO)
}

#[cfg(test)]
mod tests {
    use super::pan_gains;

    #[test]
    fn center_pan_leaves_both_channels_untouched() {
        assert_eq!(pan_gains(0.0), (1.0, 1.0));
    }

    #[test]
    fn full_left_mutes_the_right_channel() {
        assert_eq!(pan_gains(-1.0), (1.0, 0.0));
    }

    #[test]
    fn full_right_mutes_the_left_channel() {
        assert_eq!(pan_gains(1.0), (0.0, 1.0));
    }

    #[test]
    fn out_of_range_pan_is_clamped() {
        assert_eq!(pan_gains(2.0), pan_gains(1.0));
        assert_eq!(pan_gains(-2.0), pan_gains(-1.0));
    }
}
