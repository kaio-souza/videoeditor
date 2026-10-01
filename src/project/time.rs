//! Ver CLAUDE.md seção 11 — Sistema de tempo.
//!
//! `f32`/`f64` nunca é a representação interna: conversões repetidas
//! `Time <-> Frame` não podem acumular erro de arredondamento (seção 117 —
//! Trim/Split/Reverse/Speed/Fade todos passam por esse mapeamento). `Time`
//! guarda ticks inteiros; FPS é uma fração exata (`Rational`), não um float.

use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// Multiple of every fps this app needs exact (24/25/30/50/60, NTSC
/// 24000-60000/1001) and of common audio sample rates (44100/48000).
const TICKS_PER_SECOND: i64 = 35_280_000;

/// Timestamp com resolução de tick (~28ns). `i64` cobre bem mais que
/// qualquer timeline de vídeo precisaria, sem o erro de arredondamento de
/// um float.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Time {
    ticks: i64,
}

impl Time {
    pub const ZERO: Time = Time { ticks: 0 };

    pub const fn from_ticks(ticks: i64) -> Self {
        Self { ticks }
    }

    pub const fn ticks(self) -> i64 {
        self.ticks
    }

    /// Só pra exibição ou entrada de UI — nunca usar pra armazenar ou
    /// acumular tempo internamente, é exatamente onde o erro de ponto
    /// flutuante entraria.
    pub fn from_secs_f64(secs: f64) -> Self {
        Self {
            ticks: (secs * TICKS_PER_SECOND as f64).round() as i64,
        }
    }

    /// Só pra exibição — ver `from_secs_f64`.
    pub fn as_secs_f64(self) -> f64 {
        self.ticks as f64 / TICKS_PER_SECOND as f64
    }

    /// Converte um número de frame (0-based) em `Time`, dado um `fps`.
    pub fn from_frame(frame: i64, fps: Rational) -> Self {
        let ticks = (frame as i128 * fps.denominator as i128 * TICKS_PER_SECOND as i128)
            / fps.numerator as i128;
        Self {
            ticks: ticks as i64,
        }
    }

    /// Converte pro número do frame (0-based) que contém esse instante —
    /// trunca, não arredonda (mesma convenção de qualquer NLE: o frame
    /// "atual" é o que já começou).
    pub fn to_frame(self, fps: Rational) -> i64 {
        ((self.ticks as i128 * fps.numerator as i128)
            / (fps.denominator as i128 * TICKS_PER_SECOND as i128)) as i64
    }

    // Defensivos contra overflow de `i64` — nenhum caminho de código
    // atingiu isso ainda (timelines de horas não passam nem perto de
    // `i64::MAX` ticks), mas é a API certa pra quando surgir.
    #[allow(dead_code)]
    pub fn checked_add(self, rhs: Time) -> Option<Time> {
        self.ticks.checked_add(rhs.ticks).map(Time::from_ticks)
    }

    #[allow(dead_code)]
    pub fn checked_sub(self, rhs: Time) -> Option<Time> {
        self.ticks.checked_sub(rhs.ticks).map(Time::from_ticks)
    }

    #[allow(dead_code)]
    pub fn saturating_sub(self, rhs: Time) -> Time {
        Time::from_ticks(self.ticks.saturating_sub(rhs.ticks))
    }
}

impl Add for Time {
    type Output = Time;
    fn add(self, rhs: Time) -> Time {
        Time::from_ticks(self.ticks + rhs.ticks)
    }
}

impl Sub for Time {
    type Output = Time;
    fn sub(self, rhs: Time) -> Time {
        Time::from_ticks(self.ticks - rhs.ticks)
    }
}

impl AddAssign for Time {
    fn add_assign(&mut self, rhs: Time) {
        self.ticks += rhs.ticks;
    }
}

impl SubAssign for Time {
    fn sub_assign(&mut self, rhs: Time) {
        self.ticks -= rhs.ticks;
    }
}

