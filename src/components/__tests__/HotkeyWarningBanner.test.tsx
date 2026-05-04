/**
 * Tests for `HotkeyWarningBanner`.
 *
 * Coverage:
 *   - Renders nothing when `get_hotkey_warning` returns `null`.
 *   - Renders the banner with the exact returned string when it returns
 *     `Some(message)` — no string duplication on the frontend, the backend
 *     copy in `HOTKEY_WARNING_MESSAGE` is the source of truth.
 *   - Dismiss click hides the banner without re-invoking the IPC.
 *   - The IPC is invoked **exactly once** across two mount/unmount cycles
 *     in the same render lifecycle — regression guard for the swap-on-read
 *     contract on the backend (mirrors the `get_app_version` shape in
 *     `AboutDialog.test.tsx`).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import {
  HotkeyWarningBanner,
  __resetHotkeyWarningCheckForTest,
} from "../HotkeyWarningBanner";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

const MESSAGE = "Couldn't register ⌃⇧M — the mute button still works.";

beforeEach(() => {
  invokeMock.mockReset();
  // Reset the module-level "already fetched" flag — see the banner's
  // file docstring for why it lives at module scope.
  __resetHotkeyWarningCheckForTest();
});

describe("HotkeyWarningBanner", () => {
  it("renders nothing when get_hotkey_warning returns null", async () => {
    invokeMock.mockResolvedValue(null);
    render(<HotkeyWarningBanner />);

    // Let the mount-effect's promise settle before asserting absence.
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.queryByRole("alert")).toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_hotkey_warning");
  });

  it("renders the banner with the exact message when IPC returns Some", async () => {
    invokeMock.mockResolvedValue(MESSAGE);
    render(<HotkeyWarningBanner />);

    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain(MESSAGE);
    // Dismiss button is present and labelled via the a11y module.
    expect(
      screen.getByRole("button", { name: "Dismiss hotkey warning" }),
    ).toBeDefined();
  });

  it("dismiss click hides the banner without re-invoking the IPC", async () => {
    invokeMock.mockResolvedValue(MESSAGE);
    render(<HotkeyWarningBanner />);

    await screen.findByRole("alert");
    expect(invokeMock).toHaveBeenCalledTimes(1);

    fireEvent.click(
      screen.getByRole("button", { name: "Dismiss hotkey warning" }),
    );

    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
    // No second IPC — the backend already cleared its flag on the first read.
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("invokes get_hotkey_warning exactly once across two mount/unmount cycles", async () => {
    invokeMock.mockResolvedValue(MESSAGE);

    // Cycle 1
    const first = render(<HotkeyWarningBanner />);
    await screen.findByRole("alert");
    first.unmount();

    // Cycle 2 — same render lifecycle (no module reset between)
    const second = render(<HotkeyWarningBanner />);
    await act(async () => {
      // Flush any (incorrect) effect dispatch
      await Promise.resolve();
    });
    second.unmount();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_hotkey_warning");
  });
});
