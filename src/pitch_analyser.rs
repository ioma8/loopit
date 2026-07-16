use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rtrb::{Producer, RingBuffer};

use crate::dsp::{PitchTracker, PITCH_WINDOW_HOP};

const ANALYSIS_QUEUE_FRAMES: usize = 8192;

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
            let mut tracker = PitchTracker::new(sample_rate_hz);
            let mut samples_since_report = 0usize;
            let mut last_pitch_report = Instant::now() - Duration::from_millis(250);

            while running_thread.load(Ordering::Relaxed) {
                let mut made_progress = false;

                while let Ok(sample) = consumer.pop() {
                    tracker.push_sample(sample);
                    samples_since_report += 1;
                    made_progress = true;

                    latest_pitch_hz_thread.set(tracker.last_pitch_hz());

                    if samples_since_report >= PITCH_WINDOW_HOP {
                        let now = Instant::now();
                        if now.duration_since(last_pitch_report) >= Duration::from_millis(100) {
                            println!(
                                "Pitch estimate: {:.2} Hz (clarity {:.3})",
                                tracker.last_pitch_hz(),
                                tracker.last_pitch_clarity()
                            );
                            last_pitch_report = now;
                            samples_since_report = 0;
                        }
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
