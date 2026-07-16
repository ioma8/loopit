use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use pitch_detection::detector::PitchDetector;
use pitch_detection::detector::mcleod::McLeodDetector;
use rtrb::{Consumer, Producer, RingBuffer};

const TARGET_MAX_LATENCY_MS: f32 = 9.0;
const BUFFER_FRAMES: u32 = 32;
const EXTRA_QUEUE_FRAMES: usize = 32;
const BUFFER_CAPACITY: usize = BUFFER_FRAMES as usize + EXTRA_QUEUE_FRAMES;
const ANALYSIS_QUEUE_FRAMES: usize = 8192;
const PITCH_WINDOW_SIZE: usize = 1024;
const PITCH_WINDOW_HOP: usize = 512;
const POWER_THRESHOLD: f64 = 0.2;
const CLARITY_THRESHOLD: f64 = 0.5;
const OUTPUT_SYNTH_FROM_PITCH: bool = true;
const SYNTH_GAIN: f32 = 0.90;
const SYNTH_GAIN_ATTACK: f32 = 0.02;
const SYNTH_GAIN_RELEASE: f32 = 0.001;
const MIC_GAIN: f32 = 1.5;

fn mix_to_mono(frame: &[f32], input_channels: usize) -> f32 {
    frame.iter().copied().sum::<f32>() / input_channels as f32
}

struct InputProcessor {}

impl InputProcessor {
    fn new() -> Self {
        Self {}
    }

    fn ingest_input(
        &mut self,
        data: &[f32],
        input_channels: usize,
        output_producer: &mut Producer<f32>,
        analysis_producer: &mut Producer<f32>,
    ) {
        if input_channels == 0 {
            return;
        }

        for frame in data.chunks_exact(input_channels) {
            let mono = mix_to_mono(frame, input_channels) * MIC_GAIN;
            let _ = analysis_producer.push(mono);

            if !OUTPUT_SYNTH_FROM_PITCH {
                let sample = self.process_sample(mono);
                let _ = output_producer.push(sample);
            }
        }
    }

    fn process_sample(&mut self, sample: f32) -> f32 {
        sample
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
    let (mut analysis_producer, mut analysis_consumer): (Producer<f32>, Consumer<f32>) =
        RingBuffer::new(ANALYSIS_QUEUE_FRAMES);
    let mut input_processor = InputProcessor::new();
    let mut last_output_sample = 0.0_f32;
    let output_sample_rate_hz = output_config.sample_rate as f32;

    let analysis_sample_rate = sample_rate_hz.round() as usize;
    let analysis_running = Arc::new(AtomicBool::new(true));
    let latest_pitch_hz_bits = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
    let latest_pitch_clarity_bits = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
    let analysis_running_thread = Arc::clone(&analysis_running);
    let latest_pitch_hz_thread = Arc::clone(&latest_pitch_hz_bits);
    let latest_pitch_clarity_thread = Arc::clone(&latest_pitch_clarity_bits);
    let analysis_thread = std::thread::spawn(move || {
        let mut detector = McLeodDetector::<f64>::new(PITCH_WINDOW_SIZE, PITCH_WINDOW_SIZE / 2);
        let mut analysis_window: Vec<f64> = Vec::with_capacity(PITCH_WINDOW_SIZE * 2);
        let mut last_pitch_report = Instant::now() - Duration::from_millis(250);

        while analysis_running_thread.load(Ordering::Relaxed) {
            let mut made_progress = false;

            while let Ok(sample) = analysis_consumer.pop() {
                analysis_window.push(sample as f64);
                made_progress = true;
            }

            while analysis_window.len() >= PITCH_WINDOW_SIZE {
                if let Some(pitch) = detector.get_pitch(
                    &analysis_window[..PITCH_WINDOW_SIZE],
                    analysis_sample_rate,
                    POWER_THRESHOLD,
                    CLARITY_THRESHOLD,
                ) {
                    latest_pitch_hz_thread
                        .store((pitch.frequency as f32).to_bits(), Ordering::Relaxed);
                    latest_pitch_clarity_thread
                        .store((pitch.clarity as f32).to_bits(), Ordering::Relaxed);

                    let now = Instant::now();
                    if now.duration_since(last_pitch_report) >= Duration::from_millis(100) {
                        println!(
                            "Pitch estimate: {:.2} Hz (clarity {:.3})",
                            pitch.frequency, pitch.clarity
                        );
                        last_pitch_report = now;
                    }
                } else {
                    latest_pitch_hz_thread.store(0.0_f32.to_bits(), Ordering::Relaxed);
                    latest_pitch_clarity_thread.store(0.0_f32.to_bits(), Ordering::Relaxed);
                }

                analysis_window.drain(..PITCH_WINDOW_HOP);
                made_progress = true;
            }

            if !made_progress {
                sleep(Duration::from_millis(1));
            }
        }
    });

    let input_stream = input_device
        .build_input_stream(
            input_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                input_processor.ingest_input(
                    data,
                    input_channels,
                    &mut producer,
                    &mut analysis_producer,
                );
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
            {
                let latest_pitch_hz_output = Arc::clone(&latest_pitch_hz_bits);
                let mut phase_sub = 0.0_f32;
                let mut phase_norm = 0.0_f32;
                let mut phase_sup = 0.0_f32;
                let mut synth_level = 0.0_f32;
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if output_channels == 0 {
                        return;
                    }

                    for frame in data.chunks_exact_mut(output_channels) {
                        let sample = if OUTPUT_SYNTH_FROM_PITCH {
                            let target_freq_hz =
                                f32::from_bits(latest_pitch_hz_output.load(Ordering::Relaxed));

                            let has_pitch = target_freq_hz > 0.0 && target_freq_hz.is_finite();

                            if has_pitch {
                                synth_level += SYNTH_GAIN_ATTACK * (SYNTH_GAIN - synth_level);
                            } else {
                                synth_level += SYNTH_GAIN_RELEASE * (0.0 - synth_level);
                            }

                            phase_sub += (2.0 * std::f32::consts::PI * target_freq_hz * 0.5)
                                / output_sample_rate_hz;
                            if phase_sub > std::f32::consts::TAU {
                                phase_sub -= std::f32::consts::TAU;
                            }

                            phase_norm += (2.0 * std::f32::consts::PI * target_freq_hz)
                                / output_sample_rate_hz;
                            if phase_norm > std::f32::consts::TAU {
                                phase_norm -= std::f32::consts::TAU;
                            }

                            phase_sup += (2.0 * std::f32::consts::PI * target_freq_hz * 2.0)
                                / output_sample_rate_hz;
                            if phase_sup > std::f32::consts::TAU {
                                phase_sup -= std::f32::consts::TAU;
                            }

                            (phase_sub.sin() * synth_level)
                                + (phase_norm.sin() * synth_level * 0.3)
                                + (phase_sup.sin() * synth_level * 0.05)
                        } else {
                            match consumer.pop() {
                                Ok(sample) => {
                                    last_output_sample = sample;
                                    sample
                                }
                                Err(_) => {
                                    // Avoid hard discontinuities on underflow.
                                    last_output_sample *= 0.995;
                                    last_output_sample
                                }
                            }
                        };

                        for out in frame.iter_mut() {
                            *out = sample;
                        }
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
    analysis_running.store(false, Ordering::Relaxed);
    let _ = analysis_thread.join();
}
