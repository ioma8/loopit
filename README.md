# loopit

`loopit` is a microphone-driven pitch tracker that turns detected pitch into a synthesized output tone.

The repository currently has two front ends over the same core DSP:

- Native Rust app using CPAL for realtime audio I/O.
- Browser demo using a small WASM interface and Web Audio.

## Current behavior

The project is not an octave-divider or vocal FX pipeline anymore. The current signal flow is:

1. Read microphone input.
2. Mix to mono and apply `MIC_GAIN`.
3. Track pitch with the McLeod Pitch Method.
4. Drive a simple synth from the latest detected pitch.
5. Output the synth signal to speakers or headphones.

When no stable pitch is detected, the synth fades down instead of producing a hard on/off transition.

## Native app architecture

The native executable in [src/main.rs](/Users/jakubkolcar/projects/customs/loopit/src/main.rs) splits work into two paths:

- Input callback:
  - Reads `f32` microphone samples from CPAL.
  - Mixes multichannel input to mono.
  - Applies mic gain.
  - Pushes samples into a lock-free analysis queue.

- Analysis thread:
  - Drains the queue outside the realtime callback.
  - Updates the latest detected pitch and clarity.
  - Prints periodic pitch estimates for debugging.

- Output callback:
  - Reads the latest detected pitch.
  - Generates synth samples in realtime.
  - Writes the same sample to every output channel.

This keeps pitch analysis off the audio callback's critical path.

## DSP components

The core DSP lives in [src/dsp.rs](/Users/jakubkolcar/projects/customs/loopit/src/dsp.rs).

- `PitchTracker`
  - Uses `pitch-detection`'s McLeod detector.
  - Maintains a sliding analysis window.
  - Exposes the last detected pitch and clarity.

- `SynthCore`
  - Generates a layered sine-based tone.
  - Includes sub, fundamental, and optional upper partial voices.
  - Smooths output level with separate attack and release behavior.

## Browser demo

The browser demo in [public/main.js](/Users/jakubkolcar/projects/customs/loopit/public/main.js) loads [public/loopit.wasm](/Users/jakubkolcar/projects/customs/loopit/public/loopit.wasm) and processes microphone audio through a Web Audio `ScriptProcessorNode`.

The WASM layer in [src/lib.rs](/Users/jakubkolcar/projects/customs/loopit/src/lib.rs) exposes:

- `loopit_new`
- `loopit_process`
- `loopit_reset`
- `loopit_get_pitch_hz`
- `loopit_get_pitch_clarity`
- `loopit_free`

That path runs pitch tracking and synthesis sample-by-sample in the browser-facing processor.

## Key constants

Defined in [src/dsp.rs](/Users/jakubkolcar/projects/customs/loopit/src/dsp.rs):

- `MIC_GAIN = 2.0`
- `PITCH_WINDOW_SIZE = 1024`
- `PITCH_WINDOW_HOP = 512`
- `POWER_THRESHOLD = 0.1`
- `CLARITY_THRESHOLD = 0.4`

Defined in [src/pitch_analyser.rs](/Users/jakubkolcar/projects/customs/loopit/src/pitch_analyser.rs):

- `ANALYSIS_QUEUE_FRAMES = 8192`

Defined in [src/audio.rs](/Users/jakubkolcar/projects/customs/loopit/src/audio.rs):

- `CALLBACK_BUFFER_SIZE = 32`

## Build and run

Run the native app:

```bash
cargo run
```

The native executable currently starts audio, runs for 20 seconds, and then stops.

Build the browser WASM artifact:

```bash
./build-web.sh
```

This writes `public/loopit.wasm`.

## Dependencies

- `cpal` for native audio I/O
- `rtrb` for the native analysis queue
- `pitch-detection` for McLeod pitch detection
