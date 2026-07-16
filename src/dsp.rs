use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

pub const MIC_GAIN: f32 = 2.0;
pub const PITCH_WINDOW_SIZE: usize = 1024;
pub const PITCH_WINDOW_HOP: usize = 512;
pub const POWER_THRESHOLD: f64 = 0.1;
pub const CLARITY_THRESHOLD: f64 = 0.4;

const SYNTH_GAIN: f32 = 0.90;
const SYNTH_GAIN_ATTACK: f32 = 0.02;
const SYNTH_GAIN_RELEASE: f32 = 0.001;

pub struct PitchTracker {
    sample_rate_hz: usize,
    detector: McLeodDetector<f64>,
    analysis_ring: Vec<f64>,
    write_pos: usize,
    filled: usize,
    samples_since_hop: usize,
    last_pitch_hz: f32,
    last_pitch_clarity: f32,
}

impl PitchTracker {
    pub fn new(sample_rate_hz: usize) -> Self {
        Self {
            sample_rate_hz,
            detector: McLeodDetector::<f64>::new(PITCH_WINDOW_SIZE, PITCH_WINDOW_SIZE / 2),
            analysis_ring: vec![0.0_f64; PITCH_WINDOW_SIZE * 2],
            write_pos: 0,
            filled: 0,
            samples_since_hop: 0,
            last_pitch_hz: 0.0,
            last_pitch_clarity: 0.0,
        }
    }

    pub fn push_sample(&mut self, sample: f32) {
        let sample = sample as f64;
        self.analysis_ring[self.write_pos] = sample;
        self.analysis_ring[self.write_pos + PITCH_WINDOW_SIZE] = sample;

        self.write_pos += 1;
        if self.write_pos == PITCH_WINDOW_SIZE {
            self.write_pos = 0;
        }

        if self.filled < PITCH_WINDOW_SIZE {
            self.filled += 1;
        }

        self.samples_since_hop += 1;

        while self.filled == PITCH_WINDOW_SIZE && self.samples_since_hop >= PITCH_WINDOW_HOP {
            let window = &self.analysis_ring[self.write_pos..self.write_pos + PITCH_WINDOW_SIZE];

            if let Some(pitch) = self.detector.get_pitch(
                window,
                self.sample_rate_hz,
                POWER_THRESHOLD,
                CLARITY_THRESHOLD,
            ) {
                self.last_pitch_hz = pitch.frequency as f32;
                self.last_pitch_clarity = pitch.clarity as f32;
            } else {
                self.last_pitch_hz = 0.0;
                self.last_pitch_clarity = 0.0;
            }

            self.samples_since_hop -= PITCH_WINDOW_HOP;
        }
    }

    pub fn last_pitch_hz(&self) -> f32 {
        self.last_pitch_hz
    }

    pub fn last_pitch_clarity(&self) -> f32 {
        self.last_pitch_clarity
    }
}

pub struct SynthCore {
    sample_rate_hz: f32,
    phase_sub: f32,
    phase_norm: f32,
    phase_sup: f32,
    level_sub: f32,
    level_norm: f32,
    level_sup: f32,
    synth_level: f32,
}

impl SynthCore {
    pub fn new(sample_rate_hz: f32) -> Self {
        Self {
            sample_rate_hz,
            phase_sub: 0.0,
            phase_norm: 0.0,
            phase_sup: 0.0,
            level_sub: 1.2,
            level_norm: 0.5,
            level_sup: 0.0,
            synth_level: 0.0,
        }
    }

    pub fn on_frame(&mut self, pitch: f32) -> f32 {
        let has_pitch = pitch > 0.0 && pitch.is_finite();

        if has_pitch {
            self.synth_level += SYNTH_GAIN_ATTACK * (SYNTH_GAIN - self.synth_level);
        } else {
            self.synth_level += SYNTH_GAIN_RELEASE * (0.0 - self.synth_level);
        }

        self.phase_sub += (2.0 * std::f32::consts::PI * pitch * 0.5) / self.sample_rate_hz;
        if self.phase_sub > std::f32::consts::TAU {
            self.phase_sub -= std::f32::consts::TAU;
        }

        self.phase_norm += (2.0 * std::f32::consts::PI * pitch) / self.sample_rate_hz;
        if self.phase_norm > std::f32::consts::TAU {
            self.phase_norm -= std::f32::consts::TAU;
        }

        self.phase_sup += (2.0 * std::f32::consts::PI * pitch * 2.0) / self.sample_rate_hz;
        if self.phase_sup > std::f32::consts::TAU {
            self.phase_sup -= std::f32::consts::TAU;
        }

        ((self.phase_sub.sin() * self.synth_level * self.level_sub)
            + (self.phase_norm.sin() * self.synth_level * self.level_norm)
            + (self.phase_sup.sin() * self.synth_level * self.level_sup))
            / (self.level_sub + self.level_norm + self.level_sup)
    }
}