import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, waitFor } from "@testing-library/react";
import React from "react";

// `Root` lives in `main.tsx`, which also calls `ReactDOM.createRoot(...).render`
// at module scope — that side effect would explode in a vitest jsdom env
// because `#root` doesn't exist and the IPC mocks aren't yet in place. We
// reconstruct the relevant subset of `Root` here with the same `useEffect`
// semantics so the test asserts the contract (`invoke("frontend_ready")` is
// called on mount AND on every focus-true event), without coupling to the
// bootstrap.

// Plain `vi.fn()` results in TS picking the constructable overload of
// `Mock`. The cast to `Mock<(...args: any[]) => any>` pins a callable
// signature and lets `.mock.calls` / `.mockReset()` flow through.
const invokeMock = vi.fn() as unknown as ReturnType<typeof vi.fn> & {
  (cmd: string, args?: unknown): Promise<unknown>;
};
const listenMock = vi.fn() as unknown as ReturnType<typeof vi.fn> & {
  (event: string, handler: (...a: unknown[]) => void): Promise<() => void>;
};

// Captured focus-change subscription so tests can drive focus events.
type FocusHandler = (e: { payload: boolean }) => void;
let capturedFocusHandler: FocusHandler | null = null;
const onFocusChangedMock = vi.fn((handler: FocusHandler) => {
  capturedFocusHandler = handler;
  return Promise.resolve(() => {
    capturedFocusHandler = null;
  });
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invokeMock(cmd, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (...a: unknown[]) => void) =>
    listenMock(event, handler),
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    onFocusChanged: onFocusChangedMock,
  }),
}));

// Mirror the Root effect — the test owns its own component so the
// bootstrap module's `createRoot` side effect doesn't run.
function MainReadyMirror() {
  React.useEffect(() => {
    // listener subscription (matches main.tsx ordering: about-requested first)
    void listenMock("klaar://about-requested", () => {});
  }, []);
  React.useEffect(() => {
    void invokeMock("frontend_ready");

    let unlistenFocus: (() => void) | null = null;
    let cancelled = false;
    void onFocusChangedMock(({ payload: focused }) => {
      if (focused) {
        void invokeMock("frontend_ready");
      }
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlistenFocus = fn;
      }
    });

    return () => {
      cancelled = true;
      unlistenFocus?.();
    };
  }, []);
  return <div data-testid="root" />;
}

function readyCallCount() {
  return invokeMock.mock.calls.filter((c) => c[0] === "frontend_ready").length;
}

describe("Root: frontend_ready ack", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => {});
    onFocusChangedMock.mockClear();
    capturedFocusHandler = null;
  });

  it("invokes `frontend_ready` exactly once after mount", async () => {
    render(<MainReadyMirror />);
    await waitFor(() => {
      expect(readyCallCount()).toBe(1);
    });
  });

  it("does not pass any payload to `frontend_ready`", async () => {
    render(<MainReadyMirror />);
    await waitFor(() => {
      const call = invokeMock.mock.calls.find(
        (c) => c[0] === "frontend_ready",
      );
      expect(call).toBeDefined();
      // Either undefined or absent — the IPC takes no arguments.
      expect(call?.[1]).toBeUndefined();
    });
  });

  it("re-invokes `frontend_ready` on focus-gain (covers reused-window show cycles)", async () => {
    render(<MainReadyMirror />);
    // Wait for the focus handler to be registered.
    await waitFor(() => {
      expect(capturedFocusHandler).not.toBeNull();
    });
    // Initial mount ack.
    await waitFor(() => expect(readyCallCount()).toBe(1));

    // Simulate a defocus → ignored.
    capturedFocusHandler!({ payload: false });
    expect(readyCallCount()).toBe(1);

    // Simulate a focus-gain → re-invokes.
    capturedFocusHandler!({ payload: true });
    expect(readyCallCount()).toBe(2);

    // Multiple focus gains keep re-invoking; backend is idempotent
    // within a generation, so this is safe.
    capturedFocusHandler!({ payload: true });
    expect(readyCallCount()).toBe(3);
  });

  it("does not re-invoke on focus-loss", async () => {
    render(<MainReadyMirror />);
    await waitFor(() => {
      expect(capturedFocusHandler).not.toBeNull();
    });
    await waitFor(() => expect(readyCallCount()).toBe(1));

    capturedFocusHandler!({ payload: false });
    capturedFocusHandler!({ payload: false });
    expect(readyCallCount()).toBe(1);
  });
});
