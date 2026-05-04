# Releasing Klaar

Releases are produced by two GitHub Actions workflows in
`.github/workflows/`:

- `release-prepare.yml` — bumps version sites, commits, tags, and
  dispatches the build (runs on `ubuntu-latest`).
- `release.yml` — runs gates, builds the universal DMG via
  `pnpm build:release:universal`, and creates a **draft** GitHub Release
  with the DMG attached (runs on `macos-14`).

Neither workflow code-signs or notarizes. Klaar ships ad-hoc signed.

## Standard release

1. Open **Actions** → **release-prepare** on GitHub.
2. Click **Run workflow**, enter the new version (e.g. `0.2.0`, no
   leading `v`), and run.
3. Wait for `release-prepare` to complete (≈1 min). It pushes a commit
   `release: v<version>` and tag `v<version>` to `main`, then
   dispatches `release.yml` against the tag.
4. Wait for `release.yml` to complete (≈10–15 min). It produces
   `Klaar_<version>_universal.dmg` and creates a draft GitHub Release.
5. Open **Releases**, find the draft `v<version>`, review the
   auto-generated notes and the attached DMG, edit if needed, then click
   **Publish release**.

## Manual override: tag was bumped by hand

If you bumped the four version sites and pushed the tag yourself, skip
`release-prepare` entirely:

1. Open **Actions** → **release** → **Run workflow**.
2. Select the existing tag (e.g. `v0.2.0`) as the ref.
3. Run. The workflow verifies that `package.json.version` matches the
   tag, runs gates, builds, and creates the draft release.

## Failure recovery: build failed after the tag was pushed

The bump commit and tag stay on `main` even if `release.yml` fails. To
recover:

1. Diagnose the failure from the Actions logs.
2. Push a fix to `main`. If the fix needs to be on the tagged ref
   itself, force-move the tag (`git tag -f v<version> <new-sha> && git
   push -f origin v<version>`); otherwise leave the tag alone — gates
   and the build will run against the tagged commit.
3. Open **Actions** → **release** → **Run workflow** and dispatch it
   against the same tag.
4. The DMG is uploaded with `--clobber`, replacing any partial asset.
   The existing draft (and any hand-edited notes) is preserved.

## Version sites kept in sync

Every release moves the three app-version sites together, plus the
src-tauri lockfile:

- `package.json` (`.version`)
- `src-tauri/tauri.conf.json` (`.version`)
- `src-tauri/Cargo.toml` (`[package].version`)
- `src-tauri/Cargo.lock` (refreshed via `cargo update --workspace` in
  `src-tauri/`)

## What is NOT touched by the release workflow

The driver crate is independently versioned. The release workflow does
**not** edit:

- `driver/Cargo.toml` (`[package].version`)
- `driver/Info.plist` (`CFBundleVersion` / `CFBundleShortVersionString`)
- `src-tauri/src/constants.rs` (`MIN_DRIVER_VERSION`)

These three are the CoreAudio bundle's ABI version and the app's
minimum-driver-version policy. They're bumped **by hand** only when the
driver ABI actually changes (device UID, ring-buffer layout, property
table — see the `MIN_DRIVER_VERSION` doc-comment for the full bump
policy). Bumping them on every app release would force every existing
user through an admin-prompted reinstall.

### Manual driver-ABI-bump procedure

When you change driver ABI:

1. Edit `driver/Info.plist` — update both `CFBundleVersion` and
   `CFBundleShortVersionString` to the new ABI version (e.g. `1.1.0`).
2. Edit `src-tauri/src/constants.rs` — update `MIN_DRIVER_VERSION` to
   the same value.
3. Edit `driver/Cargo.toml` — update `[package].version` to the same
   value (kept in sync by convention, since nothing currently templates
   one from the other).
4. Commit the three edits as one change before the next release.

Existing users will be prompted to reinstall the driver on next launch.