impl fmt::Display for Time {
    /// `HH:MM:SS.mmm`, como no exemplo da seção 11.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ticks_per_milli = TICKS_PER_SECOND / 1000;
        let total_millis = self.ticks.div_euclid(ticks_per_milli);
        let millis = total_millis.rem_euclid(1000);
        let total_secs = total_millis.div_euclid(1000);
        let secs = total_secs.rem_euclid(60);
        let total_mins = total_secs.div_euclid(60);
        let mins = total_mins.rem_euclid(60);
        let hours = total_mins.div_euclid(60);
        write!(f, "{hours:02}:{mins:02}:{secs:02}.{millis:03}")
    }
}

/// Fração exata (`30/1`, `30000/1001` pra NTSC, ...) — usada pro FPS pra não
/// carregar o arredondamento de um float pelo pipeline inteiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    pub numerator: i64,
    pub denominator: i64,
}

impl Rational {
    pub const fn new(numerator: i64, denominator: i64) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denominator == 1 {
            write!(f, "{}", self.numerator)
        } else {
            write!(f, "{}/{}", self.numerator, self.denominator)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS_30: Rational = Rational::new(30, 1);
    const FPS_25: Rational = Rational::new(25, 1);
    const FPS_24: Rational = Rational::new(24, 1);
    const FPS_60: Rational = Rational::new(60, 1);
    const FPS_NTSC_24: Rational = Rational::new(24_000, 1_001);
    const FPS_NTSC_30: Rational = Rational::new(30_000, 1_001);
    const FPS_NTSC_60: Rational = Rational::new(60_000, 1_001);

    #[test]
    fn represents_exact_fractional_seconds() {
        // Seção 11: 00:00:00.000 / 00:00:00.500 / 00:00:01.000
        assert_eq!(Time::ZERO.to_string(), "00:00:00.000");
        assert_eq!(Time::from_secs_f64(0.5).to_string(), "00:00:00.500");
        assert_eq!(Time::from_secs_f64(1.0).to_string(), "00:00:01.000");
    }

    #[test]
    fn display_formats_hours_minutes_seconds_millis() {
        let t = Time::from_secs_f64(3661.25); // 1h01m01.250s
        assert_eq!(t.to_string(), "01:01:01.250");
    }

    #[test]
    fn frame_round_trip_is_exact_for_common_fps() {
        for fps in [
            FPS_30,
            FPS_25,
            FPS_24,
            FPS_60,
            FPS_NTSC_24,
            FPS_NTSC_30,
            FPS_NTSC_60,
        ] {
            for frame in [0, 1, 29, 30, 12345] {
                let t = Time::from_frame(frame, fps);
                assert_eq!(
                    t.to_frame(fps),
                    frame,
                    "round-trip falhou pra frame {frame} @ {fps}"
                );
            }
        }
    }

    #[test]
    fn frame_zero_is_zero_time() {
        assert_eq!(Time::from_frame(0, FPS_30), Time::ZERO);
    }

    #[test]
    fn one_second_at_30fps_is_frame_30() {
        assert_eq!(Time::from_secs_f64(1.0).to_frame(FPS_30), 30);
    }

    #[test]
    fn arithmetic_is_exact_no_drift() {
        // Soma a duração de 1 frame @ 30fps (não representável em decimal
        // exato) 300 vezes — não pode acumular erro: 300 frames @ 30fps = 10s.
        let one_frame = Time::from_frame(1, FPS_30) - Time::from_frame(0, FPS_30);
        let mut acc = Time::ZERO;
        for _ in 0..300 {
            acc += one_frame;
        }
        assert_eq!(acc, Time::from_secs_f64(10.0));
    }

    #[test]
    fn saturating_sub_never_panics_on_overflow() {
        let one_second = Time::from_secs_f64(1.0);
        assert_eq!(
            Time::ZERO.saturating_sub(one_second),
            Time::from_ticks(-TICKS_PER_SECOND)
        );
    }

    #[test]
    fn rational_display() {
        assert_eq!(Rational::new(30, 1).to_string(), "30");
        assert_eq!(Rational::new(30_000, 1_001).to_string(), "30000/1001");
    }
}
