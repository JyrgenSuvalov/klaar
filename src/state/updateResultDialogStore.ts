/**
 * In-memory store backing the `<UpdateResultDialog>`.
 *
 * Driven by the `update_manual_result` Tauri event (Rust emits it whenever a
 * check launched with `manual: true` resolves to a terminal `UpdateStatus`).
 * The store has NO persistence layer — manual-check results are transient
 * session feedback, and persisting them across restarts makes no sense.
 *
 * Coalescing rule: calling `show()` while a non-null payload is already set
 * SWAPS the payload in place — the latest manual-check result wins. The
 * `<UpdateResultDialog>` keys its auto-dismiss `useEffect` on the payload
 * identity, so a swap correctly resets the timer.
 *
 * Mirrors the shape of `useAboutStore` so consumers reach for the same
 * idioms across the dialog vocabulary.
 */

import { create } from "zustand";

import type { UpdateStatus } from "@/lib/updateCheck";

/**
 * Terminal `UpdateStatus` variants that can appear as a payload. The
 * `Checking` and `Idle` flavours never reach the store — Rust only emits
 * `update_manual_result` on terminal transitions.
 */
export type UpdateManualResultPayload = Extract<
  UpdateStatus,
  { kind: "up_to_date" } | { kind: "available" } | { kind: "error" }
>;

interface UpdateResultDialogState {
  /**
   * Currently-shown payload, or `null` when no dialog is mounted.
   * `<UpdateResultDialog>` renders conditionally on this being non-null.
   */
  payload: UpdateManualResultPayload | null;
  /**
   * Show (or swap to) the given payload. Idempotent for identity equality
   * (Zustand's default shallow-equal `set`); calling with a *new* object
   * always replaces, which is the trigger the dialog's auto-dismiss
   * `useEffect` watches.
   */
  show: (payload: UpdateManualResultPayload) => void;
  /** Clear the payload — unmounts the dialog. */
  dismiss: () => void;
}

export const useUpdateResultDialogStore = create<UpdateResultDialogState>(
  (set) => ({
    payload: null,
    show: (payload) => set({ payload }),
    dismiss: () => set({ payload: null }),
  }),
);
