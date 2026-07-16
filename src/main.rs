use std::sync::Arc;
use std::{thread::sleep, time::Duration};

mod pitch_analyser;
mod synth;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use pitch_analyser::PitchAnalyser;
use synth::Synth;

const TARGET_MAX_LATENCY_MS: f32 = 9.0;
const BUFFER_FRAMES: u32 = 32;
const EXTRA_QUEUE_FRAMES: usize = 32;
const BUFFER_CAPACITY: usize = BUFFER_FRAMES as usize + EXTRA_QUEUE_FRAMES;


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

    let output_sample_rate_hz = output_config.sample_rate as f32;
    let analysis_sample_rate = sample_rate_hz.round() as usize;

    let (mut analyser, mut analysis_producer) = PitchAnalyser::new(analysis_sample_rate);
    let latest_pitch_hz_bits = analyser.latest_pitch_hz_bits();

    let input_stream = input_device
        .build_input_stream(
            input_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                PitchAnalyser::ingest_input(&mut analysis_producer, data, input_channels);
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
                let mut synth = Synth::new(output_sample_rate_hz);
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if output_channels == 0 {
                        return;
                    }

                    for frame in data.chunks_exact_mut(output_channels) {
                        let target_freq_hz =
                            f32::from_bits(latest_pitch_hz_output.load(std::sync::atomic::Ordering::Relaxed));
                        let sample = synth.on_frame(target_freq_hz);

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
    analyser.stop();
}
