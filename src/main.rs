use std::{thread::sleep, time::Duration};

mod pitch_analyser;
mod synth;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use pitch_analyser::PitchAnalyser;
use synth::Synth;

const CALLBACK_BUFFER_SIZE: u32 = 32;

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

    input_config.buffer_size = cpal::BufferSize::Fixed(CALLBACK_BUFFER_SIZE);
    output_config.buffer_size = cpal::BufferSize::Fixed(CALLBACK_BUFFER_SIZE);

    let input_channels = usize::from(input_config.channels);
    let output_channels = usize::from(output_config.channels);

    let sample_rate_hz = input_config.sample_rate as f32;

    let output_sample_rate_hz = output_config.sample_rate as f32;
    let analysis_sample_rate = sample_rate_hz.round() as usize;

    let (mut analyser, mut analysis_producer) = PitchAnalyser::new(analysis_sample_rate);
    let latest_pitch_hz = analyser.latest_pitch_hz();

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
                let latest_pitch_hz_output = latest_pitch_hz.clone();
                let mut synth = Synth::new(output_sample_rate_hz);
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if output_channels == 0 {
                        return;
                    }

                    for frame in data.chunks_exact_mut(output_channels) {
                        let target_freq_hz = latest_pitch_hz_output.get();
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
