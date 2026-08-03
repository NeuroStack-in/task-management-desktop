# BUILDING.md — how the agent is built and shipped, per platform

What a developer needs to know before touching a build or cutting a release. Signing *policy* lives
in [`UPDATES-SECURITY.md`](UPDATES-SECURITY.md); running the agent against a live backend lives in
[`RUNBOOK.md`](RUNBOOK.md). This file is the mechanics.

> **Verified 2026-07-29** against releases v0.1.1 → v0.1.4. Every number and path below was observed,
> not assumed.

---

## 1. What exists

One Tauri v2 app, two halves, three platforms:

| Half | Where | Built with |
|---|---|---|
| Webview UI | `ui/` (Preact + Vite + TS) | `npm run build` → `ui/dist` |
| Core | `src-tauri/` (Rust, one process) | `cargo tauri build` |

Tauri runs `beforeBuildCommand` itself, so **`cargo tauri build` builds the UI too** — you don't run
the npm build separately.

### Artifacts per platform

| Platform | User installer | **Updater** artifact | Approx size |
|---|---|---|---|
| Windows | `WorkPulse_<v>_x64-setup.exe` (NSIS) | the **same** `-setup.exe` + `.sig` | 3.3 MB |
| Linux | `WorkPulse_<v>_amd64.deb` | `WorkPulse_<v>_amd64.AppImage` + `.sig` | 5.5 MB / 79 MB |
| macOS | `WorkPulse_<v>_universal.dmg` | `WorkPulse.app.tar.gz` + `.sig` | 7.9 MB / 8.0 MB |

**The installer and the updater artifact are not always the same file.** On macOS especially: the
updater ships the **`.app.tar.gz`**, never the dmg. A dmg-only build produces no macOS updater
artifact at all, and Macs then silently never update while Windows and Linux do.

---

## 2. Building locally

### Prerequisites

| | Needs |
|---|---|
| All | Rust (stable), Node 22, `cargo install tauri-cli`, then `just ui-install` |
| Windows | Nothing extra — WebView2 ships with Windows 10/11 |
| Linux | `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf build-essential libgtk-3-dev libxdo-dev libssl-dev libx11-dev libxcb1-dev libxcb-randr0-dev libxrandr-dev libxss-dev libxtst-dev libxi-dev libxext-dev libdbus-1-dev` |
| macOS | Xcode command line tools |

```bash
just dev        # run with hot reload
just build      # produce this platform's bundle
just ci         # fmt + clippy (-D warnings) + test — what CI runs on the Rust core
```

**You can only build for the platform you are on.** There is no cross-compilation here; that is the
entire reason CI uses three runners.

### Building the macOS universal binary

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo tauri build --target universal-apple-darwin --bundles app,dmg
```

⚠️ **Space-separated, not comma-separated.** `rustup target add a,b` reads the whole string as one
bogus target name and fails. This exact typo silently killed every macOS release from v0.1.0 until it
was found — the job died in 17 seconds, Windows and Linux carried on, and the dmg simply never
existed.

---

## 3. The release pipeline

Tag-driven: push `v*` and [`release.yml`](../.github/workflows/release.yml) does everything.

```
git tag v0.1.5 && git push origin v0.1.5
        │
        ├─ bundle (windows-latest)  → nsis
        ├─ bundle (ubuntu-22.04)    → deb, appimage
        └─ bundle (macos-latest)    → app, dmg   [universal-apple-darwin]
                │
                ├─ draft GitHub Release (installers + .sig + latest.json)
                ├─ checksums job  → SHA256SUMS + SHA256SUMS.sig
                └─ s3-mirror job  → installers + a REWRITTEN latest.json
                        │
                  a human clicks Publish
