/**
 * Tests for `EnumerationGate` — file-backed `rebootPending` integration.
 *
 * Coverage:
 *   - Mount path: `getRebootPending() === true` → `is_driver_installed`
 *     IPC NOT called AND surface renders `reboot-required`.
 *   - Precedence: sessionStorage seeded with `install-failure` AND
 *     `getRebootPending() === true` → reboot-required wins AND
 *     sessionStorage is cleared.
 *   - Cleared after reboot: `getRebootPending() === false` → normal
 *     enumeration runs (`is_driver_installed` IS called).
 *   - Event-driven re-check: gate is in `main`, focus event fires,
 *     `getRebootPending` flips true → transition to `reboot-required`
 *     without calling `is_driver_installed` for that event.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  render,
  screen,
  waitFor,
  fireEvent,
  act,
} from "@testing-library/react";

// ── Tauri IPC mocks ────────────────────────────────────────────────────────
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("1.0.0"),
}));

// File-backed store mock — the entire reason for this test file.
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
import {
  STORAGE_KEY_ACTIVE_SCREEN,
  STORAGE_KEY_MOUNT_COUNT,
} from "../persistence";
import { invoke } from "@tauri-apps/api/core";
import { getRebootPending } from "@/state/onboardingPersistedStore";

const invokeMock = vi.mocked(invoke);
const getRebootPendingMock = getRebootPending as unknown as ReturnType<
  typeof vi.fn
>;

beforeEach(() => {
  sessionStorage.clear();
  invokeMock.mockReset();
  getRebootPendingMock.mockReset();
  // Default: no enumeration IPCs answer truthfully unless a test overrides.
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "is_driver_installed") return Promise.resolve(true);
    if (cmd === "is_driver_file_present") return Promise.resolve(true);
    if (cmd === "get_installed_driver_version") return Promise.resolve("1.0.0");
    if (cmd === "get_min_driver_version") return Promise.resolve("1.0.0");
    return Promise.resolve(null);
  });
});

describe("EnumerationGate — file-backed rebootPending", () => {
  it("mount path: rebootPending=true short-circuits enumeration and renders reboot-required", async () => {
    getRebootPendingMock.mockResolvedValue(true);

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

    // The enumeration IPC must NOT have been invoked — reboot-pending wins
    // before any device check is attempted.
    expect(
      invokeMock.mock.calls.find(([c]) => c === "is_driver_installed"),
    ).toBeUndefined();
    expect(
      invokeMock.mock.calls.find(([c]) => c === "is_driver_file_present"),
    ).toBeUndefined();
  });

  it("precedence: rebootPending overrides sessionStorage and clears it", async () => {
    // Seed sessionStorage with a stale install-failure (would otherwise
    // sync-rehydrate on first paint).
    sessionStorage.setItem(
      STORAGE_KEY_ACTIVE_SCREEN,
      JSON.stringify({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "disk full" },
      }),
    );
    getRebootPendingMock.mockResolvedValue(true);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    // Initial sync paint: sessionStorage rehydration shows install-failure.
    expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
      "install-failure",
    );

    // After the async rebootPending check resolves, the surface flips and
    // sessionStorage is wiped to prevent resurrection on the next remount.
    await waitFor(() => {
      expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
        "reboot-required",
      );
    });
    expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();

    // Enumeration was still skipped.
    expect(
      invokeMock.mock.calls.find(([c]) => c === "is_driver_installed"),
    ).toBeUndefined();
  });

  it("cleared after reboot: rebootPending=false → normal reevaluate runs", async () => {
    getRebootPendingMock.mockResolvedValue(false);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    // Driver IS installed (default mock) — gate transitions into main UI.
    await waitFor(() => {
      expect(screen.getByText("main-ui")).toBeDefined();
    });
    // The full enumeration path must have executed.
    expect(
      invokeMock.mock.calls.some(([c]) => c === "is_driver_installed"),
    ).toBe(true);
  });

  it("event-driven re-check: focus + flipped rebootPending transitions to reboot-required", async () => {
    // Cold mount: rebootPending false, driver installed → main UI.
    getRebootPendingMock.mockResolvedValue(false);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );
    await waitFor(() => {
      expect(screen.getByText("main-ui")).toBeDefined();
    });

    const enumerationCallsBefore = invokeMock.mock.calls.filter(
      ([c]) => c === "is_driver_installed",
    ).length;

    // The user has installed the driver from somewhere else (or a parallel
    // flow set rebootPending). Now the next focus event should re-check
    // rebootPending FIRST and short-circuit before calling enumeration.
    getRebootPendingMock.mockResolvedValue(true);

    act(() => {
      fireEvent.focus(window);
    });

    await waitFor(() => {
      expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
        "reboot-required",
      );
    });

    // No additional `is_driver_installed` call should have been made by the
    // focus-driven reevaluate — rebootPending check fires before it.
    const enumerationCallsAfter = invokeMock.mock.calls.filter(
      ([c]) => c === "is_driver_installed",
    ).length;
    expect(enumerationCallsAfter).toBe(enumerationCallsBefore);
  });

  it("does not double-trigger heartbeat for rebootPending mounts", () => {
    // Sanity: heartbeat is scoped to sessionStorage rehydration. A pure
    // rebootPending mount (no persisted screen) must
    // not bump the heartbeat counter into a false-positive remount.
    sessionStorage.setItem(STORAGE_KEY_MOUNT_COUNT, "1");
    getRebootPendingMock.mockResolvedValue(true);

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    // Counter ticks but no rehydrated screen → no heartbeat fired (asserted
    // via the existing rehydration test suite); here we just confirm the
    // counter is incremented as expected.
    expect(sessionStorage.getItem(STORAGE_KEY_MOUNT_COUNT)).toBe("2");
  });
});
