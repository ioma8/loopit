const statusEl = document.getElementById("status");
const startBtn = document.getElementById("start");
const stopBtn = document.getElementById("stop");

let audioCtx = null;
let sourceNode = null;
let processorNode = null;
let stream = null;
let wasm = null;
let wasmPtr = 0;

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

    const module = await loadWasm();

    stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      },
    });

    audioCtx = new AudioContext({ latencyHint: "interactive" });
    const sampleRate = audioCtx.sampleRate;
    wasmPtr = module.loopit_new(sampleRate);

    sourceNode = audioCtx.createMediaStreamSource(stream);

    // ScriptProcessor is simple and broadly supported for this demo.
    processorNode = audioCtx.createScriptProcessor(256, 1, 1);
    processorNode.onaudioprocess = (event) => {
      if (!wasmPtr) {
        return;
      }

      const input = event.inputBuffer.getChannelData(0);
      const output = event.outputBuffer.getChannelData(0);

      for (let i = 0; i < input.length; i += 1) {
        output[i] = module.loopit_process(wasmPtr, input[i]);
      }
    };

    sourceNode.connect(processorNode);
    processorNode.connect(audioCtx.destination);

    stopBtn.disabled = false;
    setStatus(`Running at ${sampleRate} Hz`);
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

  if (wasm && wasmPtr) {
    wasm.loopit_free(wasmPtr);
    wasmPtr = 0;
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