```

Jobs are `fail-fast: false` — one platform breaking still ships the other two. Good for resilience,
but it means **a green-looking release can be missing a platform**; check all three jobs, not just the
run's overall status.

### Each build job also

- **Checks out the backend repo beside `desktop/`** — `wp-agent-contract` is a sibling path-dep, not
  a crates.io package. Needs `BACKEND_REPO_TOKEN`.
- **Bakes the backend identity into the binary** from GitHub repo *Variables* (`WP_COGNITO_CLIENT_ID`,
  `WP_COGNITO_REGION`, `WP_INGEST_URL`), read via `option_env!` in `src-tauri/src/auth/config.rs`.
  Unset variables expand to `""`, which falls back to the dev stack — see §6.

---

## 4. Versioning — three places, in lockstep

| File | Field |
|---|---|
| git tag | `v0.1.5` |
| `src-tauri/tauri.conf.json` | `"version": "0.1.5"` |
| `Cargo.toml` (repo root) | `[workspace.package] version = "0.1.5"` — `src-tauri` inherits it |

The `v` is on the tag only. **Forget the bump and nothing ships**: the updater compares versions, sees
no change, and every agent stays where it is — a release that appears to succeed and reaches nobody.

Editing these on Windows, write them with `[System.IO.File]::WriteAllText(path, text, utf8NoBom)`.
PowerShell's `Set-Content -Encoding utf8` prepends a **BOM**, and a BOM in `tauri.conf.json` breaks
the Tauri build script with an unhelpful error.

---

## 5. Signing — three separate systems

Do not confuse them. They protect different things and use different keys.

| Purpose | Key | Secret | Consequence if absent |
|---|---|---|---|
| **Updater artifacts** (minisign) | public half is baked into the binary + `tauri.conf.json` `plugins.updater.pubkey` | `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) | The agent refuses the update. Unsigned updates never install. |
| **SHA256SUMS** (Ed25519) | `SHA256SUMS.pub` | `RELEASE_SIGNING_KEY_B64` | Checksums ship unsigned; a warning, not a failure. |
| **OS code signing** | Apple Developer ID / Windows Authenticode | **none configured** | See §7. |

The first two are ours and work. The third is not set up on any platform.

---

## 6. Where releases are published — and why it isn't GitHub

**The desktop repo is private.** Private-repo release assets are not anonymously downloadable, so
neither the web Download page nor an installed agent can fetch them — an installed agent has no GitHub
credentials and never will.

So everything user-facing is mirrored to a public S3 bucket:

```
s3://wp-downloads-dev/agent/
├── v0.1.4/                        immutable, never overwritten
│   ├── WorkPulse_0.1.4_x64-setup.exe        (+ .sig)
│   ├── WorkPulse_0.1.4_amd64.AppImage       (+ .sig)
│   ├── WorkPulse_0.1.4_amd64.deb
│   ├── WorkPulse_0.1.4_universal.dmg
│   ├── WorkPulse.app.tar.gz                 (+ .sig)
│   └── latest.json
└── latest/                        overwritten every release
    ├── WorkPulse-x64-setup.exe    ← the Download page links to these
    ├── WorkPulse-amd64.deb
    ├── WorkPulse-amd64.AppImage
    ├── WorkPulse-universal.dmg
    └── latest.json                ← the agent polls this
```

### `latest.json` is rebuilt by CI, not reused

`tauri-action` writes a manifest pointing at **github.com** asset URLs — the same 404. The `s3-mirror`
job therefore constructs its own:

```json
{
  "version": "0.1.4",
  "pub_date": "2026-07-29T…Z",
  "platforms": {
    "windows-x86_64":   { "signature": "…", "url": "…/agent/v0.1.4/WorkPulse_0.1.4_x64-setup.exe" },
    "linux-x86_64":     { "signature": "…", "url": "…/agent/v0.1.4/WorkPulse_0.1.4_amd64.AppImage" },
    "darwin-universal": { "signature": "…", "url": "…/agent/v0.1.4/WorkPulse.app.tar.gz" }
  }
}
```

Three rules baked into that job:

1. **URLs point at `v<version>/`, never `latest/`.** `latest/` is overwritten by the next release, and
   the signature recorded here would then no longer match the bytes at that URL.
2. **Uploaded with `--cache-control no-cache`.** Agents poll this file; a cached manifest hides a
   shipped update for as long as the cache holds.
3. **A missing `.sig` omits that platform, with a warning.** Better a manifest that ignores a platform
   than one advertising an artifact nobody can verify.

Serving the manifest publicly changes only *where it is read*, never *whether an artifact is trusted* —
the minisign signature is checked against the key inside the binary.

---

## 7. Per-platform notes

### Windows

- NSIS, **per-user** install (`HKCU\…\Uninstall\WorkPulse`) — no admin rights needed.
- The updater consumes the same `-setup.exe`; there is no separate updater bundle.
- **`.agent-state/` lives beside the executable**, not in `%APPDATA%`: `agent_id`, `session.json`,
  `queue/batches.jsonl`. The uninstaller leaves it, which is deliberate — reinstalling to the same
  path keeps the device's identity instead of enrolling a duplicate. Wipe it only when you *want* a
  new device id.
