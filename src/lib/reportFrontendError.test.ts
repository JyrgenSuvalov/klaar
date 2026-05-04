import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { reportFrontendError } from "./reportFrontendError";
import { invoke } from "@tauri-apps/api/core";

const invokeMock = invoke as unknown as ReturnType<typeof vi.fn>;

describe("reportFrontendError", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("invokes the report_frontend_error command with the payload nested under `payload`", async () => {
    invokeMock.mockResolvedValue(undefined);
    await reportFrontendError({
      component: "OnboardingErrorBoundary",
      message: "boom",
      stack: "at Foo (foo.tsx:1:1)",
      context: { screen: "install-failure" },
    });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("report_frontend_error", {
      payload: {
        component: "OnboardingErrorBoundary",
        message: "boom",
        stack: "at Foo (foo.tsx:1:1)",
        context: { screen: "install-failure" },
      },
    });
  });

  it("does not throw when the IPC rejects", async () => {
    invokeMock.mockRejectedValue(new Error("ipc broken"));
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(
      reportFrontendError({ component: "Foo", message: "bar" }),
    ).resolves.toBeUndefined();
    expect(err).toHaveBeenCalled();
    err.mockRestore();
  });

  it("forwards minimal payloads with only required fields", async () => {
    invokeMock.mockResolvedValue(undefined);
    await reportFrontendError({ component: "Foo", message: "bar" });
    expect(invokeMock).toHaveBeenCalledWith("report_frontend_error", {
      payload: { component: "Foo", message: "bar" },
    });
  });
});
