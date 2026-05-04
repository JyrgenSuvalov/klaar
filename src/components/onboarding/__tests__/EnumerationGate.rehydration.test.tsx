import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

// ── Tauri IPC mock ─────────────────────────────────────────────────────────
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("1.0.0"),
}));

// Mock the surface so we can assert which screen it received without
// rendering full dialog trees (which would invoke their own IPC).
vi.mock("../OnboardingSurface", () => ({
  OnboardingSurface: (props: {
    initialScreen: { kind: string; error?: unknown };
  }) => (
    <div data-testid="onboarding-surface">
      <div data-testid="surface-screen-kind">{props.initialScreen.kind}</div>
      <div data-testid="surface-error-payload">
        {JSON.stringify(props.initialScreen.error ?? null)}
      </div>
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
import { reportFrontendError } from "@/lib/reportFrontendError";

const invokeMock = vi.mocked(invoke);
const reportMock = vi.mocked(reportFrontendError);

describe("EnumerationGate — rehydration & heartbeat", () => {
  beforeEach(() => {
    sessionStorage.clear();
    invokeMock.mockReset();
    reportMock.mockClear();
  });

  it("rehydrates a persisted install-failure on first paint without invoking IPC", () => {
    sessionStorage.setItem(
      STORAGE_KEY_ACTIVE_SCREEN,
      JSON.stringify({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "disk full" },
      }),
    );

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    // First-paint assertion: the surface received the install-failure screen
    // BEFORE any reevaluate IPC was invoked.
    expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
      "install-failure",
    );
    const payload = JSON.parse(
      screen.getByTestId("surface-error-payload").textContent ?? "null",
    ) as { kind: string; message: string };
    expect(payload).toEqual({ kind: "CopyFailed", message: "disk full" });

    // is_driver_installed must NOT have been called as part of mount-time
    // reevaluate — that would race with the rehydrated screen.
    expect(invokeMock).not.toHaveBeenCalledWith("is_driver_installed");
  });

  it("rehydrates a persisted update-required on first paint", () => {
    sessionStorage.setItem(
      STORAGE_KEY_ACTIVE_SCREEN,
      JSON.stringify({ kind: "update-required" }),
    );
    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );
    expect(screen.getByTestId("surface-screen-kind").textContent).toBe(
      "update-required",
    );
  });

  it("discards malformed persisted state and proceeds with reevaluate", async () => {
    sessionStorage.setItem(
      STORAGE_KEY_ACTIVE_SCREEN,
      JSON.stringify({ kind: "garbage" }),
    );
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "is_driver_installed") return Promise.resolve(true);
      if (cmd === "get_installed_driver_version")
        return Promise.resolve("1.0.0");
      if (cmd === "get_min_driver_version") return Promise.resolve("1.0.0");
      return Promise.resolve(null);
    });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    // The corrupt entry should be cleared on read.
    expect(sessionStorage.getItem(STORAGE_KEY_ACTIVE_SCREEN)).toBeNull();
    // Reevaluate should run and find the driver installed → main UI.
    await waitFor(() => {
      expect(screen.getByText("main-ui")).toBeDefined();
    });
    warn.mockRestore();
  });

  it("does not log the heartbeat on first mount of a session", () => {
    // Persist a screen, but mountCount is 1 (fresh session).
    sessionStorage.setItem(
      STORAGE_KEY_ACTIVE_SCREEN,
      JSON.stringify({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "boom" },
      }),
    );

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    expect(reportMock).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(STORAGE_KEY_MOUNT_COUNT)).toBe("1");
  });

  it("logs a remount heartbeat when gate mounts again with a persisted screen", () => {
    // Simulate a prior mount in this session by pre-seeding the counter to 1
    // and keeping the persisted screen.
    sessionStorage.setItem(STORAGE_KEY_MOUNT_COUNT, "1");
    sessionStorage.setItem(
      STORAGE_KEY_ACTIVE_SCREEN,
      JSON.stringify({
        kind: "install-failure",
        error: { kind: "CopyFailed", message: "boom" },
      }),
    );

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    expect(reportMock).toHaveBeenCalledTimes(1);
    const call = reportMock.mock.calls[0][0];
    expect(call.component).toBe("EnumerationGate");
    expect(call.message).toContain("webview remounted");
    expect(call.context?.mountCount).toBe(2);
    expect(call.context?.persistedScreenKind).toBe("install-failure");
  });

  it("does not log heartbeat on remount when no persisted screen exists", () => {
    sessionStorage.setItem(STORAGE_KEY_MOUNT_COUNT, "1");
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "is_driver_installed") return Promise.resolve(true);
      if (cmd === "get_installed_driver_version")
        return Promise.resolve("1.0.0");
      if (cmd === "get_min_driver_version") return Promise.resolve("1.0.0");
      return Promise.resolve(null);
    });

    render(
      <EnumerationGate>
        <div>main-ui</div>
      </EnumerationGate>,
    );

    expect(reportMock).not.toHaveBeenCalled();
  });
});
