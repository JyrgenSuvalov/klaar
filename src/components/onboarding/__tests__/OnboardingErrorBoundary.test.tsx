import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@/lib/reportFrontendError", () => ({
  reportFrontendError: vi.fn().mockResolvedValue(undefined),
}));

import { OnboardingErrorBoundary } from "../OnboardingErrorBoundary";
import { reportFrontendError } from "@/lib/reportFrontendError";

const reportMock = vi.mocked(reportFrontendError);

function ThrowOnce({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) throw new Error("kaboom");
  return <div>healthy children</div>;
}

describe("OnboardingErrorBoundary", () => {
  beforeEach(() => {
    reportMock.mockClear();
  });

  it("renders children when they don't throw", () => {
    render(
      <OnboardingErrorBoundary>
        <div>hello</div>
      </OnboardingErrorBoundary>,
    );
    expect(screen.getByText("hello")).toBeDefined();
    expect(reportMock).not.toHaveBeenCalled();
  });

  it("renders the fallback and reports when a child throws", () => {
    // Suppress jsdom's automatic re-throw / console.error chatter from React.
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <OnboardingErrorBoundary>
        <ThrowOnce shouldThrow={true} />
      </OnboardingErrorBoundary>,
    );
    expect(screen.getByText(/Something went wrong loading the setup screen/i)).toBeDefined();
    expect(screen.getByRole("button", { name: /retry/i })).toBeDefined();
    expect(reportMock).toHaveBeenCalledTimes(1);
    const call = reportMock.mock.calls[0][0];
    expect(call.component).toBe("OnboardingSurface");
    expect(call.message).toBe("kaboom");
    expect(typeof call.stack === "string" || call.stack === undefined).toBe(true);
    expect(call.context).toBeDefined();
    consoleError.mockRestore();
  });

  it("re-renders children when Retry is clicked after the child stops throwing", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    // Use a controlled component whose throw flag we can flip via re-render.
    const { rerender } = render(
      <OnboardingErrorBoundary>
        <ThrowOnce shouldThrow={true} />
      </OnboardingErrorBoundary>,
    );
    expect(screen.getByText(/Something went wrong/i)).toBeDefined();

    // The outer parent has fixed the underlying issue — children no longer throw.
    rerender(
      <OnboardingErrorBoundary>
        <ThrowOnce shouldThrow={false} />
      </OnboardingErrorBoundary>,
    );
    // Boundary still showing fallback because internal state hasn't reset.
    expect(screen.getByText(/Something went wrong/i)).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: /retry/i }));

    expect(screen.queryByText(/Something went wrong/i)).toBeNull();
    expect(screen.getByText("healthy children")).toBeDefined();
    consoleError.mockRestore();
  });

  it("still renders fallback when reportFrontendError itself rejects", () => {
    reportMock.mockRejectedValueOnce(new Error("ipc broken"));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <OnboardingErrorBoundary>
        <ThrowOnce shouldThrow={true} />
      </OnboardingErrorBoundary>,
    );
    expect(screen.getByText(/Something went wrong/i)).toBeDefined();
    consoleError.mockRestore();
  });
});
