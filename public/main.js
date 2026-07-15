const statusEl = document.getElementById("status");
const debugEl = document.getElementById("debug");
const startBtn = document.getElementById("start");
const stopBtn = document.getElementById("stop");

let audioCtx = null;
let sourceNode = null;
let processorNode = null;
let stream = null;
let wasm = null;
let wasmPtr = null;
let diagnosticsInterval = null;
let callbackCount = 0;
let lastRms = 0;
let lastInputChannels = 0;
let deviceSummary = "No device yet";
let adaptiveInputGain = 1.0;
let lastOutputRms = 0;

const TARGET_INPUT_RMS = 0.02;
const MIN_INPUT_GAIN = 1.0;
const MAX_INPUT_GAIN = 18.0;
const INPUT_GAIN_SMOOTH = 0.08;
const POST_GAIN = 1.35;

function softClip(sample) {
  return Math.tanh(sample);
}

function setStatus(message) {
  statusEl.textContent = message;
}

function setDebug(message) {
  debugEl.textContent = message;
}

async function updateDeviceSummary() {
  try {
    if (!navigator.mediaDevices?.enumerateDevices) {
      deviceSummary = "enumerateDevices() unavailable";
      return;
    }

    const devices = await navigator.mediaDevices.enumerateDevices();
    const audioInputs = devices.filter((device) => device.kind === "audioinput");
    const audioOutputs = devices.filter((device) => device.kind === "audiooutput");
    const track = stream?.getAudioTracks?.()[0] ?? null;
    const settings = track?.getSettings?.() ?? {};

    deviceSummary = [
      `selected input label: ${track?.label || "unknown"}`,
      `track readyState/enabled/muted: ${track?.readyState || "unknown"} / ${track?.enabled ?? "unknown"} / ${track?.muted ?? "unknown"}`,
      `selected input settings: ${JSON.stringify(settings)}`,
      `available inputs: ${audioInputs.map((d) => d.label || d.deviceId).join(", ") || "none"}`,
      `available outputs: ${audioOutputs.map((d) => d.label || d.deviceId).join(", ") || "none"}`,
      `setSinkId supported: ${typeof HTMLMediaElement !== "undefined" && "setSinkId" in HTMLMediaElement.prototype}`,
    ].join("\n");
  } catch (error) {
    deviceSummary = `device enumeration failed: ${error.message}`;
  }
}

function startDiagnostics() {
  stopDiagnostics();
  setDebug([
    `audioContext state: ${audioCtx ? audioCtx.state : "none"}`,
    `wasm pointer: ${wasmPtr}`,
    `script callbacks: ${callbackCount}`,
    `input channels seen: ${lastInputChannels}`,
    `last RMS: ${lastRms.toFixed(6)}`,
    `adaptive input gain: ${adaptiveInputGain.toFixed(3)}`,
    `output RMS: ${lastOutputRms.toFixed(6)}`,
    deviceSummary,
  ].join("\n\n"));
  diagnosticsInterval = setInterval(() => {
    const contextState = audioCtx ? audioCtx.state : "none";
    setDebug([
      `audioContext state: ${contextState}`,
      `wasm pointer: ${wasmPtr}`,
      `script callbacks: ${callbackCount}`,
      `input channels seen: ${lastInputChannels}`,
      `last RMS: ${lastRms.toFixed(6)}`,
      `adaptive input gain: ${adaptiveInputGain.toFixed(3)}`,
      `output RMS: ${lastOutputRms.toFixed(6)}`,
      deviceSummary,
    ].join("\n\n"));
  }, 300);
}

function stopDiagnostics() {
  if (diagnosticsInterval) {
    clearInterval(diagnosticsInterval);
    diagnosticsInterval = null;
  }
}

async function loadWasm() {
  if (wasm) {
    return wasm;
  }

  const response = await fetch("./loopit.wasm");
  if (!response.ok) {
    throw new Error(`Failed to load loopit.wasm: ${response.status}`);
  }

  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  wasm = instance.exports;

  if (!wasm.loopit_new || !wasm.loopit_process || !wasm.loopit_free) {
    throw new Error("WASM exports are missing expected loopit functions");
  }

  return wasm;
}

