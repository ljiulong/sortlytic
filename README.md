<!-- BEAUTIFIED -->

<div align="right">

English · <a href="README-zh.md">中文</a>

</div>

<p align="center">
  <img src="apps/macos/src-tauri/icons/icon.png" width="128" alt="Sortlytic logo" />
</p>

<h1 align="center">Sortlytic</h1>

<p align="center">
  <strong>A local-first macOS workspace for collecting, organizing, validating, and exporting public social-platform research.</strong>
  <br />
  <em>TikTok · Douyin · Xiaohongshu · Structured workflows · XLSX and PDF exports</em>
</p>

<p align="center">
  <a href="#quick-start"><img src="https://img.shields.io/badge/Quick_Start-111827?style=for-the-badge" alt="Quick Start" /></a>
  <a href="https://github.com/ljiulong/sortlytic/releases/latest"><img src="https://img.shields.io/badge/Latest_Release-0891B2?style=for-the-badge" alt="Latest Release" /></a>
</p>

<p align="center">
  <a href="https://github.com/ljiulong/sortlytic/actions/workflows/ci.yml"><img src="https://github.com/ljiulong/sortlytic/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/ljiulong/sortlytic/releases"><img src="https://img.shields.io/github/v/release/ljiulong/sortlytic?display_name=tag&amp;style=flat" alt="Release" /></a>
  <img src="https://img.shields.io/badge/macOS-Desktop-000000?style=flat&amp;logo=apple&amp;logoColor=white" alt="macOS" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/TypeScript-007ACC?style=flat&amp;logo=typescript&amp;logoColor=white" alt="TypeScript" />
  <img src="https://img.shields.io/badge/React-20232A?style=flat&amp;logo=react&amp;logoColor=61DAFB" alt="React" />
  <img src="https://img.shields.io/badge/Tauri-FFC131?style=flat&amp;logo=tauri&amp;logoColor=black" alt="Tauri" />
  <img src="https://img.shields.io/badge/Rust-000000?style=flat&amp;logo=rust&amp;logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/SQLite-003B57?style=flat&amp;logo=sqlite&amp;logoColor=white" alt="SQLite" />
</p>

## Features

| Capability | What it provides |
|---|---|
| Multi-platform collection | Maps keyword search, comments, account profiles, and item details to supported TikHub endpoints for TikTok, Douyin, and Xiaohongshu. |
| Controlled task execution | Requires plan confirmation and enforces request, record, and budget limits before the local worker executes a task. |
| Natural-language planning | Converts Chinese research intent into a validated collection plan through the current local rule parser and records its runtime snapshot. |
| Prompt governance | Stores prompt templates and versions, binds output schemas, and blocks activation when built-in regression cases fail. |
| Local-first security | Keeps workspace data in local SQLite storage and stores API credentials in macOS Keychain through scoped secret references. |
| Auditable delivery | Builds report snapshots, validates export integrity, and writes structured Excel workbooks and PDF reports with hashes and job history. |

## Quick Start

### Download the macOS app

