const statusEl = document.getElementById("status");
const startBtn = document.getElementById("start");
const stopBtn = document.getElementById("stop");

let audioCtx = null;
let sourceNode = null;
let processorNode = null;
let stream = null;
let wasm = null;
let wasmPtr = null;

function setStatus(message) {
  statusEl.textContent = message;
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

    if (audioCtx.state !== "running") {
      await audioCtx.resume();
    }

    const sampleRate = audioCtx.sampleRate;
    wasmPtr = module.loopit_new(sampleRate);

    sourceNode = audioCtx.createMediaStreamSource(stream);

    // ScriptProcessor is simple and broadly supported for this demo.
    // Firefox can expose microphone input with more than one channel, so we
    // accept stereo here and mix down manually in the callback.
    processorNode = audioCtx.createScriptProcessor(256, 2, 1);
    processorNode.onaudioprocess = (event) => {
      if (wasmPtr === null) {
        return;
      }

      const inputChannels = event.inputBuffer.numberOfChannels;
      const output = event.outputBuffer.getChannelData(0);

      for (let i = 0; i < output.length; i += 1) {
        let mono = 0;

        for (let channel = 0; channel < inputChannels; channel += 1) {
          mono += event.inputBuffer.getChannelData(channel)[i] ?? 0;
        }

        if (inputChannels > 0) {
          mono /= inputChannels;
        }

        output[i] = module.loopit_process(wasmPtr, mono);
      }
    };

    sourceNode.connect(processorNode);
    processorNode.connect(audioCtx.destination);

    if (audioCtx.state !== "running") {
      await audioCtx.resume();
    }

    stopBtn.disabled = false;
    setStatus(`Running at ${sampleRate} Hz (context: ${audioCtx.state}, ptr: ${wasmPtr})`);
  } catch (error) {
    console.error(error);
    setStatus(`Error: ${error.message}`);
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
}

startBtn.addEventListener("click", () => {
  start();
});

stopBtn.addEventListener("click", () => {
  stop();
});