async function start() {
  try {
    startBtn.disabled = true;
    setStatus("Requesting microphone...");

    // Create/resume immediately inside the click gesture to satisfy autoplay policies.
    if (!audioCtx) {
      audioCtx = new AudioContext({ latencyHint: "interactive" });
    }
    if (audioCtx.state !== "running") {
      await audioCtx.resume();
    }

    const module = await loadWasm();

    stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      },
    });
    await updateDeviceSummary();
    const track = stream.getAudioTracks()[0];
    if (track) {
      track.onmute = () => setDebug(`Track muted\n\n${deviceSummary}`);
      track.onunmute = () => setDebug(`Track unmuted\n\n${deviceSummary}`);
      track.onended = () => setDebug(`Track ended\n\n${deviceSummary}`);
    }

    if (audioCtx.state !== "running") {
      await audioCtx.resume();
    }

    const sampleRate = audioCtx.sampleRate;
    wasmPtr = module.loopit_new(sampleRate);

    sourceNode = audioCtx.createMediaStreamSource(stream);

    // ScriptProcessor is simple and broadly supported for this demo.
    // Accept stereo in/out and mix mono manually so Firefox channel behavior is visible.
    processorNode = audioCtx.createScriptProcessor(256, 2, 2);
    processorNode.onaudioprocess = (event) => {
      if (wasmPtr === null) {
        return;
      }

      const inputChannels = event.inputBuffer.numberOfChannels;
      const outputChannels = event.outputBuffer.numberOfChannels;
      callbackCount += 1;
      lastInputChannels = inputChannels;

      let inputEnergy = 0;
      let outputEnergy = 0;

      for (let i = 0; i < event.outputBuffer.length; i += 1) {
        let mono = 0;

        for (let channel = 0; channel < inputChannels; channel += 1) {
          mono += event.inputBuffer.getChannelData(channel)[i] ?? 0;
        }

        if (inputChannels > 0) {
          mono /= inputChannels;
        }

        inputEnergy += mono * mono;

        const normalizedInput = mono * adaptiveInputGain;
        const processed = softClip(module.loopit_process(wasmPtr, normalizedInput) * POST_GAIN);
        outputEnergy += processed * processed;

        for (let channel = 0; channel < outputChannels; channel += 1) {
          event.outputBuffer.getChannelData(channel)[i] = processed;
        }
      }

      lastRms = Math.sqrt(inputEnergy / Math.max(event.outputBuffer.length, 1));
      lastOutputRms = Math.sqrt(outputEnergy / Math.max(event.outputBuffer.length, 1));

      const desiredGain = Math.min(
        MAX_INPUT_GAIN,
        Math.max(MIN_INPUT_GAIN, TARGET_INPUT_RMS / Math.max(lastRms, 0.0005)),
      );
      adaptiveInputGain += INPUT_GAIN_SMOOTH * (desiredGain - adaptiveInputGain);

      if (callbackCount <= 5 || callbackCount % 50 === 0) {
        setDebug([
          `audioContext state: ${audioCtx ? audioCtx.state : "none"}`,
          `wasm pointer: ${wasmPtr}`,
          `script callbacks: ${callbackCount}`,
          `input channels seen: ${lastInputChannels}`,
          `last RMS: ${lastRms.toFixed(6)}`,
          `adaptive input gain: ${adaptiveInputGain.toFixed(3)}`,
          `output RMS: ${lastOutputRms.toFixed(6)}`,
          deviceSummary,
        ].join("\n\n"));
      }
    };

    sourceNode.connect(processorNode);
    processorNode.connect(audioCtx.destination);

    if (audioCtx.state !== "running") {
      await audioCtx.resume();
    }

    stopBtn.disabled = false;
    setStatus(`Running at ${sampleRate} Hz (context: ${audioCtx.state}, ptr: ${wasmPtr})`);
    startDiagnostics();
  } catch (error) {
    console.error(error);
    setStatus(`Error: ${error.message}`);
    setDebug(`Startup error:\n${error.stack || error.message}`);
    startBtn.disabled = false;
  }
}

function stop() {
  if (processorNode) {
    processorNode.disconnect();
    processorNode.onaudioprocess = null;
    processorNode = null;
  }

  if (sourceNode) {
    sourceNode.disconnect();
    sourceNode = null;
  }

  if (stream) {
    stream.getTracks().forEach((track) => track.stop());
    stream = null;
  }

  if (audioCtx) {
    audioCtx.close();
    audioCtx = null;
  }

  if (wasm && wasmPtr !== null) {
    wasm.loopit_free(wasmPtr);
    wasmPtr = null;
  }

  stopBtn.disabled = true;
  startBtn.disabled = false;
  setStatus("Stopped");
  stopDiagnostics();
  setDebug("Stopped");
}

startBtn.addEventListener("click", () => {
  start();
});

stopBtn.addEventListener("click", () => {
  stop();
});
