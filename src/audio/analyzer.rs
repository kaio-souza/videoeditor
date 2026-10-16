use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

/// Tamanho da janela de FFT — potência de 2 (mais rápido pro rustfft), ~46ms
/// a 44.1kHz. Curto o bastante pra parecer responsivo, longo o bastante pra
/// resolver frequências baixas (seção 39 — bandas a partir de 20Hz).
const FFT_SIZE: usize = 2048;

pub fn frequency_bands(
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
    band_count: usize,
    sensitivity: f32,
) -> Vec<f32> {
    if channels == 0 || band_count == 0 || samples.is_empty() {
        return vec![0.0; band_count];
    }

    let frames = samples.len() / channels;
    let mut mono: Vec<f32> = (0..frames.min(FFT_SIZE))
        .map(|i| {
            let base = i * channels;
            (0..channels).map(|c| samples[base + c]).sum::<f32>() / channels as f32
        })
        .collect();
    mono.resize(FFT_SIZE, 0.0);

    // Janela de Hann — reduz vazamento espectral nas bordas do buffer.
    for (i, s) in mono.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos();
        *s *= w;
    }

    let mut buffer: Vec<Complex32> = mono.iter().map(|&s| Complex32::new(s, 0.0)).collect();
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    fft.process(&mut buffer);

    let magnitudes: Vec<f32> = buffer[..FFT_SIZE / 2]
        .iter()
        .map(|c| c.norm() / (FFT_SIZE as f32).sqrt())
        .collect();

    let min_hz = 20.0_f32;
    let max_hz = (sample_rate as f32 / 2.0).min(20_000.0);
    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;

    (0..band_count)
        .map(|band| {
            let t0 = band as f32 / band_count as f32;
            let t1 = (band + 1) as f32 / band_count as f32;
            let f0 = min_hz * (max_hz / min_hz).powf(t0);
            let f1 = min_hz * (max_hz / min_hz).powf(t1);
            let bin0 = ((f0 / bin_hz) as usize).min(magnitudes.len().saturating_sub(1));
            let bin1 = ((f1 / bin_hz) as usize).max(bin0 + 1).min(magnitudes.len());

            let peak = magnitudes[bin0..bin1]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max);

            // dB-ish compressão + sensibilidade, clampado a [0, 1].
            (peak * sensitivity * 8.0).sqrt().clamp(0.0, 1.0)
        })
        .collect()
}
