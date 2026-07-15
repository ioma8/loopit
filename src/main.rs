use std::{
    thread::sleep,
    time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};

const TARGET_MAX_LATENCY_MS: f32 = 9.0;
const BUFFER_FRAMES: u32 = 32;
const EXTRA_QUEUE_FRAMES: usize = 32;
const BUFFER_CAPACITY: usize = BUFFER_FRAMES as usize + EXTRA_QUEUE_FRAMES;
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

fn mix_to_mono(frame: &[f32], input_channels: usize) -> f32 {
    frame.iter().copied().sum::<f32>() / input_channels as f32
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

struct InputProcessor {
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

impl InputProcessor {
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

    fn ingest_input(
        &mut self,
        data: &[f32],
        input_channels: usize,
        producer: &mut Producer<f32>,
    ) {
        if input_channels == 0 {
            return;
        }

        for frame in data.chunks_exact(input_channels) {
            let mono = mix_to_mono(frame, input_channels) * MIC_GAIN;
            let sample = self.process_sample(mono);
            let _ = producer.push(sample);
        }
    }

    fn process_sample(&mut self, sample: f32) -> f32 {
        let high_passed = high_pass(sample, &mut self.hp_prev_x, &mut self.hp_prev_y, self.hp_alpha);
        self.lp_state += self.lp_alpha * (high_passed - self.lp_state);
        let vocal_band = self.lp_state;
        let gate_gain = smooth_gate(vocal_band.abs(), &mut self.gate_env, &mut self.gate_gain);
        let cleaned = vocal_band * gate_gain;

        if cleaned <= -ZC_THRESHOLD {
            self.crossing_armed = true;
        }

        if self.crossing_armed && self.zc_prev <= -ZC_THRESHOLD && cleaned >= ZC_THRESHOLD {
            // Octave-divider trick: flip polarity once per positive-going crossing.
            self.polarity = -self.polarity;
            self.crossing_armed = false;
        }
        self.zc_prev = cleaned;

        let divided = cleaned * self.polarity;
        self.smoothed += SMOOTHING_ALPHA * (divided - self.smoothed);
        limit_sample(self.smoothed)
    }
}

fn main() {
    let host = cpal::default_host();

    let input_device = host
        .default_input_device()
        .expect("no input device available");

    let output_device = host
        .default_output_device()
        .expect("no output device available");

    let input_supported_config = input_device
        .default_input_config()
        .expect("failed to get default input config");

    let output_supported_config = output_device
        .default_output_config()
        .expect("failed to get default output config");

    println!("Input config: {:?}", input_supported_config);
    println!("Output config: {:?}", output_supported_config);

    if input_supported_config.sample_format() != cpal::SampleFormat::F32
        || output_supported_config.sample_format() != cpal::SampleFormat::F32
    {
        panic!(
            "this example expects f32 input/output, got input={:?}, output={:?}",
            input_supported_config.sample_format(),
            output_supported_config.sample_format()
        );
    }

    let mut input_config = input_supported_config.config();
    let mut output_config = output_supported_config.config();

    input_config.buffer_size = cpal::BufferSize::Fixed(BUFFER_FRAMES);
    output_config.buffer_size = cpal::BufferSize::Fixed(BUFFER_FRAMES);

    let input_channels = usize::from(input_config.channels);
    let output_channels = usize::from(output_config.channels);

    let sample_rate_hz = input_config.sample_rate as f32;
    let estimated_latency_ms =
        ((BUFFER_FRAMES as f32 * 2.0) + BUFFER_CAPACITY as f32) * 1000.0 / sample_rate_hz;
    println!(
        "Estimated pipeline latency: {:.2} ms (target < {:.2} ms)",
        estimated_latency_ms, TARGET_MAX_LATENCY_MS
    );

    let (mut producer, mut consumer): (Producer<f32>, Consumer<f32>) =
        RingBuffer::new(BUFFER_CAPACITY);
    let mut input_processor = InputProcessor::new(sample_rate_hz);
    let mut last_output_sample = 0.0_f32;

    let input_stream = input_device
        .build_input_stream(
            input_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                input_processor.ingest_input(data, input_channels, &mut producer);
            },
            move |err| {
                eprintln!("input stream error: {err}");
            },
            None,
        )
        .expect("failed to build input stream");

    let output_stream = output_device
        .build_output_stream(
            output_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if output_channels == 0 {
                    return;
                }

                for frame in data.chunks_exact_mut(output_channels) {
                    let sample = match consumer.pop() {
                        Ok(sample) => {
                            last_output_sample = sample;
                            sample
                        }
                        Err(_) => {
                            // Avoid hard discontinuities on underflow.
                            last_output_sample *= 0.995;
                            last_output_sample
                        }
                    };

                    for out in frame.iter_mut() {
                        *out = sample;
                    }
                }
            },
            move |err| {
                eprintln!("output stream error: {err}");
            },
            None,
        )
        .expect("failed to build output stream");

    input_stream
        .play()
        .expect("failed to start input audio stream");
    output_stream
        .play()
        .expect("failed to start output audio stream");

    sleep(Duration::from_secs(20));
}