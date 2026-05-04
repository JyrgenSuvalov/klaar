/**
 * Layered-info-dialog primitive.
 *
 * This is the shadcn-style wrapper over `@radix-ui/react-dialog` for
 * **dismissable, layered information dialogs** — the About dialog is the
 * first consumer; Preferences / Help / Check-for-Updates are the obvious
 * future ones. The backdrop is translucent so any underlying surface
 * (including a `ModalShell` fail-hard gate) remains partially visible.
 *
 * NOT to be confused with `src/components/onboarding/ModalShell.tsx`, which
 * is the **fail-hard onboarding gate** primitive: opaque backdrop, no close
 * affordance, replaces the main shell. Reach for `ModalShell` when the user
 * MUST resolve the gate to continue, and reach for this `Dialog` primitive
 * when the user is asking for information that should layer above whatever
 * is already on screen.
 */

import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";
import { a11y } from "@/i18n/a11yStrings";

const Dialog = DialogPrimitive.Root;
const DialogTrigger = DialogPrimitive.Trigger;
const DialogPortal = DialogPrimitive.Portal;
const DialogClose = DialogPrimitive.Close;

const DialogOverlay = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    style={{ zIndex: 9998 }}
    className={cn(
      // Translucent so an underlying ModalShell gate remains partially
      // visible — the layering rule from the spec is "About sits above the
      // gate, gate stays readable underneath".
      "fixed inset-0 bg-black/60 backdrop-blur-sm",
      "data-[state=open]:animate-in data-[state=closed]:animate-out",
      "data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
      className,
    )}
    {...props}
  />
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

const DialogContent = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    /** Hide the built-in close (X) button — useful when the dialog renders
     *  its own primary close action and a duplicate would be visual noise. */
    hideCloseButton?: boolean;
  }
>(({ className, children, hideCloseButton, ...props }, ref) => (
  <DialogPortal>
    <DialogOverlay />
    <DialogPrimitive.Content
      ref={ref}
      style={{ zIndex: 9999 }}
      className={cn(
        // Centre the dialog with a fixed-position transform so it lays out
        // independently of any parent overflow rules.
        "fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2",
        "w-full max-w-md rounded-lg border shadow-2xl",
        "bg-[var(--color-surface)] border-[var(--color-border)]",
        "text-[var(--color-text-primary)]",
        "p-6",
        "data-[state=open]:animate-in data-[state=closed]:animate-out",
        "data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
        "data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95",
        "focus:outline-none",
        className,
      )}
      {...props}
    >
      {children}
      {!hideCloseButton && (
        <DialogPrimitive.Close
          aria-label={a11y.dialogClose()}
          className={cn(
            "absolute right-3 top-3 rounded-sm p-1 transition-colors",
            "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]",
            "focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)]",
            "disabled:pointer-events-none cursor-pointer",
          )}
        >
          <X className="h-4 w-4" />
        </DialogPrimitive.Close>
      )}
    </DialogPrimitive.Content>
  </DialogPortal>
));
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogTitle = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("text-lg font-semibold tracking-tight", className)}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ComponentRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn(
      "text-sm text-[var(--color-text-secondary)]",
      className,
    )}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogTrigger,
  DialogPortal,
  DialogOverlay,
  DialogContent,
  DialogTitle,
  DialogDescription,
  DialogClose,
};
