"use strict";

// ===========================
// Configuration
// ===========================
const CONFIG = {
  MAX_BUFFER: 10 * 1024 * 1024, // 10MB
  IDLE_TIMEOUT: 2500,
  FPS_UPDATE_INTERVAL: 1000,
  ENDPOINTS: {
    dimensions: "/stream/dimensions",
    stream: "/stream"
  }
};

// ===========================
// DOM Cache
// ===========================
const $ = {
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

// ===========================
// State Management
// ===========================
const state = {
  width: 0,
  height: 0,
  isStreaming: false,
  abortController: null,
  buffer: new Uint8Array(CONFIG.MAX_BUFFER),
  writeOffset: 0,
  readOffset: 0,
  pendingFrame: null,
  frameCount: 0,
  fpsInterval: null,
  rafId: null,
  idleTimer: null,
};

// Canvas context with optimizations
const ctx = $.canvas.getContext("2d", {
  alpha: false,
  desynchronized: true,
  willReadFrequently: false,
});

// ===========================
// Feature Detection
// ===========================
const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent);
const supportsOffscreenCanvas = typeof OffscreenCanvas !== 'undefined';

// ===========================
// Event Delegation
// ===========================
const handlers = {
  start: () => startStream(),
  stop: () => stopStream(),
  fullscreen: () => toggleFullscreen(),
  activity: () => handleActivity(),
  fsChange: () => handleFullscreenChange(),
};

// Attach listeners
$.startBtn.addEventListener("click", handlers.start, { passive: true });
$.stopBtn.addEventListener("click", handlers.stop, { passive: true });
$.fsBtn.addEventListener("click", handlers.fullscreen, { passive: true });
$.container.addEventListener("mousemove", handlers.activity, { passive: true });
$.container.addEventListener("touchstart", handlers.activity, { passive: true });
document.addEventListener("fullscreenchange", handlers.fsChange, { passive: true });

// ===========================
// Stream Lifecycle
// ===========================
async function startStream() {
  if (state.isStreaming) return;

  try {
    await fetchDimensions();
    initializeStream();
    updateUI(true);
    
    // Start concurrent loops
    state.fpsInterval = setInterval(updateFPS, CONFIG.FPS_UPDATE_INTERVAL);
    renderLoop();
    readStream(state.abortController.signal);
  } catch (err) {
    console.error("Failed to start stream:", err);
    cleanup();
  }
}

function stopStream() {
  state.abortController?.abort();
  cleanup();
}

function cleanup() {
  state.isStreaming = false;
  state.abortController = null;
  state.pendingFrame = null;
  state.writeOffset = 0;
  state.readOffset = 0;
  
  if (state.rafId) {
    cancelAnimationFrame(state.rafId);
    state.rafId = null;
  }
  
  if (state.fpsInterval) {
    clearInterval(state.fpsInterval);
    state.fpsInterval = null;
  }
  
  updateUI(false);
}

// ===========================
// Dimension Fetching
// ===========================
async function fetchDimensions() {
  const res = await fetch(CONFIG.ENDPOINTS.dimensions);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  
  const { width, height } = await res.json();
  state.width = width;
  state.height = height;
  
  $.canvas.width = width;
  $.canvas.height = height;
  
  if (!document.fullscreenElement) {
    $.container.style.aspectRatio = `${width}/${height}`;
  }
}

function initializeStream() {
  state.abortController = new AbortController();
  state.isStreaming = true;
  state.frameCount = 0;
  state.writeOffset = 0;
  state.readOffset = 0;
}

// ===========================
// Network Engine (Producer)
// ===========================
async function readStream(signal) {
  try {
    const res = await fetch(CONFIG.ENDPOINTS.stream, { 
      method: "POST", 
      signal 
    });
    
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    
    const reader = res.body.getReader();
    
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      
      appendToBuffer(value);
      parseFrames();
    }
  } catch (err) {
    if (err.name !== "AbortError") {
      console.error("Stream error:", err);
    }
  } finally {
    cleanup();
  }
}

