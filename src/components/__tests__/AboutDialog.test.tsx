/**
 * Tests for `AboutDialog`.
 *
 * Coverage:
 *   - The store-backed event flow (calling `openAbout()` renders the dialog)
 *   - Each link/icon button click invokes `openUrl` with the spec's exact URL
 *   - Escape closes the About dialog without disturbing an underlying ModalShell
 *   - Escape closes the dialog (`aboutOpen === false`)
 *   - `get_app_version` is invoked exactly once across two open/close/open
 *     cycles in the same render lifecycle
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { AboutDialog } from "../AboutDialog";
import { useAboutStore } from "@/state/aboutStore";
import { ModalShell, ModalHeader } from "@/components/onboarding/ModalShell";

const openUrlMock = openUrl as unknown as ReturnType<typeof vi.fn>;
const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  openUrlMock.mockReset();
  openUrlMock.mockResolvedValue(undefined);
  invokeMock.mockReset();
  invokeMock.mockResolvedValue("0.1.0");
  // Reset the in-memory store between tests — Zustand stores are shared
  // singletons across the test file otherwise.
  useAboutStore.setState({ aboutOpen: false, version: null });
});

describe("AboutDialog", () => {
  it("renders the dialog content when openAbout() is called", () => {
    render(<AboutDialog />);
    expect(screen.queryByText("Klaar")).toBeNull();

    act(() => {
      useAboutStore.getState().openAbout();
    });

    expect(screen.getByText("Klaar")).toBeDefined();
    expect(screen.getByText("Jürgen Šuvalov")).toBeDefined();
    expect(screen.getByRole("button", { name: "jyrkki.eu" })).toBeDefined();
    expect(screen.getByRole("button", { name: /Email/ })).toBeDefined();
    expect(screen.getByRole("button", { name: /Bluesky/ })).toBeDefined();
    expect(screen.getByRole("button", { name: /Mastodon/ })).toBeDefined();
  });

  it("invokes openUrl with the website URL when the text link is clicked", () => {
    render(<AboutDialog />);
    act(() => {
      useAboutStore.getState().openAbout();
    });

    fireEvent.click(screen.getByRole("button", { name: "jyrkki.eu" }));
    expect(openUrlMock).toHaveBeenCalledWith("https://jyrkki.eu");
  });

  it("invokes openUrl with mailto on the Email button", () => {
    render(<AboutDialog />);
    act(() => {
      useAboutStore.getState().openAbout();
    });

    fireEvent.click(screen.getByRole("button", { name: /Email/ }));
    expect(openUrlMock).toHaveBeenCalledWith("mailto:hey@jyrkki.eu");
  });

  it("invokes openUrl with the Bluesky profile URL", () => {
    render(<AboutDialog />);
    act(() => {
      useAboutStore.getState().openAbout();
    });

    fireEvent.click(screen.getByRole("button", { name: /Bluesky/ }));
    expect(openUrlMock).toHaveBeenCalledWith(
      "https://bsky.app/profile/jyrkki.eu",
    );
  });

  it("invokes openUrl with the Mastodon profile URL", () => {
    render(<AboutDialog />);
    act(() => {
      useAboutStore.getState().openAbout();
    });

    fireEvent.click(screen.getByRole("button", { name: /Mastodon/ }));
    expect(openUrlMock).toHaveBeenCalledWith("https://est.social/@jyrkki");
  });

  it("Escape closes the About dialog (aboutOpen becomes false)", async () => {
    render(<AboutDialog />);
    act(() => {
      useAboutStore.getState().openAbout();
    });
    expect(useAboutStore.getState().aboutOpen).toBe(true);

    act(() => {
      fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });
    });

    await waitFor(() => {
      expect(useAboutStore.getState().aboutOpen).toBe(false);
    });
  });

  it("Escape closes only the About dialog and leaves a ModalShell intact", async () => {
    render(
      <>
        <ModalShell>
          <ModalHeader>Pretend gate</ModalHeader>
        </ModalShell>
        <AboutDialog />
      </>,
    );
    act(() => {
      useAboutStore.getState().openAbout();
    });

    expect(screen.getByText("Pretend gate")).toBeDefined();
    expect(screen.getByText("Klaar")).toBeDefined();

    act(() => {
      fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });
    });

    await waitFor(() => {
      expect(useAboutStore.getState().aboutOpen).toBe(false);
    });
    // ModalShell content is still in the document — the gate does not
    // listen to Escape, so it stays untouched.
    expect(screen.getByText("Pretend gate")).toBeDefined();
  });

  it("invokes get_app_version exactly once across two open/close/open cycles", async () => {
    render(<AboutDialog />);

    // Cycle 1
    act(() => {
      useAboutStore.getState().openAbout();
    });
    await waitFor(() => {
      expect(useAboutStore.getState().version).toBe("0.1.0");
    });

    act(() => {
      useAboutStore.getState().closeAbout();
    });

    // Cycle 2 — version is cached, so no second invoke
    act(() => {
      useAboutStore.getState().openAbout();
    });
    await act(async () => {
      // Let any (incorrect) effect dispatch flush
      await Promise.resolve();
    });

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_app_version");
  });
});
