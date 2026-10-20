//! Bounded stereo queues. WASAPI converts endpoint formats to 48 kHz float;
//! fractional reads track independent device clocks without growing latency.
use std::{collections::VecDeque, sync::OnceLock};

pub const RATE: usize = 48_000;
const TARGET: usize = RATE / 50;
const CAPACITY: usize = RATE / 5;
const TAPS: usize = 64;
const PHASES: usize = 256;
static KERNEL: OnceLock<[[f32; TAPS]; PHASES]> = OnceLock::new();

fn kernel() -> &'static [[f32; TAPS]; PHASES] {
    KERNEL.get_or_init(|| {
        std::array::from_fn(|phase| {
            let mut weights = std::array::from_fn(|tap| {
                let x = tap as f64 - 31.0 - phase as f64 / PHASES as f64;
                let sinc = if x.abs() < 1e-12 {
                    1.0
                } else {
                    (std::f64::consts::PI * 0.98 * x).sin() / (std::f64::consts::PI * 0.98 * x)
                };
                let window = 0.42
                    + 0.5 * (std::f64::consts::PI * x / 32.0).cos()
                    + 0.08 * (std::f64::consts::TAU * x / 32.0).cos();
                (0.98 * sinc * window) as f32
            });
            let sum: f32 = weights.iter().sum();
            weights.iter_mut().for_each(|weight| *weight /= sum);
            weights
        })
    })
}

pub struct ClockBuffer {
    frames: VecDeque<[f32; 2]>,
    phase: f64,
    started: bool,
    kernel: &'static [[f32; TAPS]; PHASES],
    pub dropped: u64,
    pub underruns: u64,
}
impl ClockBuffer {
    pub fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(CAPACITY),
            phase: 0.0,
            started: false,
            kernel: kernel(),
            dropped: 0,
            underruns: 0,
        }
    }
    pub fn clear(&mut self) {
        self.frames.clear();
        self.phase = 0.0;
        self.started = false;
    }
    pub fn push(&mut self, frame: [f32; 2]) {
        if self.frames.len() == CAPACITY {
            self.frames.pop_front();
            self.dropped += 1;
        }
        self.frames.push_back(frame.map(|v| {
            if v.is_finite() {
                v.clamp(-8.0, 8.0)
            } else {
                0.0
            }
        }));
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    #[cfg(target_os = "windows")]
    pub fn peak(&self) -> f32 {
        self.frames
            .iter()
            .flatten()
            .fold(0.0_f32, |peak, value| peak.max(value.abs()))
    }
    pub fn next(&mut self) -> [f32; 2] {
        if !self.started {
            if self.frames.len() < TARGET {
                return [0.0; 2];
            }
            self.started = true;
        }
        if self.frames.len() < TAPS + 2 {
            self.underruns += 1;
            self.clear();
            return [0.0; 2];
        }
        // Normally +/-500 ppm; catch up faster after scheduler stalls. Bounds
        // prevent a runaway input clock or a delayed device accumulating audio.
        let correction = ((self.frames.len() as f64 - TARGET as f64) / TARGET as f64 * 0.002)
            .clamp(-0.005, 0.005);
        // Precomputed windowed-sinc interpolation avoids the high-frequency
        // attenuation of linear interpolation while tracking device clocks.
        // Initialization happens before streaming; reads allocate nothing.
        let weights = &self.kernel[(self.phase * PHASES as f64) as usize % PHASES];
        let mut out = [0.0; 2];
        for (tap, weight) in weights.iter().enumerate() {
            out[0] += self.frames[tap][0] * weight;
            out[1] += self.frames[tap][1] * weight;
        }
        self.phase += 1.0 + correction;
        while self.phase >= 1.0 && !self.frames.is_empty() {
            self.frames.pop_front();
            self.phase -= 1.0;
        }
        out
    }
}

/// Stereo-linked fast attack / slow release limiter preserves stereo balance.
pub struct Mixer {
    envelope: f32,
}
impl Mixer {
    pub fn new() -> Self {
        Self { envelope: 1.0 }
    }
    pub fn mix(
        &mut self,
        music: [f32; 2],
        mic: [f32; 2],
        music_gain: f32,
        mic_gain: f32,
    ) -> ([f32; 2], bool) {
        let raw = std::array::from_fn::<_, 2, _>(|ch| music[ch] * music_gain + mic[ch] * mic_gain);
        let peak = raw[0].abs().max(raw[1].abs());
        let desired = if peak > 0.98 { 0.98 / peak } else { 1.0 };
        self.envelope = desired.min(self.envelope + 0.0002);
        (raw.map(|v| v * self.envelope), peak > 0.98)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_queue_clears_stale_voice_and_rejects_nan() {
        let mut buffer = ClockBuffer::new();
        for _ in 0..CAPACITY + 500 {
            buffer.push([f32::NAN, 0.5]);
        }
        assert_eq!(buffer.len(), CAPACITY);
        assert_eq!(buffer.dropped, 500);
        assert_eq!(buffer.next()[0], 0.0);
        buffer.clear();
        assert_eq!(buffer.next(), [0.0; 2]);
    }
    #[test]
    fn limiter_preserves_stereo_and_mic_can_be_completely_absent() {
        let mut mixer = Mixer::new();
        let (quiet, limited) = mixer.mix([0.2, -0.1], [1.0; 2], 1.0, 0.0);
        assert_eq!(quiet, [0.2, -0.1]);
        assert!(!limited);
        let (loud, limited) = mixer.mix([1.0, 0.5], [1.0, 0.5], 1.0, 2.0);
        assert!(limited);
        assert!(loud[0] <= 0.980001);
        assert!((loud[1] / loud[0] - 0.5).abs() < 0.0001);
    }
    #[test]
    fn mismatched_clocks_remain_bounded_for_a_long_session() {
        for slow in [false, true] {
            let mut buffer = ClockBuffer::new();
            for _ in 0..TARGET {
                buffer.push([0.25; 2]);
            }
            for tick in 0..30_000 {
                let frames = if slow {
                    480 - usize::from(tick % 4 == 0)
                } else {
                    480 + usize::from(tick % 4 == 0)
                };
                for _ in 0..frames {
                    buffer.push([0.25; 2]);
                }
                for _ in 0..480 {
                    assert!((buffer.next()[0] - 0.25).abs() < 0.0001);
                }
            }
            assert!(buffer.len() < TARGET * 3);
            assert_eq!(buffer.underruns, 0);
            assert_eq!(buffer.dropped, 0);
        }
    }

    #[test]
    fn drift_interpolation_preserves_music_band_without_high_frequency_loss() {
        for weights in kernel() {
            for frequency in [1000.0, 10_000.0, 18_000.0, 20_000.0] {
                let mut real = 0.0;
                let mut imaginary = 0.0;
                for (tap, weight) in weights.iter().enumerate() {
                    let angle = std::f64::consts::TAU * frequency / RATE as f64 * tap as f64;
                    real += *weight as f64 * angle.cos();
                    imaginary += *weight as f64 * angle.sin();
                }
                assert!((real.hypot(imaginary) - 1.0).abs() < 0.001);
            }
        }
    }
}