// ===========================
// Buffer Management
// ===========================
function appendToBuffer(chunk) {
  const needed = state.writeOffset + chunk.length;
  
  // Compact buffer if overflow imminent
  if (needed > CONFIG.MAX_BUFFER) {
    const validSize = state.writeOffset - state.readOffset;
    
    if (validSize > 0) {
      state.buffer.copyWithin(0, state.readOffset, state.writeOffset);
      state.writeOffset = validSize;
      state.readOffset = 0;
    } else {
      // Reset on corruption
      state.writeOffset = 0;
      state.readOffset = 0;
    }
  }
  
  state.buffer.set(chunk, state.writeOffset);
  state.writeOffset += chunk.length;
}

function parseFrames() {
  const buf = state.buffer;
  
  while (state.writeOffset - state.readOffset >= 4) {
    // Fast 32-bit little-endian read
    const len = buf[state.readOffset] |
                (buf[state.readOffset + 1] << 8) |
                (buf[state.readOffset + 2] << 16) |
                (buf[state.readOffset + 3] << 24);
    
    const totalSize = 4 + len;
    
    if (state.writeOffset - state.readOffset < totalSize) {
      break; // Incomplete frame
    }
    
    const start = state.readOffset + 4;
    const end = start + len;
    
    // Extract frame (copy for safety)
    state.pendingFrame = buf.slice(start, end);
    state.readOffset += totalSize;
  }
}

// ===========================
// Render Engine (Consumer)
// ===========================
function renderLoop() {
  if (!state.isStreaming) return;
  
  if (state.pendingFrame) {
    drawFrame(state.pendingFrame);
    state.pendingFrame = null;
  }
  
  state.rafId = requestAnimationFrame(renderLoop);
}

async function drawFrame(data) {
  try {
    const blob = new Blob([data], { type: "image/jpeg" });
    const bitmap = await createImageBitmap(blob, {
      resizeQuality: "low", // Faster decoding
      premultiplyAlpha: "none",
    });
    
    ctx.drawImage(bitmap, 0, 0, state.width, state.height);
    bitmap.close();
    
    state.frameCount++;
  } catch (err) {
    console.error("Render error:", err);
  }
}

// ===========================
// UI Updates
// ===========================
function updateUI(active) {
  const method = active ? "add" : "remove";
  
  $.placeholder.classList[active ? "add" : "remove"]("hidden");
  $.statusDot.classList[method]("live");
  $.statusText.textContent = active ? "LIVE" : "OFFLINE";
  
  if (!active) {
    $.fpsCounter.textContent = "";
  }
}

function updateFPS() {
  $.fpsCounter.textContent = `${state.frameCount} FPS`;
  state.frameCount = 0;
}

// ===========================
// Fullscreen Management
// ===========================
function toggleFullscreen() {
  if (isIOS) {
    // iOS fallback
    const active = $.container.classList.toggle("mobile-fs");
    document.body.classList.toggle("mobile-fs", active);
    return;
  }
  
  if (document.fullscreenElement) {
    document.exitFullscreen();
  } else {
    $.container.requestFullscreen({ navigationUI: "hide" })
      .catch(err => console.error("Fullscreen error:", err));
  }
}

function handleActivity() {
  if (!document.fullscreenElement) return;
  
  $.container.classList.remove("user-inactive");
  clearTimeout(state.idleTimer);
  
  state.idleTimer = setTimeout(() => {
    if (document.fullscreenElement) {
      $.container.classList.add("user-inactive");
    }
  }, CONFIG.IDLE_TIMEOUT);
}

function handleFullscreenChange() {
  if (!document.fullscreenElement) {
    clearTimeout(state.idleTimer);
    $.container.classList.remove("user-inactive");
    
    if (state.width && state.height) {
      $.container.style.aspectRatio = `${state.width}/${state.height}`;
    }
  }
}

// ===========================
// Cleanup on Page Unload
// ===========================
window.addEventListener("beforeunload", () => {
  stopStream();
}, { passive: true });