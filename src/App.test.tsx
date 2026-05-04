import { vi } from "vitest";
import { render } from "@testing-library/react";

// ── Mocks ──────────────────────────────────────────────────────────────────
//
// App pulls in stores, IPC, and many child components. We stub the children
// (which themselves invoke Tauri IPC at mount) and drive the state via the
// real Zustand stores — that lets us assert the rendered banner DOM directly
// without spinning up the full panel tree.

vi.mock("@tauri-apps/api/core", () => ({
  // Stub invoke per command — different App-mount IPC calls expect different shapes.
  invoke: vi.fn((cmd: string) => {
    switch (cmd) {
      case "list_input_devices":
      case "list_output_devices":
        return Promise.resolve([]);
      case "list_profiles":
        return Promise.resolve([]);
      case "restore_session":
        return Promise.resolve({
          settings: { activeProfileId: null, autoLaunch: false },
          profile: null,
          warnings: [],
        });
      default:
        return Promise.resolve(undefined);
    }
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@/components/DeviceSelector", () => ({
  DeviceSelector: () => <div data-testid="device-selector" />,
}));
vi.mock("@/components/ProcessingToggle", () => ({
  ProcessingToggle: () => <div />,
}));
vi.mock("@/components/ProfileSelector", () => ({
  ProfileSelector: () => <div />,
}));
vi.mock("@/components/ProfileActions", () => ({
  ProfileActions: () => <div />,
}));
vi.mock("@/components/SettingsPanel", () => ({
  SettingsPanel: () => <div />,
}));
vi.mock("@/components/controls/Meter", () => ({
  Meter: () => <div data-testid="meter" />,
}));
vi.mock("@/components/panels/NoiseGatePanel", () => ({
  NoiseGatePanel: () => <div />,
}));
vi.mock("@/components/panels/DeEsserPanel", () => ({
  DeEsserPanel: () => <div />,
}));
vi.mock("@/components/panels/CompressorPanel", () => ({
  CompressorPanel: () => <div />,
}));
vi.mock("@/components/panels/LimiterPanel", () => ({
  LimiterPanel: () => <div />,
}));
vi.mock("@/components/eq/EqPanel", () => ({
  EqPanel: () => <div />,
}));
vi.mock("@/components/panels/RecordPlaybackPanel", () => ({
  RecordPlaybackPanel: () => <div />,
}));
vi.mock("@/hooks/useMicPermissionRecheck", () => ({
  useMicPermissionRecheck: () => {},
}));

import App from "./App";
import { useEngineStore } from "@/store/engineStore";
import { useProfileStore } from "@/store/profileStore";

describe("App error banners", () => {
  beforeEach(() => {
    useEngineStore.setState({ engineError: null });
    useProfileStore.setState({ profileError: null });
  });

  it("engine error banner has role=alert", () => {
    useEngineStore.setState({
      engineError: { type: "generic", message: "boom" },
    });
    const { getAllByRole } = render(<App />);
    const alerts = getAllByRole("alert");
    expect(alerts.some((el) => el.textContent?.includes("boom"))).toBe(true);
  });

  it("profile error banner has role=alert", () => {
    useProfileStore.setState({ profileError: "Failed to save profile" });
    const { getAllByRole } = render(<App />);
    const alerts = getAllByRole("alert");
    expect(
      alerts.some((el) => el.textContent?.includes("Failed to save profile")),
    ).toBe(true);
  });
});
