use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;
use rtrb::{Consumer, Producer, RingBuffer};

const ANALYSIS_QUEUE_FRAMES: usize = 8192;
const PITCH_WINDOW_SIZE: usize = 1024;
const PITCH_WINDOW_HOP: usize = 512;
const POWER_THRESHOLD: f64 = 0.2;
const CLARITY_THRESHOLD: f64 = 0.5;
const MIC_GAIN: f32 = 1.5;

fn mix_to_mono(frame: &[f32], input_channels: usize) -> f32 {
    frame.iter().copied().sum::<f32>() / input_channels as f32
}

pub struct PitchAnalyser {
    pub latest_pitch_hz_bits: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PitchAnalyser {
    pub fn new(sample_rate_hz: usize) -> (Self, Producer<f32>) {
        let latest_pitch_hz_bits = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let running = Arc::new(AtomicBool::new(true));
        let (analysis_producer, mut analysis_consumer) = RingBuffer::new(ANALYSIS_QUEUE_FRAMES);

        let latest_pitch_hz_thread = Arc::clone(&latest_pitch_hz_bits);
        let running_thread = Arc::clone(&running);

        let thread = std::thread::spawn(move || {
            let mut detector = McLeodDetector::<f64>::new(PITCH_WINDOW_SIZE, PITCH_WINDOW_SIZE / 2);
            let mut analysis_window: Vec<f64> = Vec::with_capacity(PITCH_WINDOW_SIZE * 2);
            let mut last_pitch_report = Instant::now() - Duration::from_millis(250);

            while running_thread.load(Ordering::Relaxed) {
                let mut made_progress = false;

                while let Ok(sample) = analysis_consumer.pop() {
                    analysis_window.push(sample as f64);
                    made_progress = true;
                }

                while analysis_window.len() >= PITCH_WINDOW_SIZE {
                    if let Some(pitch) = detector.get_pitch(
                        &analysis_window[..PITCH_WINDOW_SIZE],
                        sample_rate_hz,
                        POWER_THRESHOLD,
                        CLARITY_THRESHOLD,
                    ) {
                        latest_pitch_hz_thread.store((pitch.frequency as f32).to_bits(), Ordering::Relaxed);

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
                    }

                    analysis_window.drain(..PITCH_WINDOW_HOP);
                    made_progress = true;
                }

                if !made_progress {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        });

        (
            Self {
                latest_pitch_hz_bits,
                running,
                thread: Some(thread),
            },
            analysis_producer,
        )
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    pub fn latest_pitch_hz_bits(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.latest_pitch_hz_bits)
    }

    pub fn ingest_input(
        analysis_producer: &mut Producer<f32>,
        data: &[f32],
        input_channels: usize,
    ) {
        if input_channels == 0 {
            return;
        }

        for frame in data.chunks_exact(input_channels) {
            let mono = mix_to_mono(frame, input_channels) * MIC_GAIN;
            let _ = analysis_producer.push(mono);
        }
    }
}
