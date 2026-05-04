/**
 * Tests for `EnumerationGate` — file-backed `pendingPostInstallApps` integration.
 *
 * Covers the post-reboot conferencing-apps screen flow: when the user
 * completes a fresh install, `OnboardingSurface` writes both `rebootPending`
 * AND `pendingPostInstallApps`. The reboot auto-clears `rebootPending`, so
 * on the next launch the gate sees `is_driver_installed === true` AND a
 * sticky `pendingPostInstallApps`. It must route through the conferencing
 * screen exactly once before landing the user on main UI.
 *
 * Coverage:
 *   - flag set + driver installed → renders `post-install-apps`.
 *   - flag absent + driver installed → renders main UI (regression guard).
 *   - flag set BUT rebootPending also true → reboot-required wins (the
 *     post-install screen is downstream of the reboot gate).
 *   - flag set BUT enumeration fails → main UI (fail-open; the screen is
 *     helpful, not load-bearing).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("1.0.0"),
}));

vi.mock("@/state/onboardingPersistedStore", () => ({
  getRebootPending: vi.fn().mockResolvedValue(false),
  setRebootPending: vi.fn().mockResolvedValue(undefined),
  clearRebootPending: vi.fn().mockResolvedValue(undefined),
  getPendingPostInstallApps: vi.fn().mockResolvedValue(false),
  setPendingPostInstallApps: vi.fn().mockResolvedValue(undefined),
  clearPendingPostInstallApps: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../OnboardingSurface", () => ({
  OnboardingSurface: (props: { initialScreen: { kind: string } }) => (
    <div data-testid="onboarding-surface">
      <div data-testid="surface-screen-kind">{props.initialScreen.kind}</div>
    </div>
  ),
}));

vi.mock("../OnboardingErrorBoundary", () => ({
  OnboardingErrorBoundary: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
}));

vi.mock("../UpdateAvailableBanner", () => ({
  UpdateAvailableBanner: () => null,
}));

vi.mock("@/lib/reportFrontendError", () => ({
  reportFrontendError: vi.fn().mockResolvedValue(undefined),
}));

import { EnumerationGate } from "../EnumerationGate";
import { invoke } from "@tauri-apps/api/core";
import {
  getPendingPostInstallApps,
  getRebootPending,
} from "@/state/onboardingPersistedStore";

const invokeMock = vi.mocked(invoke);
const getRebootPendingMock = getRebootPending as unknown as ReturnType<
  typeof vi.fn
>;
const getPendingPostInstallAppsMock =
  getPendingPostInstallApps as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  sessionStorage.clear();
  invokeMock.mockReset();
  getRebootPendingMock.mockReset();
  getPendingPostInstallAppsMock.mockReset();
  // Default happy-path: driver installed, version current.
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "is_driver_installed") return Promise.resolve(true);
    if (cmd === "is_driver_file_present") return Promise.resolve(true);
    if (cmd === "get_installed_driver_version") return Promise.resolve("1.0.0");
    if (cmd === "get_min_driver_version") return Promise.resolve("1.0.0");
    return Promise.resolve(null);
  });
  getRebootPendingMock.mockResolvedValue(false);
  getPendingPostInstallAppsMock.mockResolvedValue(false);
});

describe("EnumerationGate — pendingPostInstallApps", () => {
  it("driver installed + flag set → routes through post-install-apps", async () => {
    getPendingPostInstallAppsMock.mockResolvedValue(true);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
        "post-install-apps",
      );
    });
    // Main UI must NOT have rendered — the screen is gating it.
    expect(screen.queryByText("main-ui")).toBeNull();
  });

  it("driver installed + flag absent → main UI (regression guard)", async () => {
    getPendingPostInstallAppsMock.mockResolvedValue(false);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    await waitFor(() => {
      expect(screen.getByText("main-ui")).toBeDefined();
    });
    expect(screen.queryByTestId("surface-screen-kind")).toBeNull();
  });

  it("rebootPending wins over pendingPostInstallApps — reboot gate is upstream", async () => {
    getRebootPendingMock.mockResolvedValue(true);
    getPendingPostInstallAppsMock.mockResolvedValue(true);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
        "reboot-required",
      );
    });
    // Enumeration short-circuits before the post-install check is even
    // reached, so the flag-read mock should never have been called.
    expect(getPendingPostInstallAppsMock).not.toHaveBeenCalled();
  });

  it("flag-read failure falls through to main UI (fail-open)", async () => {
    getPendingPostInstallAppsMock.mockRejectedValue(new Error("disk error"));

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    await waitFor(() => {
      expect(screen.getByText("main-ui")).toBeDefined();
    });
    expect(screen.queryByTestId("surface-screen-kind")).toBeNull();
  });
});
