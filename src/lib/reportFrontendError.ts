import { invoke } from "@tauri-apps/api/core";

/**
 * Payload for the `report_frontend_error` Tauri command. Mirrors the Rust
 * `FrontendErrorPayload` shape in `src-tauri/src/commands/system.rs`.
 *
 * `component` and `message` are required. `stack` and `context` are optional.
 * The backend clamps each string field to 4 KiB; oversized values are not
 * rejected, just truncated with a `…[truncated]` suffix.
 */
export interface FrontendErrorPayload {
  component: string;
  message: string;
  stack?: string;
  context?: Record<string, unknown>;
}

/**
 * Send a structured render-error report to the Rust log pipeline. Used by
 * `OnboardingErrorBoundary` and the gate's webview-remount heartbeat
 * (diagnostics).
 *
 * Never throws — the caller is typically already in a degraded state and a
 * reporting failure should not cascade. On rejection the error is logged to
 * `console.error` so it is at least visible in DevTools during development.
 */
export async function reportFrontendError(
  payload: FrontendErrorPayload,
): Promise<void> {
  try {
    await invoke("report_frontend_error", { payload });
  } catch (e) {
    // Swallow — the boundary fallback UI must still render even if the IPC
    // itself fails (e.g. backend command not registered, capability denied).
    console.error("[reportFrontendError] IPC failed:", e, "payload:", payload);
  }
}
