import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useEngineStore } from "@/store/engineStore";

/**
 * Auto-recovery hook for the persistent-denied mic-permission state.
 *
 * When `engineError.type === "microphone_permission_denied_persistent"`,
 * re-check the TCC status on every window `focus` event (cheap, no prompt).
 * If the user has meanwhile granted access in System Settings and returned
 * to the app, clear the banner and re-invoke engine start with the stored
 * input-device UID — same semantics as the Reconnect button.
 *
 * Idempotent and only active while the persistent banner is visible, so
 * there's no per-focus overhead in the happy path.
 *
 * Implements the `macos-permissions` spec's "Authorization is re-checked on
 * window focus" requirement.
 */
export function useMicPermissionRecheck() {
  const engineError = useEngineStore((s) => s.engineError);
  const selectedInputUid = useEngineStore((s) => s.selectedInputUid);
  const selectInputDevice = useEngineStore((s) => s.selectInputDevice);
  const clearError = useEngineStore((s) => s.clearError);

  useEffect(() => {
    if (engineError?.type !== "microphone_permission_denied_persistent") {
      // Nothing to watch for.
      return;
    }

    const runRecheck = async () => {
      try {
        // Read-only probe — never prompts. `request_access` is reserved for
        // `try_start_engine`.
        const status = await invoke<string>("get_microphone_authorization_status");
        if (status !== "authorized") {
          // Still denied — banner stays visible, no-op.
          return;
        }
        // User granted access while we were backgrounded. Clear banner and
        // retry engine start via the existing device-selection flow.
        clearError();
        if (selectedInputUid) {
          await selectInputDevice(selectedInputUid);
        }
      } catch (e) {
        console.error("[useMicPermissionRecheck] status probe failed:", e);
      }
    };

    // Sync wrapper so `addEventListener` gets a `() => void` signature; the
    // async work is fire-and-forget via `void`.
    const onFocus = () => {
      void runRecheck();
    };

    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [engineError, selectedInputUid, selectInputDevice, clearError]);
}
