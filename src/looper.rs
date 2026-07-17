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
        atomic::{AtomicU8, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

const IDLE: u8 = 0;
const RECORDING: u8 = 1;
const PLAYING: u8 = 2;
const RECORDING_BUFFER_CAPACITY: usize = 1024 * 1024;
const PLAYBACK_COMMAND_CAPACITY: usize = 2;

struct LoopRecorder {
    recorded_samples: Vec<f32>,
    recording_consumer: Consumer<f32>,
    playback_producer: Producer<Vec<f32>>,
    mode: Arc<AtomicU8>,
    active_input_callbacks: Arc<AtomicUsize>,
    recording: bool,
}

impl LoopRecorder {
    fn new(
        recording_consumer: Consumer<f32>,
        playback_producer: Producer<Vec<f32>>,
        mode: Arc<AtomicU8>,
        active_input_callbacks: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            recorded_samples: Vec::new(),
            recording_consumer,
            playback_producer,
            mode,
            active_input_callbacks,
            recording: false,
        }
    }

    fn is_recording(&self) -> bool {
        self.recording
    }

    /// Move samples from the realtime input callback into the control thread.
    fn poll(&mut self) {
        if self.recording {
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
        self.mode.store(RECORDING, Ordering::Release);
        self.recording = true;
        println!("Recording...");
    }

    fn stop_recording(&mut self) {
        self.mode.store(IDLE, Ordering::Release);

        // A callback may have started before the mode changed. Wait for it
        // before draining the queue so the final samples are not missed.
        while self.active_input_callbacks.load(Ordering::Acquire) != 0 {
            thread::yield_now();
        }
        self.drain_recording_samples();
        self.recording = false;

        if self.recorded_samples.is_empty() {
            println!("The recording was empty; press Enter to try again.");
            return;
        }

        let loop_length = self.recorded_samples.len();
        self.playback_producer
            .push(std::mem::take(&mut self.recorded_samples))
            .expect("playback command queue is unexpectedly full");
        self.mode.store(PLAYING, Ordering::Release);
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
    let mode = Arc::new(AtomicU8::new(IDLE));
    let active_input_callbacks = Arc::new(AtomicUsize::new(0));

    let input_mode = Arc::clone(&mode);
    let input_callbacks = Arc::clone(&active_input_callbacks);

    audio.set_callbacks(
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            input_callbacks.fetch_add(1, Ordering::Relaxed);

            if input_channels != 0 {
                for frame in data.chunks_exact(input_channels) {
                    if input_mode.load(Ordering::Acquire) != RECORDING {
                        break;
                    }

                    let mono = mix_to_mono(frame, input_channels) * MIC_GAIN;
                    let _ = recording_producer.push(mono);
                }
            }

            input_callbacks.fetch_sub(1, Ordering::Release);
        },
        {
            let output_mode = Arc::clone(&mode);
            let mut playback_buffer = Vec::new();
            let mut playback_position = 0;

            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if output_channels == 0 {
                    return;
                }

                while let Ok(next_loop) = playback_consumer.pop() {
                    playback_buffer = next_loop;
                    playback_position = 0;
                }

                let playing = output_mode.load(Ordering::Acquire) == PLAYING;
                for frame in data.chunks_exact_mut(output_channels) {
                    let sample = if playing && !playback_buffer.is_empty() {
                        let sample = playback_buffer[playback_position];
                        playback_position = (playback_position + 1) % playback_buffer.len();
                        sample
                    } else {
                        0.0
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
        Arc::clone(&mode),
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
