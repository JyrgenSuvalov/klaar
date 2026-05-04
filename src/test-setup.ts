// Global test setup for Vitest + jsdom
// Runs before each test file via vite.config.ts test.setupFiles

// Canvas API stub — jsdom does not implement canvas rendering,
// so we provide a minimal stub so canvas.getContext() doesn't crash.
if (typeof HTMLCanvasElement !== "undefined") {
  HTMLCanvasElement.prototype.getContext = () => null;
}
