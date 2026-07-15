const SMOOTHING_ALPHA: f32 = 0.18;
const MIC_GAIN: f32 = 1.5;
const LIMIT_CEILING: f32 = 0.95;
const HP_CUTOFF_HZ: f32 = 60.0;
const LP_CUTOFF_HZ: f32 = 420.0;
const GATE_THRESHOLD: f32 = 0.008;
const GATE_FLOOR: f32 = 0.02;
const GATE_ATTACK: f32 = 0.20;
const GATE_RELEASE: f32 = 0.01;
const GATE_SMOOTH: f32 = 0.08;
const ZC_THRESHOLD: f32 = 0.001;

fn limit_sample(sample: f32) -> f32 {
    let magnitude = sample.abs();

    if magnitude > LIMIT_CEILING {
        sample * (LIMIT_CEILING / magnitude)
    } else {
        sample
    }
}

fn high_pass(sample: f32, previous_input: &mut f32, previous_output: &mut f32, alpha: f32) -> f32 {
    let output = alpha * (*previous_output + sample - *previous_input);
    *previous_input = sample;
    *previous_output = output;
    output
}

fn smooth_gate(level: f32, envelope: &mut f32, gain: &mut f32) -> f32 {
    let env_coeff = if level > *envelope {
        GATE_ATTACK
    } else {
        GATE_RELEASE
    };
    *envelope += env_coeff * (level - *envelope);

    let target_gain = if *envelope < GATE_THRESHOLD {
        GATE_FLOOR
    } else {
        1.0
    };
    *gain += GATE_SMOOTH * (target_gain - *gain);

    *gain
}

pub struct WasmOctaveProcessor {
    smoothed: f32,
    hp_prev_x: f32,
    hp_prev_y: f32,
    hp_alpha: f32,
    lp_state: f32,
    lp_alpha: f32,
    gate_env: f32,
    gate_gain: f32,
    zc_prev: f32,
    crossing_armed: bool,
    polarity: f32,
}

impl WasmOctaveProcessor {
    fn new(sample_rate_hz: f32) -> Self {
        let dt = 1.0 / sample_rate_hz.max(1.0);
        let hp_rc = 1.0 / (2.0 * std::f32::consts::PI * HP_CUTOFF_HZ);
        let hp_alpha = hp_rc / (hp_rc + dt);
        let lp_rc = 1.0 / (2.0 * std::f32::consts::PI * LP_CUTOFF_HZ);
        let lp_alpha = dt / (lp_rc + dt);

        Self {
            smoothed: 0.0,
            hp_prev_x: 0.0,
            hp_prev_y: 0.0,
            hp_alpha,
            lp_state: 0.0,
            lp_alpha,
            gate_env: 0.0,
            gate_gain: 1.0,
            zc_prev: 0.0,
            crossing_armed: true,
            polarity: 1.0,
        }
    }

    fn process_sample(&mut self, sample: f32) -> f32 {
        let mono = sample * MIC_GAIN;
        let high_passed = high_pass(mono, &mut self.hp_prev_x, &mut self.hp_prev_y, self.hp_alpha);
        self.lp_state += self.lp_alpha * (high_passed - self.lp_state);
        let vocal_band = self.lp_state;
        let gate_gain = smooth_gate(vocal_band.abs(), &mut self.gate_env, &mut self.gate_gain);
        let cleaned = vocal_band * gate_gain;

        if cleaned <= -ZC_THRESHOLD {
            self.crossing_armed = true;
        }

        if self.crossing_armed && self.zc_prev <= -ZC_THRESHOLD && cleaned >= ZC_THRESHOLD {
            self.polarity = -self.polarity;
            self.crossing_armed = false;
        }
        self.zc_prev = cleaned;

        let divided = cleaned * self.polarity;
        self.smoothed += SMOOTHING_ALPHA * (divided - self.smoothed);
        limit_sample(self.smoothed)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn loopit_new(sample_rate_hz: f32) -> *mut WasmOctaveProcessor {
    let processor = WasmOctaveProcessor::new(sample_rate_hz);
    Box::into_raw(Box::new(processor))
}

#[unsafe(no_mangle)]
pub extern "C" fn loopit_process(ptr: *mut WasmOctaveProcessor, sample: f32) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }

    // SAFETY: The pointer is created by loopit_new and owned by caller until loopit_free.
    let processor = unsafe { &mut *ptr };
    processor.process_sample(sample)
}

#[unsafe(no_mangle)]
pub extern "C" fn loopit_reset(ptr: *mut WasmOctaveProcessor, sample_rate_hz: f32) {
    if ptr.is_null() {
        return;
    }

    // SAFETY: The pointer is created by loopit_new and valid for mutable access here.
    let processor = unsafe { &mut *ptr };
    *processor = WasmOctaveProcessor::new(sample_rate_hz);
}

#[unsafe(no_mangle)]
pub extern "C" fn loopit_free(ptr: *mut WasmOctaveProcessor) {
    if ptr.is_null() {
        return;
    }

    // SAFETY: Pointer was allocated by Box::into_raw in loopit_new.
    unsafe {
        drop(Box::from_raw(ptr));
    }
}
