/**
 * About dialog.
 *
 * Layered info dialog opened from the tray right-click menu. Subscribes to
 * `useAboutStore` for visibility, fetches the running binary's version on
 * first open via the `get_app_version` IPC and caches it for the session,
 * and renders author / contact metadata with link buttons that route through
 * `tauri-plugin-opener::openUrl`.
 *
 * Layering: this dialog is mounted at the React tree top level (sibling to
 * `<EnumerationGate>`) so its Radix portal renders above any active
 * `ModalShell` fail-hard gate. Closing it leaves the gate untouched.
 */

import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
  DialogClose,
} from "@/components/ui/dialog";
import { BlueskyIcon } from "@/components/icons/BlueskyIcon";
import { MastodonIcon } from "@/components/icons/MastodonIcon";
import { EmailIcon } from "@/components/icons/EmailIcon";
import { useAboutStore } from "@/state/aboutStore";

const URL_WEBSITE = "https://jyrkki.eu";
const URL_EMAIL = "mailto:hey@jyrkki.eu";
const URL_BLUESKY = "https://bsky.app/profile/jyrkki.eu";
const URL_MASTODON = "https://est.social/@jyrkki";

function safeOpenUrl(url: string) {
  void openUrl(url).catch((e) => {
    console.warn("[AboutDialog] openUrl failed:", e);
  });
}

export function AboutDialog() {
  const aboutOpen = useAboutStore((s) => s.aboutOpen);
  const closeAbout = useAboutStore((s) => s.closeAbout);
  const version = useAboutStore((s) => s.version);
  const setVersion = useAboutStore((s) => s.setVersion);

  // Fetch the running binary's version once on first open and cache it for
  // the session. Subsequent opens MUST NOT re-invoke (spec scenario "Version
  // is fetched once per session").
  useEffect(() => {
    if (aboutOpen && version === null) {
      void invoke<string>("get_app_version")
        .then((v) => setVersion(v))
        .catch((e) => {
          console.warn("[AboutDialog] get_app_version failed:", e);
        });
    }
  }, [aboutOpen, version, setVersion]);

  return (
    <Dialog
      open={aboutOpen}
      onOpenChange={(open) => {
        if (!open) closeAbout();
      }}
    >
      <DialogContent
        className="max-w-sm"
        // Suppress Radix's default "focus first tabbable on open" behaviour.
        // Without this, the jyrkki.eu website button (the first focusable
        // element below) gets a focus ring on open and looks pre-selected.
        // Focus stays on the DialogContent container itself,
        // so Escape / click-outside still close as expected.
        onOpenAutoFocus={(e) => e.preventDefault()}
      >
        <div className="flex flex-col items-center text-center gap-3">
          {/* App icon — served from `public/about-icon.png` (sourced from
             `src-tauri/icons/128x128@2x.png`). The wrapping div reserves
             the 64×64 box up front so we never reflow when the image
             finishes loading; that reflow was the cause of the brief
             "starts larger, snaps smaller" flash on dialog open. */}
          <div className="h-16 w-16 shrink-0">
            <img
              src="/about-icon.png"
              alt=""
              aria-hidden="true"
              width={64}
              height={64}
              className="h-16 w-16 rounded-md"
            />
          </div>

          <DialogTitle className="text-xl">Klaar</DialogTitle>

          <DialogDescription className="text-xs">
            {version ?? "—"}
          </DialogDescription>

          <div className="text-sm text-[var(--color-text-primary)]">
            Jürgen Šuvalov
          </div>

          <button
            type="button"
            className="text-sm text-[var(--color-accent)] hover:text-[var(--color-accent-hover)] underline-offset-2 hover:underline cursor-pointer focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)] rounded-sm"
            onClick={() => safeOpenUrl(URL_WEBSITE)}
          >
            jyrkki.eu
          </button>

          <div className="flex items-center justify-center gap-3 pt-1">
            <IconLinkButton
              label="Email — hey@jyrkki.eu"
              onClick={() => safeOpenUrl(URL_EMAIL)}
            >
              <EmailIcon className="h-6 w-6" />
            </IconLinkButton>
            <IconLinkButton
              label="Bluesky — @jyrkki.eu"
              onClick={() => safeOpenUrl(URL_BLUESKY)}
            >
              <BlueskyIcon className="h-6 w-6" />
            </IconLinkButton>
            <IconLinkButton
              label="Mastodon — @jyrkki@est.social"
              onClick={() => safeOpenUrl(URL_MASTODON)}
            >
              <MastodonIcon className="h-6 w-6" />
            </IconLinkButton>
          </div>

          {/* TODO: open-source — repo URL + license */}
          <div data-testid="about-footer-slot" />

          <DialogClose asChild>
            <button
              type="button"
              className="mt-2 rounded-md border border-[var(--color-border)] bg-transparent px-4 py-1.5 text-sm font-medium text-[var(--color-text-primary)] cursor-pointer hover:bg-[var(--color-surface-elevated)] focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)]"
            >
              Close
            </button>
          </DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  );
}

interface IconLinkButtonProps {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}

function IconLinkButton({ label, onClick, children }: IconLinkButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="inline-flex h-8 w-8 items-center justify-center rounded-md text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)] cursor-pointer transition-colors"
    >
      <span aria-hidden="true" className="inline-flex">
        {children}
      </span>
    </button>
  );
}
