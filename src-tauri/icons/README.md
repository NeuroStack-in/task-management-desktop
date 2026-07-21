# Tray icons

These assets are **committed** — the Tauri build requires them (`generate_context!` and the Windows
resource step both read from here). They render the WorkPulse pulse mark on the **panel teal**
`--primary` (`#0f9b8e`, `ui/src/index.css`) — the same badge the in-panel `LogoMark` draws.

> Regenerated 2026-07-21. They previously used the web app's **Graphite & Indigo** `--primary`
> (`#4f46e5`), which left the taskbar/tray/title-bar icon as the one indigo element beside an
> otherwise teal panel. The *glyph* is unchanged and still shared with the web wordmark; only the
> badge fill moved to the desktop palette. Note the web app's own mark uses `--m-accent` (the
> marketing teal), not its indigo `--primary` — so this now matches there too.

- `icon.png` (256×256) — default app icon
- `32x32.png`, `128x128.png`, `128x128@2x.png` — bundle sizes
- `icon.ico` (16/32/48/64 + 256) — Windows
- `icon.icns` (128 + 256) — macOS

## Regenerating

If the brand mark changes, regenerate from a single 1024×1024 source PNG:

```sh
cargo tauri icon path/to/workpulse-logo.png
```

That overwrites every file here to match the `bundle.icon` paths in `tauri.conf.json`. Do not borrow
any other product's mark.

**It also emits assets this app does not use** — `64x64.png`, the `Square*Logo.png` / `StoreLogo.png`
MSIX set, and `android/` + `ios/` trees. WorkPulse's agent is desktop-only and `bundle.icon` lists
just the five files above (plus `icon.png` as the source of record), so delete the rest rather than
committing a mobile icon set for a product with no mobile build.
