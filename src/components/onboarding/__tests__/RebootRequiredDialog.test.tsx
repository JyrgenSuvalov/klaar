/**
 * Tests for `RebootRequiredDialog`.
 *
 * Coverage:
 *   - Renders heading and copy
 *   - Reboot Now → invokes `request_reboot` IPC
 *   - UserCancelled → dialog stays open, no error banner
 *   - OsascriptFailed → inline error banner with stderr; reboot button re-enabled
 *   - Later → calls `setRebootPending` (idempotent re-affirm) and parent's onLater
 *   - Reboot Now disables both buttons during the in-flight request
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@/state/onboardingPersistedStore", () => ({
  setRebootPending: vi.fn().mockResolvedValue(undefined),
  getRebootPending: vi.fn().mockResolvedValue(false),
  clearRebootPending: vi.fn().mockResolvedValue(undefined),
}));

import { invoke } from "@tauri-apps/api/core";
import { setRebootPending } from "@/state/onboardingPersistedStore";
import { RebootRequiredDialog } from "../RebootRequiredDialog";

const invokeMock = vi.mocked(invoke);
const setRebootPendingMock = setRebootPending as unknown as ReturnType<
  typeof vi.fn
>;

beforeEach(() => {
  invokeMock.mockReset();
  setRebootPendingMock.mockReset();
  setRebootPendingMock.mockResolvedValue(undefined);
});

describe("RebootRequiredDialog", () => {
  it("renders the heading and explanatory copy", () => {
    render(<RebootRequiredDialog onLater={() => {}} />);
    expect(screen.getByText("Klaar driver installed")).toBeDefined();
    expect(screen.getAllByText(/Klaar/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Reboot Now" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Later" })).toBeDefined();
  });

  it("invokes request_reboot when Reboot Now is clicked", async () => {
    invokeMock.mockResolvedValue(undefined);
    render(<RebootRequiredDialog onLater={() => {}} />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Reboot Now" }));
      await Promise.resolve();
    });

    expect(invokeMock).toHaveBeenCalledWith("request_reboot");
  });

  it("does not surface an error banner when the user cancels the macOS prompt", async () => {
    invokeMock.mockRejectedValue({ kind: "UserCancelled" });
    render(<RebootRequiredDialog onLater={() => {}} />);

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Reboot Now" }));
    });
    // No alert role; the modal stays open and clean.
    expect(screen.queryByRole("alert")).toBeNull();
    // Button re-enabled so the user can click again without remounting.
    await waitFor(() => {
      expect(
        screen.getByRole<HTMLButtonElement>("button", { name: "Reboot Now" })
          .disabled,
      ).toBe(false);
    });
  });

  it("renders an inline error banner with stderr on OsascriptFailed", async () => {
    invokeMock.mockRejectedValue({
      kind: "OsascriptFailed",
      message: "execution error: -1743",
    });
    render(<RebootRequiredDialog onLater={() => {}} />);

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Reboot Now" }));
    });

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Couldn't initiate restart");
    expect(alert.textContent).toContain("execution error: -1743");
    // Manual reboot instruction surfaced for the user.
    expect(alert.textContent).toContain("Apple menu");
  });

  it("normalises non-tagged rejections into OsascriptFailed", async () => {
    // Simulates a Rust panic / bridge error that doesn't carry the tagged
    // shape — the dialog must still render an actionable banner instead of
    // crashing.
    invokeMock.mockRejectedValue("transport closed");
    render(<RebootRequiredDialog onLater={() => {}} />);

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Reboot Now" }));
    });

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("transport closed");
  });

  it("calls setRebootPending and onLater when Later is clicked", async () => {
    const onLater = vi.fn();
    render(<RebootRequiredDialog onLater={onLater} />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Later" }));
      await Promise.resolve();
    });

    expect(setRebootPendingMock).toHaveBeenCalledTimes(1);
    expect(onLater).toHaveBeenCalledTimes(1);
  });

  it("still calls onLater if setRebootPending throws (best-effort persistence)", async () => {
    setRebootPendingMock.mockRejectedValueOnce(new Error("disk full"));
    const onLater = vi.fn();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    render(<RebootRequiredDialog onLater={onLater} />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Later" }));
      await Promise.resolve();
    });

    expect(onLater).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("disables both buttons while the reboot request is in flight", async () => {
    let resolveInvoke: (() => void) | null = null;
    invokeMock.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveInvoke = resolve;
        }),
    );
    render(<RebootRequiredDialog onLater={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "Reboot Now" }));

    // Async state update from setRebooting(true).
    await waitFor(() => {
      expect(
        screen.getByRole<HTMLButtonElement>("button", { name: /Restarting/ })
          .disabled,
      ).toBe(true);
    });
    expect(
      screen.getByRole<HTMLButtonElement>("button", { name: "Later" }).disabled,
    ).toBe(true);

    await act(async () => {
      resolveInvoke?.();
      await Promise.resolve();
    });
  });
});