mod cpal_setup;
mod pitch_analyser;
mod synth;

use std::{thread::sleep, time::Duration};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal_setup::CpalSetup;
use pitch_analyser::PitchAnalyser;
use synth::Synth;

const MIC_GAIN: f32 = 2.0;

fn mix_to_mono(frame: &[f32], input_channels: usize) -> f32 {
    frame.iter().copied().sum::<f32>() / input_channels as f32
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

    let input_config = input_supported_config.config();
    let output_config = output_supported_config.config();
    let input_channels = usize::from(input_config.channels);
    let output_channels = usize::from(output_config.channels);

    let analysis_sample_rate = input_config.sample_rate as usize;
    let output_sample_rate_hz = output_config.sample_rate as f32;

    let mut analyser = PitchAnalyser::new(analysis_sample_rate);
    let mut analysis_producer = analyser.take_producer();
    let latest_pitch_hz = analyser.latest_pitch_hz();
    let synth = Synth::new(output_sample_rate_hz);
    let audio = CpalSetup::new(
        &input_device,
        &output_device,
        &input_supported_config,
        &output_supported_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            if input_channels == 0 {
                return;
            }

            for frame in data.chunks_exact(input_channels) {
                let mono = mix_to_mono(frame, input_channels) * MIC_GAIN;
                let _ = analysis_producer.push(mono);
            }
        },
        {
            let mut synth = synth;
            let latest_pitch_hz_output = latest_pitch_hz.clone();
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
    );

    audio.play();

    sleep(Duration::from_secs(20));
    analyser.stop();
}
