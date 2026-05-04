/**
 * Tests for `MuteButton`.
 *
 * Coverage:
 *   - Renders the unmuted state (mic glyph, aria-pressed=false, "Mute" copy)
 *   - Renders the muted state (mic-off glyph, aria-pressed=true, "Muted" copy,
 *     `data-muted="true"` for the lit-red styling hook)
 *   - Click invokes the `toggle_mute` IPC
 *   - A `klaar://mute-changed` event payload mutating the store flips the
 *     rendered state without a re-click (proves the event listener path
 *     drives the UI, not the click)
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

import { invoke } from "@tauri-apps/api/core";
import { MuteButton } from "../MuteButton";
import { useEngineStore } from "@/store/engineStore";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(false);
  // Reset just the mute slice — leave the rest of the store untouched so
  // unrelated test files in the same Vitest run don't see polluted state.
  useEngineStore.setState({ muted: false });
});

describe("MuteButton", () => {
  it("renders the unmuted state by default", () => {
    render(<MuteButton />);
    const btn = screen.getByRole("button", { name: "Mute microphone" });
    expect(btn.getAttribute("aria-pressed")).toBe("false");
    expect(btn.getAttribute("data-muted")).toBe("false");
    // The visible label reads "Mute" while unmuted (the aria-label
    // carries the longer accessible copy). Note: the button also contains
    // a visually-hidden "Muted" span as a width-stabilizer (see the
    // `aria-hidden` grid in MuteButton.tsx), so `textContent` always
    // includes "Muted" regardless of state — assert via `aria-pressed`
    // / `data-muted` / the accessible name instead. The unmuted state is
    // already covered by `getByRole({ name: "Mute microphone" })` above.
    expect(btn.textContent).toContain("Mute");
    // Accelerator hint is part of the button text so it's discoverable.
    expect(btn.textContent).toContain("⌃⇧M");
  });

  it("renders the muted state when the store flag is true", () => {
    useEngineStore.setState({ muted: true });
    render(<MuteButton />);
    const btn = screen.getByRole("button", { name: "Unmute microphone" });
    expect(btn.getAttribute("aria-pressed")).toBe("true");
    expect(btn.getAttribute("data-muted")).toBe("true");
    expect(btn.textContent).toContain("Muted");
    // Accelerator hint persists in both states.
    expect(btn.textContent).toContain("⌃⇧M");
  });

  it("invokes toggle_mute IPC on click", () => {
    render(<MuteButton />);
    fireEvent.click(screen.getByRole("button", { name: "Mute microphone" }));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("toggle_mute");
  });

  it("flips rendered state when the store mute flag changes (event-driven)", () => {
    render(<MuteButton />);
    expect(
      screen.getByRole("button", { name: "Mute microphone" }),
    ).toBeDefined();

    // Simulate the `klaar://mute-changed` listener writing into the store.
    // No click; this proves the listener path is the source of truth and
    // the button is a pure projection of `engineStore.muted`.
    act(() => {
      useEngineStore.setState({ muted: true });
    });

    expect(
      screen.getByRole("button", { name: "Unmute microphone" }),
    ).toBeDefined();
    // And the lit-red style hook should track too.
    expect(
      screen
        .getByRole("button", { name: "Unmute microphone" })
        .getAttribute("data-muted"),
    ).toBe("true");
  });
});
