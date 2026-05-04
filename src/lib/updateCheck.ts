/**
 * GitHub release update check.
 *
 * Design D7:
 *   - 24-hour disk-backed cache
 *   - Single unauthenticated request per day (under GitHub's 60/hr/IP limit)
 *   - Silent failure — on network / parse / rate-limit errors, refresh the
 *     cache timestamp (so we don't retry tight) and return null
 *   - Banner is dismissible per-tag; dismissed tags are stored in the cache
 *
 * The fetch runs in the webview (not the Rust backend) so we don't pull
 * `reqwest` in solely for this. `https://api.github.com` is whitelisted in
 * the Tauri CSP's `connect-src`.
 */

import { invoke } from "@tauri-apps/api/core";
import { isNewerThan } from "./semver";

/** Owner/repo slug for the GitHub release feed. Derived from the git remote. */
export const RELEASE_REPO = "JyrgenSuvalov/klaar";

/**
 * Compile-time kill-switch for the GitHub release update check.
 *
 * SAFETY: this MUST remain `false` while `JyrgenSuvalov/klaar` is a
 * private repository. GitHub returns 404 (not 401) for unauthenticated calls
 * to `/repos/.../releases/latest` on private repos, which causes the silent-
 * failure branch below to permanently suppress the update banner.
 *
 * When the repo is made public, flip this to `true` AND restore the
 * `https://api.github.com` entry in `connect-src` in `src-tauri/tauri.conf.json`
 * in the same commit. The two settings are coupled.
 */
// Typed as `boolean` (not the `false` literal) so TypeScript does not narrow
// downstream code as unreachable. Vite still const-folds the value at build.
export const UPDATE_CHECK_ENABLED: boolean = false;

const TWENTY_FOUR_HOURS_MS = 24 * 60 * 60 * 1000;

interface UpdateCache {
  lastCheckedAt: number;
  latestTag: string | null;
  dismissedTags: string[];
}

interface GitHubRelease {
  tag_name?: string;
  html_url?: string;
  prerelease?: boolean;
  draft?: boolean;
}

async function readCache(): Promise<UpdateCache> {
  try {
    const cached = await invoke<UpdateCache | null>("read_update_cache");
    return (
      cached ?? { lastCheckedAt: 0, latestTag: null, dismissedTags: [] }
    );
  } catch {
    return { lastCheckedAt: 0, latestTag: null, dismissedTags: [] };
  }
}

async function writeCache(cache: UpdateCache): Promise<void> {
  try {
    await invoke("write_update_cache", { cache });
  } catch (e) {
    console.warn("[updateCheck] write_update_cache failed:", e);
  }
}

export interface UpdateAvailable {
  tag: string;
  url: string;
}

/**
 * Check for a newer Klaar release on GitHub. Returns `null` when up to
 * date, the tag has already been dismissed, or any step failed (silent).
 */
export async function checkForAppUpdate(
  currentVersion: string,
): Promise<UpdateAvailable | null> {
  // Kill-switch: while disabled, perform no IPC, no fetch, no log output.
  // See `UPDATE_CHECK_ENABLED` above.
  if (!UPDATE_CHECK_ENABLED) return null;

  const cache = await readCache();

  const cacheFresh = Date.now() - cache.lastCheckedAt < TWENTY_FOUR_HOURS_MS;

  let latestTag = cache.latestTag;
  let htmlUrl: string | null = null;

  if (!cacheFresh) {
    try {
      const resp = await fetch(
        `https://api.github.com/repos/${RELEASE_REPO}/releases/latest`,
        {
          headers: { Accept: "application/vnd.github+json" },
        },
      );
      if (!resp.ok) {
        // Non-2xx: refresh timestamp so we don't hammer, keep old tag.
        await writeCache({ ...cache, lastCheckedAt: Date.now() });
        return bannerFrom(cache, currentVersion, null);
      }
      const json = (await resp.json()) as GitHubRelease;
      if (json.draft || json.prerelease || !json.tag_name) {
        await writeCache({ ...cache, lastCheckedAt: Date.now() });
        return bannerFrom(cache, currentVersion, null);
      }
      latestTag = json.tag_name;
      htmlUrl =
        json.html_url ??
        `https://github.com/${RELEASE_REPO}/releases/tag/${encodeURIComponent(latestTag)}`;
      await writeCache({
        ...cache,
        lastCheckedAt: Date.now(),
        latestTag,
      });
    } catch (e) {
      console.warn("[updateCheck] fetch failed:", e);
      await writeCache({ ...cache, lastCheckedAt: Date.now() });
      return bannerFrom(cache, currentVersion, null);
    }
  }

  return bannerFrom(
    { ...cache, latestTag: latestTag ?? cache.latestTag },
    currentVersion,
    htmlUrl,
  );
}

function bannerFrom(
  cache: UpdateCache,
  currentVersion: string,
  explicitUrl: string | null,
): UpdateAvailable | null {
  if (!cache.latestTag) return null;
  if (cache.dismissedTags.includes(cache.latestTag)) return null;
  if (!isNewerThan(cache.latestTag, currentVersion)) return null;

  const url =
    explicitUrl ??
    `https://github.com/${RELEASE_REPO}/releases/tag/${encodeURIComponent(cache.latestTag)}`;
  return { tag: cache.latestTag, url };
}

/** Persist the current latest-tag as dismissed so the banner hides until the next release. */
export async function dismissUpdateTag(tag: string): Promise<void> {
  const cache = await readCache();
  if (cache.dismissedTags.includes(tag)) return;
  await writeCache({
    ...cache,
    dismissedTags: [...cache.dismissedTags, tag],
  });
}
