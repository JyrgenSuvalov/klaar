/**
 * Tests for `useUpdateResultDialogStore`. Covers the show / dismiss / swap
 * semantics that the `<UpdateResultDialog>` consumes.
 */

import { beforeEach, describe, expect, it } from "vitest";

import {
  useUpdateResultDialogStore,
  type UpdateManualResultPayload,
} from "./updateResultDialogStore";

const upToDate: UpdateManualResultPayload = {
  kind: "up_to_date",
  checked_at: "2026-05-05T00:00:00Z",
};

const available: UpdateManualResultPayload = {
  kind: "available",
  tag: "v1.4.0",
  url: "https://example/v1.4.0",
  checked_at: "2026-05-05T00:00:00Z",
};

const networkError: UpdateManualResultPayload = {
  kind: "error",
  error: "network",
  checked_at: "2026-05-05T00:00:00Z",
};

beforeEach(() => {
  useUpdateResultDialogStore.setState({ payload: null });
});

describe("useUpdateResultDialogStore", () => {
  it("starts with a null payload", () => {
    expect(useUpdateResultDialogStore.getState().payload).toBeNull();
  });

  it("show() sets the payload", () => {
    useUpdateResultDialogStore.getState().show(upToDate);
    expect(useUpdateResultDialogStore.getState().payload).toEqual(upToDate);
  });

  it("dismiss() clears the payload", () => {
    useUpdateResultDialogStore.getState().show(upToDate);
    useUpdateResultDialogStore.getState().dismiss();
    expect(useUpdateResultDialogStore.getState().payload).toBeNull();
  });

  it("show() while non-null swaps the payload (coalescing rule)", () => {
    useUpdateResultDialogStore.getState().show(upToDate);
    useUpdateResultDialogStore.getState().show(available);
    expect(useUpdateResultDialogStore.getState().payload).toEqual(available);
    // Subsequent swap to a new error payload also replaces in place.
    useUpdateResultDialogStore.getState().show(networkError);
    expect(useUpdateResultDialogStore.getState().payload).toEqual(networkError);
  });

  it("show() with a new object reference triggers a state change", () => {
    // The dialog's auto-dismiss effect keys on payload identity. Two
    // identical-by-value calls SHOULD still produce a fresh reference (the
    // store doesn't dedupe), so the effect re-fires and the timer resets.
    useUpdateResultDialogStore.getState().show({ ...upToDate });
    const first = useUpdateResultDialogStore.getState().payload;
    useUpdateResultDialogStore.getState().show({ ...upToDate });
    const second = useUpdateResultDialogStore.getState().payload;
    expect(first).not.toBe(second);
  });
});
