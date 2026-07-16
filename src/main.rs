mod audio;
mod pitch_analyser;
mod synth;

use audio::Audio;
use pitch_analyser::PitchAnalyser;
use std::{thread::sleep, time::Duration};
use synth::Synth;

const MIC_GAIN: f32 = 2.0;

fn mix_to_mono(frame: &[f32], input_channels: usize) -> f32 {
    frame.iter().copied().sum::<f32>() / input_channels as f32
}

fn main() {
    let mut audio = Audio::new();
    let input_channels = usize::from(audio.input_config.channels);
    let output_channels = usize::from(audio.output_config.channels);
    let analysis_sample_rate = audio.input_config.sample_rate as usize;
    let output_sample_rate_hz = audio.output_config.sample_rate as f32;

    let mut analyser = PitchAnalyser::new(analysis_sample_rate);
    let mut analysis_producer = analyser.take_producer();
    let latest_pitch_hz = analyser.latest_pitch_hz();
    let synth = Synth::new(output_sample_rate_hz);
    audio.set_callbacks(
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
