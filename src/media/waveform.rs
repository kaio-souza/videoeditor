use std::path::Path;

use crate::media::audio_decoder::decode_pcm;

/// Número fixo de picos por clip — a timeline estica esse vetor pro tamanho
/// do retângulo do clip na tela, independente do zoom (seção 46).
const PEAK_COUNT: usize = 200;
const SAMPLE_RATE: u32 = 8_000;

/// Picos de amplitude (`0..=1`) de um trecho `[start, start+duration)` do
/// arquivo, downsampled pra `PEAK_COUNT` buckets — usado pra desenhar a
/// forma de onda na timeline (seção 46). Taxa de amostragem baixa (8kHz
/// mono) de propósito: só interessa o envelope de amplitude, não fidelidade
/// de áudio.
pub fn peaks(path: &Path, start_secs: f64, duration_secs: f64) -> Vec<f32> {
    let Ok(samples) = decode_pcm(path, start_secs, duration_secs, SAMPLE_RATE, 1) else {
        return vec![0.0; PEAK_COUNT];
    };
    if samples.is_empty() {
        return vec![0.0; PEAK_COUNT];
    }

    let bucket_size = (samples.len() / PEAK_COUNT).max(1);
    (0..PEAK_COUNT)
        .map(|i| {
            let start = i * bucket_size;
            let end = (start + bucket_size).min(samples.len());
            if start >= end {
                return 0.0;
            }
            samples[start..end]
                .iter()
                .fold(0.0_f32, |acc, s| acc.max(s.abs()))
        })
        .collect()
}