1. Open the [latest GitHub Release](https://github.com/ljiulong/sortlytic/releases/latest).
2. Check your Mac architecture from **Apple menu → About This Mac**, or run `uname -m` in Terminal.
3. Download the DMG whose name ends in `_aarch64.dmg` for Apple Silicon (`arm64`), or `_x64.dmg` for an Intel Mac (`x86_64`). The `.app.tar.gz` and `.sig` files are updater artifacts, not the normal installer.
4. Open the DMG, drag Sortlytic into **Applications**, eject the disk image, and launch Sortlytic from the Applications folder.

The current release workflow does not yet apply Apple Developer ID signing and notarization. Read [First launch and the “damaged” alert](#first-launch-and-the-damaged-alert) before overriding any macOS security warning.

### Run from source

Source development requires macOS, Node.js 24, pnpm 11.5.2 through Corepack, and Rust 1.77.2 or newer.

```bash
git clone https://github.com/ljiulong/sortlytic.git
cd sortlytic/apps/macos
corepack enable
corepack install
pnpm install --frozen-lockfile
pnpm tauri dev
```

For interface preview without the native backend, run:

```bash
pnpm dev
```

The browser preview uses demonstration data. It cannot use macOS Keychain, execute native collection tasks, create local exports, or install application updates.

## Usage

### First launch and the “damaged” alert

The current `v0.1.5` release contains Tauri updater signatures, but the GitHub Actions workflow does not yet contain the Apple Developer ID certificate and notarization credentials required by macOS Gatekeeper. Tauri documents that browser-downloaded macOS apps need code signing to avoid the “application is damaged and can’t be opened” warning. Updater signatures verify update artifacts inside Sortlytic; they do not replace Apple code signing or notarization.

If macOS shows the alert in the screenshot:

1. Delete the rejected copy and download the correct DMG again from the [official Sortlytic Releases page](https://github.com/ljiulong/sortlytic/releases). Do not use a mirror or a file forwarded through chat.
2. Confirm that `_aarch64.dmg` matches Apple Silicon or `_x64.dmg` matches an Intel Mac.
3. Try to open Sortlytic once, then open **System Settings → Privacy & Security**. If **Open Anyway** is available, use it only after confirming the download source. Apple notes that this exception is normally offered for about one hour after an attempted launch.
4. If macOS still reports that the app is damaged or does not offer **Open Anyway**, and you have verified that the app came from the official Release, remove the quarantine attribute from Sortlytic only and launch it again:

   ```bash
   xattr -dr com.apple.quarantine "/Applications/Sortlytic.app"
   open "/Applications/Sortlytic.app"
   ```

   These commands do not disable Gatekeeper globally. They remove the download quarantine attribute only from this app bundle. `sudo` is normally unnecessary; if Terminal reports `Permission denied`, run only the `xattr` command again with `sudo`.
5. If the app still cannot open after the targeted removal, delete it and recheck the architecture and download integrity. Run from source or wait for a Developer ID-signed and notarized release instead of using `sudo spctl --master-disable` or another system-wide bypass.

See [Apple’s Gatekeeper guidance](https://support.apple.com/102445) and [Tauri’s macOS signing guide](https://v2.tauri.app/distribute/sign/macos/) for the security model and release requirements.

### Interface map

| Area | Use it for |
|---|---|
| Workbench | Create plans, confirm collection, follow task status, review data and evidence, and export deliverables. |
| Settings | Inspect the local workspace, configure TikHub and model providers, retest connections, and install updates. |
| Guide button | Open the book icon in the top-right corner for TikHub registration, token, domain, cost, and safety guidance. |
| Theme button | Switch between light and dark themes; the preference is retained locally. |

### Configure TikHub

TikHub is required for real collection. Create and verify an account before building a task:

1. [Register a TikHub account](https://user.tikhub.io/register), verify the email address, then [sign in to the user center](https://user.tikhub.io/login).
2. Create an API Token in the user center and copy it when shown. Check the [TikHub pricing page](https://tikhub.io/pricing) before using paid endpoints.
3. In Sortlytic, open **Settings → TikHub Settings → Configure TikHub API**.
4. Select a domain, paste the token, and choose **Save and Test**.

| Network | API domain |
|---|---|
| International network | `https://api.tikhub.io` |
| Mainland China network | `https://api.tikhub.dev` |

A successful test displays the masked account email, available free credit, and email verification status. The token is written to macOS Keychain; the SQLite workspace stores only a scoped secret reference. When editing an existing configuration, leave the token field empty to reuse the saved token.

Start the first collection with 10–50 records. This makes it easier to verify the platform, data type, region, keyword, and endpoint cost before expanding the task.

### Configure a model provider (optional)

Open **Settings → Model API Settings** to store an OpenAI, Anthropic, Gemini, or custom OpenAI-compatible profile. Select the API format, fill in the Base URL when required, enter the default model ID and API key, then choose **Save and Validate**. The available API formats also include Ollama for saved local-provider configurations. Saved keys use macOS Keychain and can be reused without re-entering them.

This configuration is optional in the current MVP. Natural-language planning still uses `local-rule-engine/rule-parser-v1`; provider-backed plan generation and real model inference are not connected yet.

### Create and confirm a collection plan

Open **Workbench → Collection Builder** and choose an entry method:

| Method | When to use it | Required review |
|---|---|---|
| Form | You already know the platform, data type, region, keyword, time range, record limit, and budget. | Confirm every field before generating the plan. |
| Natural language | You want to describe the research goal in Chinese and let the local parser structure it. | Check inferred platforms, data types, missing conditions, record limits, and budget. |

The form supports TikTok, Douyin, and Xiaohongshu with keyword search, public account information, comment collection, and item details. A single task accepts 10–5,000 records and a budget value from 1–500.

After selecting **Generate Plan** or **Parse into Plan**:

1. Review the plan preview, especially platform, data type, region, time range, maximum records, request estimate, and amount limit.
2. Resolve any value shown under **Missing Conditions**. A plan cannot be confirmed until backend validation reports it as valid.
3. Select **Confirm Run**. Planning itself does not start paid collection; confirmation adds the task to the local queue.
4. Follow the task in **Task Queue**. Possible states include queued, running, waiting for confirmation, partially successful, successful, and failed.

### Review results and evidence

Select a row under **Data Assets** to inspect its source link, evidence summary, validation state, confidence, model run, and transformation reason in the right-hand panel. Records marked **Manual Confirmation** or **Insufficient Evidence** should be checked before they are used in a report.

Current MVP boundary: native tasks and raw-record storage are implemented, but the workbench’s real-record query is not connected yet. In a packaged backend session, **Data Assets** may therefore remain empty even after a task runs. Browser preview rows are demonstration data and are not collection results.

### Export Excel and PDF

1. Create at least one collection task.
2. In the right-hand **Export Center**, select **Run Export Check**.
3. Sortlytic builds a report snapshot, validates the export request, and creates both XLSX and PDF jobs.
4. When both jobs show **Passed**, use the paths displayed under **Excel Workbook** and **PDF Report** to locate the files.

Files are written under the active workspace:

```text
default-workspace/
├── app.sqlite
├── raw/tikhub/
├── reports/
├── exports/excel/
└── exports/pdf/
```

Use XLSX for the structured report payload. The current PDF writer produces a short summary that points readers to the workbook for complete structured data. **Webhook Summary** is visible in the interface but is not enabled and does not send data.

### Update the app

Packaged releases can update from **Settings → Automatic Updates**:

1. Select **Check for Updates**.
2. Review the version number and release notes.
3. Select **Download and Restart**. Sortlytic verifies the Tauri updater artifact signature, installs the update, and relaunches.

Browser preview does not have update permission. Apple Developer ID signing and notarization are separate from updater signature verification and still need to be added to the release workflow.

### Troubleshooting

| Symptom | Check |
|---|---|
| “Sortlytic is damaged and can’t be opened” | Re-download the matching DMG from the official release, then follow [First launch and the “damaged” alert](#first-launch-and-the-damaged-alert). |
| TikHub test fails | Check that the token is complete, the email is verified, the domain matches the current network, and the account has enough credit. |
| **Save and Test** is disabled | A new TikHub token must contain at least 8 characters. An existing saved token can be reused by leaving the field empty. |
| A plan cannot be confirmed | Generate the plan first, clear **Missing Conditions**, and verify that its validation status is valid. |
| Export fails | Create a task first. Then read the message below the workspace title for the backend error and retry after the task data is available. |
| The screen shows realistic records but no native features work | The app is running through `pnpm dev` in browser demonstration mode. Start it with `pnpm tauri dev` or use the packaged app. |
| No **Open Anyway** button appears | Re-attempt the launch and check Privacy & Security within one hour. For the verified official build, use the targeted `xattr` command above; do not disable Gatekeeper globally. |

### Data and security boundaries

- Sortlytic currently creates one local `default-workspace`; it does not provide user accounts, a remote database, remote synchronization, or multi-device synchronization.
- Workspace data, raw responses, prompt snapshots, logs, reports, and exports remain under the macOS application data directory.
- TikHub and model API secrets remain in macOS Keychain. They are not written into reports or exports.
- Deleting the application does not necessarily remove the workspace or Keychain entries. Back up required XLSX, PDF, and raw files before manually deleting application data.
- Only collect public data that you are permitted to access, and follow platform terms, privacy requirements, and applicable law.

## Architecture

```mermaid
%%{init: {'theme': 'base', 'themeVariables': {'fontSize': '14px', 'lineColor': '#64748B'}}}%%
graph LR
    classDef client fill:#3B82F6,stroke:#2563EB,color:#fff,stroke-width:2px
    classDef service fill:#10B981,stroke:#059669,color:#fff,stroke-width:2px
    classDef data fill:#8B5CF6,stroke:#7C3AED,color:#fff,stroke-width:2px
    classDef auth fill:#F97316,stroke:#EA580C,color:#fff,stroke-width:2px
    classDef external fill:#F43F5E,stroke:#E11D48,color:#fff,stroke-width:2px

    A[React Workbench<br/>TypeScript] --> B[Tauri Command Layer<br/>Rust]
    B --> C[Collection Task Worker]
    B --> D[Prompt and Plan Runtime]
    B --> E[Report and Export Engine]
    B --> F[(Local Workspace<br/>SQLite and files)]
    B --> G[macOS Keychain]
    B --> H[TikHub and Model APIs]
    C --> F
    C --> H
    D --> F
    E --> F

    class A client
    class B,C,D,E service
    class F data
    class G auth
    class H external
```

## Configuration

### Application identity

| Setting | Value | Source |
|---|---|---|
| Product name | `Sortlytic` | `apps/macos/src-tauri/tauri.conf.json` |
| Application identifier | `com.steven.sortlytic` | `apps/macos/src-tauri/tauri.conf.json` |
| Default workspace | `default-workspace` | Created under the macOS app data directory |
| Local persistence | SQLite, raw records, reports, and exports | Stored inside the active workspace |
| Updater endpoint | `https://github.com/ljiulong/sortlytic/releases/latest/download/latest.json` | Tauri updater configuration |

### In-app settings

| Setting | Purpose | Storage |
|---|---|---|
| TikHub API domain | Selects `api.tikhub.io` or `api.tikhub.dev` for the current network | Workspace database |
| TikHub token | Authenticates collection and account checks | macOS Keychain |
| Model provider | Stores provider format, endpoint, region, policies, and health status | Workspace database |
| Model API key | Authenticates provider connection tests | macOS Keychain |
| Default model profile | Records model capabilities and the active model choice | Workspace database |

### Release secrets

| GitHub Actions secret | Purpose |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Signs updater artifacts produced by the release workflow. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Unlocks the updater signing key when the key is password-protected. |

Do not commit signing keys, API tokens, or exported credentials to the repository.

## Project Structure

```text
.
├── .github/workflows/          # CI and macOS release automation
│   ├── ci.yml                  # Frontend, Rust, and dependency checks
│   └── release-macos.yml       # Version bump, signing, packaging, and publishing
├── apps/macos/                 # Sortlytic desktop application
│   ├── src/                    # React workbench and settings interfaces
│   ├── src-tauri/              # Rust commands, storage, workers, and bundling
│   └── package.json            # pnpm scripts and frontend dependencies
├── excel/                      # Spreadsheet templates used by the project
├── plan/                       # Product, architecture, testing, and delivery notes
├── AGENTS.md                   # Repository collaboration rules
├── README.md                   # English documentation
└── README-zh.md                # Simplified Chinese documentation
```

## Tech Stack

### Interface

| Technology | Purpose |
|---|---|
| React 19 | Desktop workbench and settings UI |
| TypeScript 6 | Frontend types and Tauri command contracts |
| Vite 8 | Frontend development and production builds |
| TanStack Query and Table | Server-state coordination and tabular presentation |
| React Hook Form and Zod | Form state and input validation |
| Radix Tabs and Lucide | Accessible navigation primitives and interface icons |

### Desktop and data

| Technology | Purpose |
|---|---|
| Tauri 2 | Native macOS application shell and command bridge |
| Rust | Workspace, collection, task, prompt, security, and export logic |
| SQLite and rusqlite | Local transactional workspace storage |
| macOS Keychain | API credential storage through scoped key references |
| reqwest | TikHub and provider connection requests |
| rust_xlsxwriter | Native XLSX report generation |

### Quality and delivery

| Technology | Purpose |
|---|---|
| Vitest | Frontend unit tests |
| Oxlint | Frontend static analysis |
| Cargo fmt, test, and Clippy | Rust formatting, tests, and lint checks |
| GitHub Actions | CI, versioning, dual-architecture macOS builds, and releases |
| Tauri updater | Signed update metadata and downloadable application artifacts |

## Deployment

### Validate locally

```bash
cd apps/macos
pnpm lint
pnpm test
pnpm build
```

```bash
cd apps/macos/src-tauri
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
```

### Build macOS artifacts

```bash
cd apps/macos
pnpm build:mac
```

Local updater builds require `TAURI_SIGNING_PRIVATE_KEY` and, when applicable, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

### Publish a release

Run the [`release-macos`](.github/workflows/release-macos.yml) workflow manually and select a patch, minor, or major version bump. The workflow synchronizes `package.json`, `tauri.conf.json`, and `Cargo.toml`, creates an `app-vX.Y.Z` tag, then builds Apple Silicon and Intel `.app` and `.dmg` artifacts before publishing the GitHub Release.

## Contributing

1. Fork the repository.
2. Create a focused branch: `git checkout -b feature/short-description`.
3. Make the change and run the relevant frontend and Rust checks.
4. Commit only the files in scope.
5. Push the branch and open a Pull Request.

No LICENSE file is currently present. Add a LICENSE before distributing or accepting external contributions under defined terms.
