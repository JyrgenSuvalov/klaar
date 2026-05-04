import type { ReactNode } from "react";

/**
 * Shared full-window modal shell for onboarding / repair dialogs.
 *
 * Implements the fail-hard gate: the backdrop is opaque (not a translucent
 * overlay on top of the main UI) because these dialogs replace the main
 * shell rather than layer over it. No close affordance — the gate itself
 * controls dismissal.
 */
export function ModalShell({ children }: { children: ReactNode }) {
  return (
    <div
      className="fixed inset-0 flex items-center justify-center p-6"
      style={{ backgroundColor: "var(--color-background)", color: "var(--color-text-primary)" }}
    >
      <div
        className="w-full max-w-lg rounded-lg border shadow-2xl"
        style={{
          backgroundColor: "var(--color-surface)",
          borderColor: "var(--color-border)",
        }}
      >
        {children}
      </div>
    </div>
  );
}

export function ModalHeader({ children }: { children: ReactNode }) {
  return (
    <div className="px-6 pt-6 pb-3">
      <h2 className="text-lg font-semibold tracking-tight">{children}</h2>
    </div>
  );
}

export function ModalBody({ children }: { children: ReactNode }) {
  return (
    <div className="px-6 pb-4 text-sm" style={{ color: "var(--color-text-secondary)" }}>
      {children}
    </div>
  );
}

export function ModalFooter({ children }: { children: ReactNode }) {
  return (
    <div
      className="flex items-center justify-end gap-2 border-t px-6 py-3"
      style={{ borderColor: "var(--color-border)" }}
    >
      {children}
    </div>
  );
}

type ButtonProps = {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  variant?: "primary" | "secondary" | "ghost";
  title?: string;
};

export function ModalButton({
  children,
  onClick,
  disabled,
  variant = "primary",
  title,
}: ButtonProps) {
  const base =
    "rounded-md px-4 py-1.5 text-sm font-medium transition-colors disabled:opacity-60 disabled:cursor-not-allowed cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]";

  const styles: Record<string, React.CSSProperties> = {
    primary: {
      backgroundColor: "var(--color-accent)",
      color: "#0a0a0a",
      border: "1px solid transparent",
    },
    secondary: {
      backgroundColor: "transparent",
      color: "var(--color-text-primary)",
      border: "1px solid var(--color-border)",
    },
    ghost: {
      backgroundColor: "transparent",
      color: "var(--color-text-secondary)",
      border: "1px solid transparent",
    },
  };

  return (
    <button
      className={base}
      onClick={onClick}
      disabled={disabled}
      title={title}
      style={styles[variant]}
    >
      {children}
    </button>
  );
}
