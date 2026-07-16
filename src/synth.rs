const SYNTH_GAIN: f32 = 0.90;
const SYNTH_GAIN_ATTACK: f32 = 0.02;
const SYNTH_GAIN_RELEASE: f32 = 0.001;

pub struct Synth {
    sample_rate_hz: f32,
    phase_sub: f32,
    phase_norm: f32,
    phase_sup: f32,
    level_sub: f32,
    level_norm: f32,
    level_sup: f32,
    synth_level: f32,
}

impl Synth {
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
