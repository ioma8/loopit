mod audio;
mod dsp;
mod pitch_analyser;
mod synth;

use audio::Audio;
use dsp::MIC_GAIN;
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    io::{self, BufRead},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const RECORDING_BUFFER_CAPACITY: usize = 1024 * 1024;
const PLAYBACK_COMMAND_CAPACITY: usize = 4;

enum PlaybackCommand {
    Clear,
    Play(Vec<f32>),
}

struct LoopRecorder {
    recorded_samples: Vec<f32>,
    recording_consumer: Consumer<f32>,
    playback_producer: Producer<PlaybackCommand>,
    recording: Arc<AtomicBool>,
    active_input_callbacks: Arc<AtomicUsize>,
}

impl LoopRecorder {
    fn new(
        recording_consumer: Consumer<f32>,
        playback_producer: Producer<PlaybackCommand>,
        recording: Arc<AtomicBool>,
        active_input_callbacks: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            recorded_samples: Vec::new(),
            recording_consumer,
            playback_producer,
            recording,
            active_input_callbacks,
        }
    }

    fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Acquire)
    }

    /// Move samples from the realtime input callback into the control thread.
    fn poll(&mut self) {
        if self.is_recording() {
            while let Ok(sample) = self.recording_consumer.pop() {
                self.recorded_samples.push(sample);
            }
        } else {
            self.discard_pending_samples();
        }
    }

    fn start_recording(&mut self) {
        self.discard_pending_samples();
        self.recorded_samples.clear();
        self.playback_producer
            .push(PlaybackCommand::Clear)
            .expect("playback command queue is unexpectedly full");
        self.recording.store(true, Ordering::Release);
        println!("Recording...");
    }

    fn stop_recording(&mut self) {
        self.recording.store(false, Ordering::Release);

        // A callback may have started before the mode changed. Wait for it
        // before draining the queue so the final samples are not missed.
        while self.active_input_callbacks.load(Ordering::Acquire) != 0 {
            thread::yield_now();
        }
        self.drain_recording_samples();

        if self.recorded_samples.is_empty() {
            println!("The recording was empty; press Enter to try again.");
            return;
        }

        let loop_length = self.recorded_samples.len();
        self.playback_producer
            .push(PlaybackCommand::Play(std::mem::take(
                &mut self.recorded_samples,
            )))
            .expect("playback command queue is unexpectedly full");
        println!("Playing a {loop_length}-sample loop. Press Enter to record a new loop.");
    }

    fn handle_enter(&mut self) {
        if self.is_recording() {
            self.stop_recording();
        } else {
            self.start_recording();
        }
    }

    fn drain_recording_samples(&mut self) {
        while let Ok(sample) = self.recording_consumer.pop() {
            self.recorded_samples.push(sample);
        }
    }

    fn discard_pending_samples(&mut self) {
        while self.recording_consumer.pop().is_ok() {}
    }
}

fn mix_to_mono(frame: &[f32], input_channels: usize) -> f32 {
    frame.iter().copied().sum::<f32>() / input_channels as f32
}

fn main() {
    let mut audio = Audio::new();

    let input_channels = usize::from(audio.input_config.channels);
    let output_channels = usize::from(audio.output_config.channels);
    let (mut recording_producer, recording_consumer) = RingBuffer::new(RECORDING_BUFFER_CAPACITY);
    let (playback_producer, mut playback_consumer) = RingBuffer::new(PLAYBACK_COMMAND_CAPACITY);
    let recording = Arc::new(AtomicBool::new(false));
    let active_input_callbacks = Arc::new(AtomicUsize::new(0));

    let input_recording = Arc::clone(&recording);
    let input_callbacks = Arc::clone(&active_input_callbacks);

    audio.set_callbacks(
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            input_callbacks.fetch_add(1, Ordering::Relaxed);

            if input_channels != 0 {
                for frame in data.chunks_exact(input_channels) {
                    if !input_recording.load(Ordering::Acquire) {
                        break;
                    }

                    let mono = mix_to_mono(frame, input_channels) * MIC_GAIN;
                    let _ = recording_producer.push(mono);
                }
            }

            input_callbacks.fetch_sub(1, Ordering::Release);
        },
        {
            let mut playback_buffer = None;
            let mut playback_position = 0;

            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if output_channels == 0 {
                    return;
                }

                while let Ok(command) = playback_consumer.pop() {
                    match command {
                        PlaybackCommand::Clear => playback_buffer = None,
                        PlaybackCommand::Play(next_loop) => {
                            playback_buffer = Some(next_loop);
                            playback_position = 0;
                        }
                    }
                }

                for frame in data.chunks_exact_mut(output_channels) {
                    let sample = match playback_buffer.as_ref() {
                        Some(loop_buffer) if !loop_buffer.is_empty() => {
                            let sample = loop_buffer[playback_position];
                            playback_position = (playback_position + 1) % loop_buffer.len();
                            sample
                        }
                        _ => 0.0,
                    };

                    for out in frame.iter_mut() {
                        *out = sample;
                    }
                }
            }
        },
    );

    let mut loop_recorder = LoopRecorder::new(
        recording_consumer,
        playback_producer,
        Arc::clone(&recording),
        active_input_callbacks,
    );

    audio.play();

    let (key_tx, key_rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for _ in stdin.lock().lines() {
            if key_tx.send(()).is_err() {
                break;
            }
        }
    });

    println!("Press Enter to start recording.");
    println!("Press Enter again to stop and play the loop.");

    loop {
        loop_recorder.poll();

        match key_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(()) => loop_recorder.handle_enter(),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
