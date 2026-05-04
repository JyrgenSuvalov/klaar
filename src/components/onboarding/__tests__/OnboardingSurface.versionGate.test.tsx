/**
 * Tests for the version-gate update flow through `OnboardingSurface`.
 *
 * Why this lives separately from `OnboardingSurface.persistence.test.tsx`:
 * that suite mocks `DriverUpdateRequiredDialog` to a stub that calls
 * `onUpdated` directly. Here we deliberately DO NOT mock that dialog — the
 * point is to verify that the real dialog's `invoke("install_driver")` call
 * routes back through `handleInstallSuccess`, which writes the file-backed
 * reboot-pending flag and transitions to `reboot-required`.
 *
 * Coverage:
 *   - update-required → click Update → install_driver resolves →
 *     `setRebootPending` called → screen becomes `reboot-required`
 *   - install_driver rejects with `CopyFailed` → screen becomes
 *     `install-failure` (the existing failure path is preserved for updates)
 *   - install_driver rejects with `UserCancelled` → screen stays on
 *     `update-required`, no `setRebootPending` write
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
  getPendingPostInstallApps: vi.fn().mockResolvedValue(false),
  setPendingPostInstallApps: vi.fn().mockResolvedValue(undefined),
  clearPendingPostInstallApps: vi.fn().mockResolvedValue(undefined),
}));

// Mock the RebootRequiredDialog so we can assert the transition without
// engaging the real dialog's IPCs. Identical pattern to
// OnboardingSurface.persistence.test.tsx.
vi.mock("../RebootRequiredDialog", () => ({
  RebootRequiredDialog: () => <div data-testid="reboot-required-dialog" />,
}));

// Failure path renders InstallFailureDialog — stub it so we can detect the
// transition without depending on its internals.
vi.mock("../InstallFailureDialog", () => ({
  InstallFailureDialog: (props: { error: { kind: string; message?: string } }) => (
    <div data-testid="install-failure-dialog">
      <div data-testid="error-kind">{props.error.kind}</div>
    </div>
  ),
}));

// NOTE: `DriverUpdateRequiredDialog` is intentionally NOT mocked — the test
// drives the real dialog's button to exercise its `invoke("install_driver")`
// call path.

import { invoke } from "@tauri-apps/api/core";
import { setRebootPending } from "@/state/onboardingPersistedStore";
import { OnboardingSurface } from "../OnboardingSurface";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;
const setRebootPendingMock = setRebootPending as unknown as ReturnType<
  typeof vi.fn
>;

beforeEach(() => {
  invokeMock.mockReset();
  setRebootPendingMock.mockReset();
  setRebootPendingMock.mockResolvedValue(undefined);
  sessionStorage.clear();
});

describe("OnboardingSurface — version-gate update flow", () => {
  it("transitions to reboot-required and writes the flag when Update Driver succeeds", async () => {
    invokeMock.mockResolvedValue(undefined);

    render(
      <OnboardingSurface
        initialScreen={{ kind: "update-required" }}
        onDismiss={() => {}}
        installedVersion="0.9.0"
        minVersion="1.0.0"
      />,
    );

    // Sanity: we start on the update-required dialog (real one this time).
    expect(
      screen.getByRole("button", { name: "Update Driver" }),
    ).toBeDefined();

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Update Driver" }));
    });

    // The dialog's handleUpdate calls `invoke("install_driver")` …
    expect(invokeMock).toHaveBeenCalledWith("install_driver");

    // … and on success bubbles up to handleInstallSuccess, which writes the
    // file-backed flag and routes to the reboot-required screen.
    await waitFor(() => {
      expect(screen.getByTestId("reboot-required-dialog")).toBeDefined();
    });
    expect(setRebootPendingMock).toHaveBeenCalledTimes(1);
  });

  it("routes to install-failure when install_driver rejects with CopyFailed", async () => {
    invokeMock.mockRejectedValue({ kind: "CopyFailed", message: "disk full" });

    render(
      <OnboardingSurface
        initialScreen={{ kind: "update-required" }}
        onDismiss={() => {}}
        installedVersion="0.9.0"
        minVersion="1.0.0"
      />,
    );

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Update Driver" }));
    });

    await waitFor(() => {
      expect(screen.getByTestId("install-failure-dialog")).toBeDefined();
    });
    expect(screen.getByTestId("error-kind").textContent).toBe("CopyFailed");
    // Critically: failed updates must NOT write the reboot-pending flag —
    // the on-disk bundle was not replaced, so a reboot is meaningless.
    expect(setRebootPendingMock).not.toHaveBeenCalled();
  });

  it("stays on update-required and does not write the flag when the user cancels the macOS prompt", async () => {
    invokeMock.mockRejectedValue({ kind: "UserCancelled" });

    render(
      <OnboardingSurface
        initialScreen={{ kind: "update-required" }}
        onDismiss={() => {}}
        installedVersion="0.9.0"
        minVersion="1.0.0"
      />,
    );

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Update Driver" }));
      await Promise.resolve();
    });

    // Still on the update-required dialog — UserCancelled is silent in the
    // dialog (matches DriverInstallDialog behaviour).
    expect(
      screen.getByRole("button", { name: "Update Driver" }),
    ).toBeDefined();
    expect(screen.queryByTestId("reboot-required-dialog")).toBeNull();
    expect(screen.queryByTestId("install-failure-dialog")).toBeNull();
    expect(setRebootPendingMock).not.toHaveBeenCalled();
  });
});
