import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen } from "@testing-library/react";

// ── Tauri IPC mock ─────────────────────────────────────────────────────────
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { DriverInstallDialog } from "../DriverInstallDialog";
import { invoke } from "@tauri-apps/api/core";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

/**
 * Progressive copy buckets advance at 5 / 20 / 45 s while
 * `install_driver` is in flight. Backend stays a single `invoke()` returning
 * `Result<(), InstallError>`; the only signal driving the UI is wall time.
 */
describe("DriverInstallDialog — progressive install copy", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  /** Resolve the in-flight `install_driver` invoke with the given outcome. */
  function makeDeferredInvoke() {
    let resolve!: () => void;
    let reject!: (err: unknown) => void;
    const promise = new Promise<void>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    invokeMock.mockReturnValue(promise);
    return { resolve, reject };
  }

  function clickInstall() {
    const btn = screen.getByRole("button", { name: /install driver/i });
    act(() => {
      btn.click();
    });
  }

  it("starts on the first bucket and advances at 5 / 20 / 45 s thresholds", () => {
    makeDeferredInvoke();

    render(
      <DriverInstallDialog
        onSuccess={vi.fn()}
        onFailure={vi.fn()}
      />,
    );

    // Idle: live region is empty — no premature announcement.
    const liveRegion = screen.getByRole("status");
    expect(liveRegion.getAttribute("aria-live")).toBe("polite");
    expect(liveRegion.getAttribute("aria-atomic")).toBe("true");
    expect(liveRegion.textContent).toBe("");

    clickInstall();

    // Bucket 0 — 0 to <5 s.
    expect(liveRegion.textContent).toBe("Installing driver…");
    act(() => {
      vi.advanceTimersByTime(4_999);
    });
    expect(liveRegion.textContent).toBe("Installing driver…");

    // Bucket 1 — 5 to <20 s.
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(liveRegion.textContent).toBe("Finalising install…");
    act(() => {
      vi.advanceTimersByTime(15_000 - 1);
    });
    expect(liveRegion.textContent).toBe("Finalising install…");

    // Bucket 2 — 20 to <45 s.
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(liveRegion.textContent).toContain("Taking longer than usual");

    // Bucket 3 — 45 s onward.
    act(() => {
      vi.advanceTimersByTime(25_000);
    });
    expect(liveRegion.textContent).toContain("Almost done");
  });

  it("clears the live region and resets the bucket on success", async () => {
    const { resolve } = makeDeferredInvoke();
    const onSuccess = vi.fn();

    render(
      <DriverInstallDialog
        onSuccess={onSuccess}
        onFailure={vi.fn()}
      />,
    );

    clickInstall();
    act(() => {
      vi.advanceTimersByTime(25_000);
    });
    expect(screen.getByRole("status").textContent).toContain(
      "Taking longer than usual",
    );

    // Resolve the invoke; React needs a microtask flush before the finally
    // block runs and `installing` flips back to false.
    await act(async () => {
      resolve();
      await Promise.resolve();
    });

    expect(onSuccess).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toBe("");
  });

  it("resets to bucket 0 after a UserCancelled retry", async () => {
    // First invoke → UserCancelled. We drive timers far enough to land in
    // bucket 2 before the rejection, then assert the next click starts
    // fresh at bucket 0 rather than inheriting the prior bucket index.
    const first = makeDeferredInvoke();

    render(
      <DriverInstallDialog
        onSuccess={vi.fn()}
        onFailure={vi.fn()}
      />,
    );

    clickInstall();
    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(screen.getByRole("status").textContent).toContain(
      "Taking longer than usual",
    );

    await act(async () => {
      first.reject({ kind: "UserCancelled" });
      // Two flushes: one for the rejection, one for the finally setState.
      await Promise.resolve();
      await Promise.resolve();
    });

    // Live region cleared while idle.
    expect(screen.getByRole("status").textContent).toBe("");

    // Retry — second invoke is also deferred so we can inspect mid-flight.
    makeDeferredInvoke();
    clickInstall();
    expect(screen.getByRole("status").textContent).toBe("Installing driver…");
    act(() => {
      vi.advanceTimersByTime(4_999);
    });
    expect(screen.getByRole("status").textContent).toBe("Installing driver…");
  });

  it("clears scheduled bucket transitions on failure", async () => {
    const { reject } = makeDeferredInvoke();
    const onFailure = vi.fn();

    render(
      <DriverInstallDialog
        onSuccess={vi.fn()}
        onFailure={onFailure}
      />,
    );

    clickInstall();
    act(() => {
      vi.advanceTimersByTime(3_000);
    });
    expect(screen.getByRole("status").textContent).toBe("Installing driver…");

    // `RestartFailed` was retired. Use the surviving `CopyFailed` variant —
    // the test's intent (cleanup-on-error) is variant-agnostic.
    await act(async () => {
      reject({ kind: "CopyFailed", message: "boom" });
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(onFailure).toHaveBeenCalledWith({ kind: "CopyFailed", message: "boom" });

    // Advancing past the next threshold must NOT resurrect a stale message —
    // the cleanup should have torn down all pending timeouts.
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByRole("status").textContent).toBe("");
  });
});
