# React + TypeScript + Vite

This template provides a minimal setup to get React working in Vite with HMR and some Oxlint rules.

Currently, two official plugins are available:

- [@vitejs/plugin-react](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react) uses [Oxc](https://oxc.rs)
- [@vitejs/plugin-react-swc](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react-swc) uses [SWC](https://swc.rs/)

## React Compiler

The React Compiler is not enabled on this template because of its impact on dev & build performances. To add it, see [this documentation](https://react.dev/learn/react-compiler/installation).

## Expanding the Oxlint configuration

If you are developing a production application, we recommend enabling type-aware lint rules by installing `oxlint-tsgolint` and editing `.oxlintrc.json`:

```json
{
  "$schema": "./node_modules/oxlint/configuration_schema.json",
  "plugins": ["react", "typescript", "oxc"],
  "options": {
    "typeAware": true
  },
  "rules": {
    "react/rules-of-hooks": "error",
    "react/only-export-components": ["warn", { "allowConstantExport": true }]
  }
}
```

See the [Oxlint rules documentation](https://oxc.rs/docs/guide/usage/linter/rules) for the full list of rules and categories.

## Sortlytic release workflow

This directory contains the Sortlytic macOS application. The repository-level [English README](../../README.md) contains the user-facing product guide; this section records the macOS packaging and update behavior defined by [`release-macos.yml`](../../.github/workflows/release-macos.yml).

### Release ownership and commit titles

For a normal push to `main`, `semantic-release` owns the release decision and metadata. It reads English Conventional Commit titles, selects the next version (`fix` and `revert` for patch, `feat` for minor, and `BREAKING CHANGE` for major), synchronizes the application version files, creates the `app-vX.Y.Z` tag, generates Release notes, and creates the draft GitHub Release named `Sortlytic vX.Y.Z`.

Release-relevant commit titles must be written in English and use Conventional Commits, for example `feat: add a collection target`, `fix: preserve the request limit`, or `revert: ...`. Use a `BREAKING CHANGE: ...` footer when a major release is required. Do not manually choose the release number, tag, or Release notes.

### macOS build and publication order

The normal workflow follows this order:

1. The reusable CI workflow verifies the pushed commit.
2. `semantic-release` creates a new tagged draft Release when the commit history requires one. If no release is needed, packaging is skipped.
3. The `build-and-release` matrix checks out the new tag and builds both `aarch64-apple-darwin` (Apple Silicon) and `x86_64-apple-darwin` (Intel) targets with `--bundles app,dmg`. Each target uploads its `.app`, `.dmg`, and Tauri updater artifacts to the same draft Release.
4. `finalize-release` rewrites `latest.json` to direct GitHub Release download URLs, requires a non-empty updater signature for every platform entry, uploads the normalized manifest, and makes the Release public only after the matrix succeeds.

The Tauri updater private key is supplied to the release action through `TAURI_SIGNING_PRIVATE_KEY`; `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is supplied when the key is password-protected. The updater public key and `latest.json` endpoint are configured in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json). Keep both private-key values out of source control.

### Recovery rebuilds with `rebuild_tag`

Use **Run workflow** and set `rebuild_tag` only to an existing `app-vX.Y.Z` tag. The workflow verifies the tag and its GitHub Release, checks out that tag for CI and packaging, skips `semantic-release`, rebuilds both architectures, uploads the artifacts to the existing Release, and refreshes its updater manifest. It does not create a new version, tag, or Release, and it skips the normal draft-publication step.

### In-app updater steps and signing boundary

In a packaged Sortlytic app, open **Settings → About Sortlytic**, choose **Check for Updates**, and review the version and Release notes. Choose **Download and install** to verify and prepare the signed artifact. When it is ready, choose **Restart and update**; the updater never restarts the app automatically. Browser preview does not have updater permission.

The current workflow configures Tauri updater signing but does not configure an Apple Developer ID certificate or notarization credentials. Updater signatures authenticate update artifacts; they do not replace Apple code signing or notarization, so Gatekeeper may still warn about a browser-downloaded app. Use only the official Sortlytic Release and the DMG matching the Mac architecture. If macOS blocks the verified app, follow the targeted quarantine-removal guidance in the root README; do not disable Gatekeeper system-wide.

For a local macOS bundle, run:

```bash
pnpm build:mac
```

Local updater builds require `TAURI_SIGNING_PRIVATE_KEY` and, when applicable, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