- Unsigned (no Authenticode), so SmartScreen may warn on first run.

### Linux

- Two formats: `.deb` for Ubuntu/Debian, `.AppImage` for everything else. The AppImage is ~79 MB
  because it carries its own runtime.
- **The AppImage is the updater artifact.** A `.deb`-only build cannot self-update.
- Wayland limits some capture signals — see [`CAPTURE.md`](CAPTURE.md).

### macOS

- One job, one **universal** binary (Intel + Apple Silicon lipo'd together) on the arm64 runner. Do
  not add a `macos-13` Intel job; GitHub retired those runners.
- Bundle **`app,dmg`** — `app` is what yields the `.app.tar.gz` the updater needs (§1).
- **Ad-hoc signed** (`APPLE_SIGNING_IDENTITY: "-"` in `release.yml`), **not notarised.** The ad-hoc
  signature is not cosmetic: **Apple Silicon refuses to execute unsigned arm64 code**, so a build with
  no signature at all is killed on launch on every M-series Mac and reports *"the app is damaged"* —
  which reads as a corrupt download, not a signing gap. Ad-hoc signing is the minimum that makes the
  bundle run.
- Gatekeeper still blocks first launch with *"Apple could not verify this app"* (ad-hoc is not a
  Developer ID); the user must approve it under System Settings → Privacy & Security, or run
  `xattr -dr com.apple.quarantine /Applications/WorkPulse.app`.
- **The real cost isn't the warning — it's permissions, and ad-hoc signing does NOT fix it.** macOS
  binds Screen Recording and Accessibility grants to an app's *signing identity*. An ad-hoc signature
  carries no stable identity, so the grants still reset on every auto-update: capture stops silently
  after an update with nothing on screen explaining why. Tell users to expect a re-grant after each
  macOS update, and point them at `WorkPulse --test-capture`, which names the cause.
  Fixing this needs the **Apple Developer Program ($99/yr)** and a Developer ID certificate; there is
  no free tier for distribution certificates. Add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` and `tauri-action` notarises
  automatically.
- macOS support in the Rust code is **thin** — one `#[cfg(target_os = "macos")]` block in the tree.
  It compiles and runs; the TCC request/verify flow `CAPTURE.md` specifies is not written.

---

## 8. Traps that have already cost us a release

| Trap | Symptom | Rule |
|---|---|---|
| Comma in `rustup target add` | macOS job dies in ~17 s, other platforms ship | Space-separate targets |
| Floating `tauri-action@v0` | All three platforms fail on a commit that changed only a version string | **Pinned to a SHA.** Bump deliberately, read the changelog |
| `beforeBuildCommand` cwd | `npm error enoent … ui/ui/package.json` | Tauri runs it from the **frontend** dir; use plain `npm run build` |
| BOM in `tauri.conf.json` | Tauri build script fails to parse | Write UTF-8 **without** BOM |
| Version not bumped | Build succeeds, nobody receives it | Bump all three places (§4) |
| Actions spending limit | *"The job was not started because recent account payments have failed…"* — every job fails in seconds with no steps run | Private repos meter minutes: **Linux 1×, Windows 2×, macOS 10×**. One release ≈ 256 billable minutes ≈ $2 |

---

## 9. Verifying a release

Never trust the green tick alone:

1. **All three bundle jobs green** — not just the run (`fail-fast: false` hides a missing platform).
2. **`latest.json` lists three platforms**, and its `version` matches the tag:
   ```bash
   curl -s https://wp-downloads-dev.s3.ap-south-1.amazonaws.com/agent/latest/latest.json
   ```
3. **The installers are actually in S3**: `aws s3 ls s3://wp-downloads-dev/agent/latest/`
4. **Publish the draft release** — until then the tag exists and nothing is live.
5. **Prove the update path**, at least once per platform: install version *N*, publish *N+1*, restart
   the agent (the check runs at launch, then every 6 h), and confirm the version changes on the
   device page in the web app.

Step 5 is the only one that proves the thing users actually depend on. It has been done on **Windows**
(0.1.2 → 0.1.3 → 0.1.4). **Linux and macOS self-update remain unverified.**
