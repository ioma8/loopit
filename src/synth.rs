use crate::dsp::SynthCore;

pub struct Synth {
    core: SynthCore,
}

impl Synth {
    pub fn new(sample_rate_hz: f32) -> Self {
        Self {
            core: SynthCore::new(sample_rate_hz),
        }
    }

    pub fn on_frame(&mut self, pitch: f32) -> f32 {
        self.core.on_frame(pitch)
    }
}
