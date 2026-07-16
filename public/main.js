const statusEl = document.getElementById("status");
const debugEl = document.getElementById("debug");
const startBtn = document.getElementById("start");
const stopBtn = document.getElementById("stop");
const micGainSlider = document.getElementById("micGain");
const micGainValueEl = document.getElementById("micGainValue");
const speakerSafeCheckbox = document.getElementById("speakerSafe");

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
let lastOutputRms = 0;
let userMicGain = Number.parseFloat(micGainSlider?.value ?? "1.0") || 1.0;
let lastPitchHz = 0;
let lastPitchClarity = 0;

function setStatus(message) {
  statusEl.textContent = message;
}

function setDebug(message) {
  debugEl.textContent = message;
}

function updateMicGainLabel() {
  if (micGainValueEl) {
    micGainValueEl.textContent = `${userMicGain.toFixed(1)}x`;
  }
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
    `user mic gain: ${userMicGain.toFixed(2)}x`,
    `output RMS: ${lastOutputRms.toFixed(6)}`,
    `pitch estimate: ${lastPitchHz.toFixed(2)} Hz (clarity ${lastPitchClarity.toFixed(3)})`,
    `speaker-safe mode: ${speakerSafeCheckbox?.checked ?? false}`,
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
      `user mic gain: ${userMicGain.toFixed(2)}x`,
      `output RMS: ${lastOutputRms.toFixed(6)}`,
      `pitch estimate: ${lastPitchHz.toFixed(2)} Hz (clarity ${lastPitchClarity.toFixed(3)})`,
      `speaker-safe mode: ${speakerSafeCheckbox?.checked ?? false}`,
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
        echoCancellation: speakerSafeCheckbox?.checked ?? false,
        noiseSuppression: speakerSafeCheckbox?.checked ?? false,
        autoGainControl: speakerSafeCheckbox?.checked ?? false,
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

        const normalizedInput = mono * userMicGain;
        const processed = module.loopit_process(wasmPtr, normalizedInput);
        outputEnergy += processed * processed;

        for (let channel = 0; channel < outputChannels; channel += 1) {
          event.outputBuffer.getChannelData(channel)[i] = processed;
        }
      }

      lastRms = Math.sqrt(inputEnergy / Math.max(event.outputBuffer.length, 1));
      lastOutputRms = Math.sqrt(outputEnergy / Math.max(event.outputBuffer.length, 1));

      if (module.loopit_get_pitch_hz && module.loopit_get_pitch_clarity) {
        lastPitchHz = module.loopit_get_pitch_hz(wasmPtr) || 0;
        lastPitchClarity = module.loopit_get_pitch_clarity(wasmPtr) || 0;
      }

      if (callbackCount <= 5 || callbackCount % 50 === 0) {
        setDebug([
          `audioContext state: ${audioCtx ? audioCtx.state : "none"}`,
          `wasm pointer: ${wasmPtr}`,
          `script callbacks: ${callbackCount}`,
          `input channels seen: ${lastInputChannels}`,
          `last RMS: ${lastRms.toFixed(6)}`,
          `user mic gain: ${userMicGain.toFixed(2)}x`,
          `output RMS: ${lastOutputRms.toFixed(6)}`,
          `pitch estimate: ${lastPitchHz.toFixed(2)} Hz (clarity ${lastPitchClarity.toFixed(3)})`,
          `speaker-safe mode: ${speakerSafeCheckbox?.checked ?? false}`,
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
    setStatus(`Running at ${sampleRate} Hz (pitch: ${lastPitchHz.toFixed(1)} Hz)`);
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
  lastPitchHz = 0;
  lastPitchClarity = 0;
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

micGainSlider?.addEventListener("input", () => {
  userMicGain = Number.parseFloat(micGainSlider.value) || 1.0;
  updateMicGainLabel();
});

updateMicGainLabel();
