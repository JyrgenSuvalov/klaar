# GitHub Actions workflows

This directory contains the release automation for Klaar.

## Workflows

### `release-prepare.yml` — version bump + tag

- **Trigger:** manual (`workflow_dispatch`) only.
- **Runner:** `ubuntu-latest`.
- **Inputs:** `version` (required, semver like `0.2.0` or `0.2.0-rc.1`, no leading `v`).
- **What it does:**
  1. Validates the `version` input against the semver pattern.
  2. Checks out `main` and ensures `v<version>` does not already exist.
  3. Bumps the three app-version sites: `package.json`,
     `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`. The driver
     crate (`driver/Cargo.toml`, `driver/Info.plist`,
     `MIN_DRIVER_VERSION`) is intentionally not touched — see
     `RELEASING.md` for the manual driver-ABI-bump procedure.
  4. Refreshes `src-tauri/Cargo.lock`.
  5. Commits to `main` as `release: v<version>`.
  6. Creates and pushes annotated tag `v<version>`.
  7. Dispatches `release.yml` against the new tag (because tags pushed by
     the default `GITHUB_TOKEN` do not trigger downstream workflows).

### `release.yml` — build + draft release

- **Trigger:** `workflow_dispatch` and `push` of tags matching `v*`.
- **Runner:** `macos-14` (Apple Silicon).
- **What it does:**
  1. Verifies `package.json.version` matches the tag.
  2. Sets up Node 20 / pnpm and Rust stable with both Darwin targets.
  3. Runs gates: `pnpm typecheck`, `pnpm lint`, `pnpm test`,
     `cargo test --workspace` (per workspace).
  4. Runs `pnpm build:release:universal`.
  5. Creates the GitHub Release as a **draft** with auto-generated notes
     (or leaves an existing draft's notes untouched on re-runs).
  6. Uploads the universal DMG to the release (replacing on re-run via
     `--clobber`) and as a 14-day workflow artifact for debugging.

## Operator flow

1. Open the **Actions** tab on GitHub.
2. Select **release-prepare**, click **Run workflow**, enter the new
   version, and run.
3. Wait for `release-prepare` to finish; it will dispatch `release` for
   the new tag automatically.
4. Wait for `release` to finish (~10–15 min on `macos-14`).
5. Open the **Releases** tab, find the draft, review the auto-generated
   notes and the attached DMG, then click **Publish release**.

## Manual override paths

- **Re-run a failed build:** the bump commit and tag stay in place if
  `release.yml` fails. Fix the issue, push the fix to `main` (if the fix
  needs to be on the tagged ref, retag), then dispatch `release.yml`
  manually against the tag from the Actions tab. The asset is uploaded
  with `--clobber`, so re-runs replace it cleanly without touching the
  draft's release notes.
- **Bump versions by hand:** if the version was already bumped and tagged
  manually, skip `release-prepare` and just dispatch `release.yml`
  against the existing tag. The tag-vs-`package.json` consistency check
  will catch a mismatch.

## What this workflow does not do

- No code-signing or notarization. Klaar ships ad-hoc signed
  (`signingIdentity: "-"` in `tauri.conf.json`); first-launch users see
  Gatekeeper "unidentified developer" warnings.
- No PATs, no Apple secrets, no non-default tokens. Everything uses the
  default `GITHUB_TOKEN`.
- No automatic publishing. Drafts only.

See the spec at `openspec/specs/release-automation/spec.md` (synced from
the change once archived) for the formal contracts.
