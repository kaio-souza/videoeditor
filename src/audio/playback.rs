use std::num::NonZero;

/// Saída de áudio do sistema (seção 12 — Audio Playback), aberta uma vez e
/// mantida viva pelo processo inteiro — soltar o `MixerDeviceSink` corta o
/// som. `None` em qualquer parte (sem dispositivo de áudio disponível) só
/// significa "sem som", nunca crash (seção 113).
pub struct AudioEngine {
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
    sample_rate: u32,
    channels: u16,
}

impl AudioEngine {
    pub fn new() -> Option<Self> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        let sample_rate = sink.config().sample_rate().get();
        let channels = sink.config().channel_count().get();
        let player = rodio::Player::connect_new(sink.mixer());
        Some(Self {
            _sink: sink,
            player,
            sample_rate,
            channels,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Troca o que está tocando pelo buffer inteiro e começa a reproduzir.
    pub fn play_buffer(&self, samples: Vec<f32>) {
        let (Some(channels), Some(sample_rate)) =
            (NonZero::new(self.channels), NonZero::new(self.sample_rate))
        else {
            return;
        };
        self.player.clear();
        self.player.append(rodio::buffer::SamplesBuffer::new(
            channels,
            sample_rate,
            samples,
        ));
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn stop(&self) {
        self.player.stop();
    }
}
