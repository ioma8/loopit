mod dsp;

use crate::dsp::{MIC_GAIN, PitchTracker, SynthCore};

pub struct WasmOctaveProcessor {
    tracker: PitchTracker,
    synth: SynthCore,
    last_pitch_hz: f32,
    last_pitch_clarity: f32,
}

impl WasmOctaveProcessor {
    fn new(sample_rate_hz: f32) -> Self {
        let sample_rate_hz = sample_rate_hz.max(1.0);
        Self {
            tracker: PitchTracker::new(sample_rate_hz.round() as usize),
            synth: SynthCore::new(sample_rate_hz),
            last_pitch_hz: 0.0,
            last_pitch_clarity: 0.0,
        }
    }

    fn process_sample(&mut self, sample: f32) -> f32 {
        let mono = sample * MIC_GAIN;
        self.tracker.push_sample(mono);
        self.last_pitch_hz = self.tracker.last_pitch_hz();
        self.last_pitch_clarity = self.tracker.last_pitch_clarity();
        self.synth.on_frame(self.last_pitch_hz)
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
pub extern "C" fn loopit_get_pitch_hz(ptr: *const WasmOctaveProcessor) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }

    // SAFETY: The pointer is created by loopit_new and valid for shared access here.
    let processor = unsafe { &*ptr };
    processor.last_pitch_hz
}

#[unsafe(no_mangle)]
pub extern "C" fn loopit_get_pitch_clarity(ptr: *const WasmOctaveProcessor) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }

    // SAFETY: The pointer is created by loopit_new and valid for shared access here.
    let processor = unsafe { &*ptr };
    processor.last_pitch_clarity
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
