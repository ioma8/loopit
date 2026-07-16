use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pitch_detection::detector::PitchDetector;
use pitch_detection::detector::mcleod::McLeodDetector;
use rtrb::{Producer, RingBuffer};

const ANALYSIS_QUEUE_FRAMES: usize = 8192;
const PITCH_WINDOW_SIZE: usize = 1024;
const PITCH_WINDOW_HOP: usize = 512;
const POWER_THRESHOLD: f64 = 0.1;
const CLARITY_THRESHOLD: f64 = 0.4;

pub struct PitchAnalyser {
    latest_pitch_hz: LatestPitchHz,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    producer: Option<Producer<f32>>,
}

#[derive(Clone)]
pub struct LatestPitchHz {
    bits: Arc<AtomicU32>,
}

impl LatestPitchHz {
    fn new() -> Self {
        Self {
            bits: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
        }
    }

    pub fn set(&self, frequency_hz: f32) {
        self.bits.store(frequency_hz.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.bits.load(Ordering::Relaxed))
    }
}

impl PitchAnalyser {
    pub fn new(sample_rate_hz: usize) -> Self {
        let latest_pitch_hz = LatestPitchHz::new();
        let running = Arc::new(AtomicBool::new(true));
        let (producer, mut consumer) = RingBuffer::new(ANALYSIS_QUEUE_FRAMES);

        let latest_pitch_hz_thread = latest_pitch_hz.clone();
        let running_thread = Arc::clone(&running);

        let thread = std::thread::spawn(move || {
            let mut detector = McLeodDetector::<f64>::new(PITCH_WINDOW_SIZE, PITCH_WINDOW_SIZE / 2);
            let mut analysis_ring = vec![0.0_f64; PITCH_WINDOW_SIZE * 2];
            let mut write_pos = 0usize;
            let mut filled = 0usize;
            let mut samples_since_hop = 0usize;
            let mut last_pitch_report = Instant::now() - Duration::from_millis(250);

            while running_thread.load(Ordering::Relaxed) {
                let mut made_progress = false;

                while let Ok(sample) = consumer.pop() {
                    let sample = sample as f64;
                    analysis_ring[write_pos] = sample;
                    analysis_ring[write_pos + PITCH_WINDOW_SIZE] = sample;

                    write_pos += 1;
                    if write_pos == PITCH_WINDOW_SIZE {
                        write_pos = 0;
                    }

                    if filled < PITCH_WINDOW_SIZE {
                        filled += 1;
                    }

                    samples_since_hop += 1;
                    made_progress = true;

                    while filled == PITCH_WINDOW_SIZE && samples_since_hop >= PITCH_WINDOW_HOP {
                        let window = &analysis_ring[write_pos..write_pos + PITCH_WINDOW_SIZE];

                        if let Some(pitch) = detector.get_pitch(
                            window,
                            sample_rate_hz,
                            POWER_THRESHOLD,
                            CLARITY_THRESHOLD,
                        ) {
                            latest_pitch_hz_thread.set(pitch.frequency as f32);

                            let now = Instant::now();
                            if now.duration_since(last_pitch_report) >= Duration::from_millis(100) {
                                println!(
                                    "Pitch estimate: {:.2} Hz (clarity {:.3})",
                                    pitch.frequency, pitch.clarity
                                );
                                last_pitch_report = now;
                            }
                        } else {
                            latest_pitch_hz_thread.set(0.0_f32);
                        }

                        samples_since_hop -= PITCH_WINDOW_HOP;
                    }
                }

                if !made_progress {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        });

        Self {
            latest_pitch_hz,
            running,
            thread: Some(thread),
            producer: Some(producer),
        }
    }

    pub fn take_producer(&mut self) -> Producer<f32> {
        self.producer
            .take()
            .expect("analysis producer was already taken")
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    pub fn latest_pitch_hz(&self) -> LatestPitchHz {
        self.latest_pitch_hz.clone()
    }
}
