import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportFrontendError } from "@/lib/reportFrontendError";
import { ModalShell, ModalHeader, ModalBody, ModalFooter, ModalButton } from "./ModalShell";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * React error boundary wrapping the onboarding surface.
 *
 * Converts silent JS render crashes into actionable backend log entries via
 * the `report_frontend_error` IPC, and renders a minimal fallback UI with a
 * Retry affordance that re-mounts the boundary's children.
 *
 * Scope: wraps the entire onboarding flow rather than individual dialogs —
 * a render error in any child should produce the same fallback UX, and the
 * report payload includes the React component stack so resolution doesn't
 * suffer from the wider scope.
 */
export class OnboardingErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    // Fire-and-forget; reportFrontendError swallows IPC failures so the
    // boundary will always render its fallback even if logging itself fails.
    void reportFrontendError({
      component: "OnboardingSurface",
      message: error.message || "<no message>",
      stack: error.stack,
      context: {
        componentStack: errorInfo.componentStack ?? null,
      },
    });
  }

  private handleRetry = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    if (this.state.error === null) {
      return this.props.children;
    }
    return (
      <ModalShell>
        <ModalHeader>Something went wrong loading the setup screen</ModalHeader>
        <ModalBody>
          <p>
            An unexpected error happened while rendering the onboarding flow. The error has
            been logged so we can investigate.
          </p>
          <p className="mt-2">
            Click <strong>Retry</strong> to try again. If the problem persists, please
            restart Klaar.
          </p>
          <pre
            className="mt-3 max-h-32 overflow-auto rounded px-2 py-1.5 text-[10px] font-mono whitespace-pre-wrap break-all"
            style={{
              backgroundColor: "var(--color-background)",
              color: "var(--color-text-primary)",
              border: "1px solid var(--color-border)",
            }}
          >
            {this.state.error.message || "<no message>"}
          </pre>
        </ModalBody>
        <ModalFooter>
          <ModalButton onClick={this.handleRetry}>Retry</ModalButton>
        </ModalFooter>
      </ModalShell>
    );
  }
}
