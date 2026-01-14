"use strict";

// --- Configuration ---
const MAX_BUFFER_SIZE = 10 * 1024 * 1024; // 10MB
const IDLE_TIME_MS = 2500;

// --- DOM Elements ---
const elements = {
  container: document.getElementById("container"),
  canvas: document.getElementById("canvas"),
  startBtn: document.getElementById("startBtn"),
  stopBtn: document.getElementById("stopBtn"),
  fsBtn: document.getElementById("fsBtn"),
  fpsCounter: document.getElementById("fpsCounter"),
  placeholder: document.getElementById("placeholder"),
  statusDot: document.getElementById("statusDot"),
  statusText: document.getElementById("statusText"),
};

const ctx = elements.canvas.getContext("2d", {
  alpha: false,
  desynchronized: true,
});

// --- State Variables ---
let width = 0,
  height = 0;
let abortController = null;
let isStreaming = false;
let streamBuffer = new Uint8Array(MAX_BUFFER_SIZE);

// Rendering State
let pendingFrame = null; // Holds the latest complete JPEG bytes
let frameCount = 0;
let fpsTimer = null;
let animationId = null;

// --- Event Listeners ---
elements.startBtn.addEventListener("click", startStream);
elements.stopBtn.addEventListener("click", stopStream);
elements.fsBtn.addEventListener("click", toggleFullscreen);
elements.container.addEventListener("mousemove", handleUserActivity);
document.addEventListener("fullscreenchange", handleFullscreenChange);

// --- Core Logic ---

async function startStream() {
  if (isStreaming) return;

  // 1. Fetch Metadata
  try {
    const res = await fetch("/stream/dimensions");
    const data = await res.json();
    width = data.width;
    height = data.height;
    elements.canvas.width = width;
    elements.canvas.height = height;

    if (!document.fullscreenElement) {
      elements.container.style.aspectRatio = `${width}/${height}`;
    }
  } catch (err) {
    console.error("Failed to fetch dimensions:", err);
    return;
  }

  // 2. Initialize State
  abortController = new AbortController();
  isStreaming = true;
  pendingFrame = null;
  frameCount = 0;

  // 3. Update UI
  updateUI(true);

  // 4. Start Loops
  fpsTimer = setInterval(updateFPS, 1000);
  renderLoop(); // Starts the drawing engine
  readStream(abortController.signal); // Starts the network engine
}

function stopStream() {
  if (abortController) abortController.abort();
  cleanup();
}

function cleanup() {
  isStreaming = false;
  if (animationId) cancelAnimationFrame(animationId);
  if (fpsTimer) clearInterval(fpsTimer);
  updateUI(false);
}

// --- Network Engine (Producer) ---
async function readStream(signal) {
  let writeOffset = 0;
  let readOffset = 0;

  try {
    const response = await fetch("/stream", { method: "POST", signal });
    const reader = response.body.getReader();

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;

      // Buffer Management: Compact if needed
      if (writeOffset + value.length > MAX_BUFFER_SIZE) {
        // If we are about to overflow, shift valid data to start
        if (readOffset < writeOffset) {
          const remaining = streamBuffer.subarray(readOffset, writeOffset);
          streamBuffer.set(remaining, 0);
          writeOffset = remaining.length;
          readOffset = 0;
        } else {
          // Hard reset if pointers are bad
          writeOffset = 0;
          readOffset = 0;
        }
      }

      // Append new data
      streamBuffer.set(value, writeOffset);
      writeOffset += value.length;

      // Parse Frames
      // Header: 4 bytes (Little Endian Length)
      while (writeOffset - readOffset >= 4) {
        // Extract length using bitwise ops (fast)
        const frameLen =
          streamBuffer[readOffset] |
          (streamBuffer[readOffset + 1] << 8) |
          (streamBuffer[readOffset + 2] << 16) |
          (streamBuffer[readOffset + 3] << 24);

        // Check if we have the full payload
        if (writeOffset - readOffset >= 4 + frameLen) {
          const start = readOffset + 4;
          const end = start + frameLen;

          // CRITICAL OPTIMIZATION:
          // We slice (copy) the data here. This allows the network loop
          // to immediately overwrite the buffer space without corrupting
          // the frame waiting to be drawn.
          // We overwrite 'pendingFrame', effectively dropping older frames
          // if the renderer is running slower than the network.
          pendingFrame = streamBuffer.slice(start, end);

          readOffset += 4 + frameLen;
        } else {
          // Not enough data for the full frame yet
          break;
        }
      }
    }
  } catch (err) {
    if (err.name !== "AbortError") console.error("Stream error:", err);
  } finally {
    cleanup();
  }
}

// --- Rendering Engine (Consumer) ---
async function renderLoop() {
  if (!isStreaming) return;

  if (pendingFrame) {
    const data = pendingFrame;
    pendingFrame = null; // Clear queue

    try {
      // Create ImageBitmap directly from BufferSource (fastest)
      const blob = new Blob([data], { type: "image/jpeg" });
      const bitmap = await createImageBitmap(blob);

      // Draw and cleanup GPU memory immediately
      ctx.drawImage(bitmap, 0, 0, width, height);
      bitmap.close();

      frameCount++;
    } catch (e) {
      console.error("Render error:", e);
    }
  }

  animationId = requestAnimationFrame(renderLoop);
}

// --- UI Helpers ---
function updateUI(active) {
  if (active) {
    elements.placeholder.classList.add("hidden");
    elements.statusDot.classList.add("live");
    elements.statusText.innerText = "LIVE";
  } else {
    elements.placeholder.classList.remove("hidden");
    elements.statusDot.classList.remove("live");
    elements.statusText.innerText = "OFFLINE";
    elements.fpsCounter.innerText = "";
  }
}

function updateFPS() {
  elements.fpsCounter.innerText = `${frameCount} FPS`;
  frameCount = 0;
}

// --- Fullscreen & Idle Logic ---
let idleTimer;

function toggleFullscreen() {
  const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent);

  if (isIOS) {
    // Fake fullscreen for iOS
    const enabled = container.classList.toggle("mobile-fs");
    document.body.classList.toggle("mobile-fs", enabled);
    return;
  }

  // Standard fullscreen for desktop / Android
  if (!document.fullscreenElement) {
    container.requestFullscreen().catch((err) => {
      console.error(err);
    });
  } else {
    document.exitFullscreen();
  }
}

function handleUserActivity() {
  if (!document.fullscreenElement) return;

  elements.container.classList.remove("user-inactive");
  clearTimeout(idleTimer);

  idleTimer = setTimeout(() => {
    if (document.fullscreenElement) {
      elements.container.classList.add("user-inactive");
    }
  }, IDLE_TIME_MS);
}

function handleFullscreenChange() {
  if (!document.fullscreenElement) {
    clearTimeout(idleTimer);
    elements.container.classList.remove("user-inactive");
    // Restore aspect ratio check in windowed mode
    if (width && height)
      elements.container.style.aspectRatio = `${width}/${height}`;
  }
}
