/**
 * Tests for `<UpdateResultDialog>`. Coverage:
 *   - Each terminal-state payload renders the expected copy + buttons.
 *   - `available` does NOT auto-dismiss; `up_to_date` and `error` do.
 *   - Swapping the payload resets the auto-dismiss timer.
 *   - Escape closes the dialog (clearing the store payload).
 *   - "Open release page" calls `openUrl` and dismisses.
 *   - "Later" dismisses without opening any URL.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

import { openUrl } from "@tauri-apps/plugin-opener";

import { UpdateResultDialog } from "../UpdateResultDialog";
import { useAboutStore } from "@/state/aboutStore";
import {
  useUpdateResultDialogStore,
  type UpdateManualResultPayload,
} from "@/state/updateResultDialogStore";

const openUrlMock = openUrl as unknown as ReturnType<typeof vi.fn>;

const upToDate: UpdateManualResultPayload = {
  kind: "up_to_date",
  checked_at: "2026-05-05T00:00:00Z",
};
const available: UpdateManualResultPayload = {
  kind: "available",
  tag: "v1.4.0",
  url: "https://example.com/release/v1.4.0",
  checked_at: "2026-05-05T00:00:00Z",
};
const networkError: UpdateManualResultPayload = {
  kind: "error",
  error: "network",
  checked_at: "2026-05-05T00:00:00Z",
};
const rateLimited: UpdateManualResultPayload = {
  kind: "error",
  error: "rate_limited",
  checked_at: "2026-05-05T00:00:00Z",
};
const parseError: UpdateManualResultPayload = {
  kind: "error",
  error: "parse",
  checked_at: "2026-05-05T00:00:00Z",
};

beforeEach(() => {
  openUrlMock.mockReset();
  openUrlMock.mockResolvedValue(undefined);
  useUpdateResultDialogStore.setState({ payload: null });
  useAboutStore.setState({ aboutOpen: false, version: "0.1.0" });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("UpdateResultDialog", () => {
  it("renders nothing when payload is null", () => {
    render(<UpdateResultDialog />);
    expect(screen.queryByText(/Klaar/)).toBeNull();
  });

  it("renders the up_to_date copy with the cached app version and an OK button", () => {
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(upToDate);
    });
    expect(screen.getByText("You're up to date.")).toBeDefined();
    // Body uses the cached version from `useAboutStore`.
    expect(screen.getByText(/0\.1\.0/)).toBeDefined();
    expect(screen.getByRole("button", { name: "OK" })).toBeDefined();
    expect(screen.queryByRole("button", { name: /Open release/ })).toBeNull();
  });

  it("renders the available copy with the tag and the two action buttons", () => {
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(available);
    });
    expect(screen.getByText(/v1\.4\.0 is available\./)).toBeDefined();
    expect(screen.getByRole("button", { name: "Open release page" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Later" })).toBeDefined();
    expect(screen.queryByRole("button", { name: "OK" })).toBeNull();
  });

  it("renders per-kind error copy", () => {
    const { rerender } = render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(networkError);
    });
    expect(screen.getByText(/Couldn't reach GitHub/)).toBeDefined();

    act(() => {
      useUpdateResultDialogStore.getState().show(rateLimited);
    });
    rerender(<UpdateResultDialog />);
    expect(screen.getByText(/rate-limiting/)).toBeDefined();

    act(() => {
      useUpdateResultDialogStore.getState().show(parseError);
    });
    rerender(<UpdateResultDialog />);
    expect(screen.getByText(/Couldn't parse/)).toBeDefined();
  });

  it("auto-dismisses up_to_date after ~5 s", () => {
    vi.useFakeTimers();
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(upToDate);
    });
    expect(useUpdateResultDialogStore.getState().payload).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(useUpdateResultDialogStore.getState().payload).toBeNull();
  });

  it("auto-dismisses error after ~5 s", () => {
    vi.useFakeTimers();
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(networkError);
    });
    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(useUpdateResultDialogStore.getState().payload).toBeNull();
  });

  it("does NOT auto-dismiss available", () => {
    vi.useFakeTimers();
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(available);
    });
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(useUpdateResultDialogStore.getState().payload).toEqual(available);
  });

  it("resets the auto-dismiss timer when the payload swaps", () => {
    vi.useFakeTimers();
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(networkError);
    });
    // 4 s into the network-error timer — almost expiring.
    act(() => {
      vi.advanceTimersByTime(4_000);
    });
    expect(useUpdateResultDialogStore.getState().payload).toEqual(networkError);
    // Swap to a fresh error payload — timer should reset.
    act(() => {
      useUpdateResultDialogStore.getState().show(rateLimited);
    });
    // 4 s after the swap — still mounted because the timer reset.
    act(() => {
      vi.advanceTimersByTime(4_000);
    });
    expect(useUpdateResultDialogStore.getState().payload).toEqual(rateLimited);
    // 1 more second — total 5 s since the swap — now dismissed.
    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    expect(useUpdateResultDialogStore.getState().payload).toBeNull();
  });

  it("Escape dismisses the dialog (store payload becomes null)", async () => {
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(upToDate);
    });
    act(() => {
      fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });
    });
    await waitFor(() => {
      expect(useUpdateResultDialogStore.getState().payload).toBeNull();
    });
  });

  it("Open release page invokes openUrl and dismisses", async () => {
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(available);
    });
    fireEvent.click(screen.getByRole("button", { name: "Open release page" }));
    expect(openUrlMock).toHaveBeenCalledWith(
      "https://example.com/release/v1.4.0",
    );
    await waitFor(() => {
      expect(useUpdateResultDialogStore.getState().payload).toBeNull();
    });
  });

  it("Later dismisses without opening any URL", async () => {
    render(<UpdateResultDialog />);
    act(() => {
      useUpdateResultDialogStore.getState().show(available);
    });
    fireEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(openUrlMock).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(useUpdateResultDialogStore.getState().payload).toBeNull();
    });
  });
});
