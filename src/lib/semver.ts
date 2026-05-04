/**
 * Mirror of `src-tauri/src/util/semver.rs`. Strict `MAJOR.MINOR.PATCH` parsing.
 * Any other shape returns `null` and callers treat that as "unknown".
 *
 * Keep this tiny and dependency-free — we don't need a full semver parser for
 * the driver-version gate or the GitHub release check.
 */

export type Version = [number, number, number];

export function parseVersion(raw: string | null | undefined): Version | null {
  if (!raw) return null;
  // Allow a leading "v" (GitHub release tags) but nothing else.
  const s = raw.startsWith("v") ? raw.slice(1) : raw;
  const parts = s.split(".");
  if (parts.length !== 3) return null;
  const nums: number[] = [];
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return null;
    const n = Number(p);
    if (!Number.isFinite(n) || n < 0) return null;
    nums.push(n);
  }
  return [nums[0], nums[1], nums[2]];
}

/** Returns -1 / 0 / 1 like `String.prototype.localeCompare`, or `null` if either side fails to parse. */
export function compareVersions(a: string, b: string): -1 | 0 | 1 | null {
  const pa = parseVersion(a);
  const pb = parseVersion(b);
  if (!pa || !pb) return null;
  for (let i = 0; i < 3; i++) {
    if (pa[i] < pb[i]) return -1;
    if (pa[i] > pb[i]) return 1;
  }
  return 0;
}

/** True iff `installed < required`. Returns `false` when either fails to parse (fail-safe). */
export function isOlderThan(installed: string, required: string): boolean {
  return compareVersions(installed, required) === -1;
}

/** True iff `candidate > current`. Returns `false` when either fails to parse. */
export function isNewerThan(candidate: string, current: string): boolean {
  return compareVersions(candidate, current) === 1;
}
