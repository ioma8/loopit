# loopit

Low-latency realtime vocal processing in Rust using CPAL, plus block-based pitch analysis using the McLeod Pitch Method (MPM).

## What runs where

There are two separate paths in [src/main.rs](src/main.rs):

- Realtime audio path (device callback deadlines):
  - Input callback reads mic data.
  - Signal is processed (HP/LP filter, gate, octave-divider style polarity flip, smoothing, limiter).
  - Samples are pushed to a small output ring buffer.
  - Output callback pops samples and fills the output device buffer.

- Analysis path (allowed to be delayed):
  - Input callback also pushes raw mono mic samples into a larger analysis ring buffer.
  - Main thread drains that queue and runs MPM on sliding 1024-sample windows.
  - Pitch estimates are printed with clarity/confidence.

This keeps heavy analysis off the realtime callback path.

## Why pitch detection is not in the callback

Audio callbacks are time-critical. Missing callback deadlines causes clicks/dropouts.

MPM is efficient, but still heavier than the simple per-sample DSP used for output. Running it in the callback would increase glitch risk, so it is intentionally done on the main thread over buffered blocks.

## Key constants

Defined in [src/main.rs](src/main.rs):

- `BUFFER_FRAMES = 32`
  - Requested device callback buffer size.

- `EXTRA_QUEUE_FRAMES = 32`
  - Extra headroom for the output queue to absorb callback timing mismatch.

- `BUFFER_CAPACITY = BUFFER_FRAMES + EXTRA_QUEUE_FRAMES` (64)
  - Capacity of realtime output ring buffer.

- `ANALYSIS_QUEUE_FRAMES = 8192`
  - Larger queue used for delayed analysis path.

- `PITCH_WINDOW_SIZE = 1024`
  - MPM analysis frame size.

- `PITCH_WINDOW_HOP = 512`
  - 50% overlap between analysis windows.

- `POWER_THRESHOLD = 5.0`
  - Minimum signal power to attempt pitch detection.

- `CLARITY_THRESHOLD = 0.7`
  - Minimum MPM confidence required to accept a pitch.

## MPM thresholds explained

For `detector.get_pitch(signal, sample_rate, power_threshold, clarity_threshold)`:

- `power_threshold`
  - Early gate on energy. If signal power is too low, returns no pitch.
  - Higher value: fewer false triggers, less sensitivity.
  - Lower value: more sensitivity, more false detections.

- `clarity_threshold`
  - Confidence gate on detected periodicity.
  - Higher value: cleaner/stabler pitch output, more dropouts.
  - Lower value: more continuous output, potentially more wrong estimates.

## Build and run

```bash
cargo run
```

The app currently runs for 20 seconds and prints detected pitch lines when available.

## Dependencies

- `cpal` for audio I/O
- `rtrb` for lock-free SPSC ring buffers
- `pitch-detection` for McLeod Pitch Method (MPM)
