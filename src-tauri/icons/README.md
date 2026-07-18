# Tray icons

These assets are **committed** — the Tauri build requires them (`generate_context!` and the Windows
resource step both read from here). They render the WorkPulse **Graphite & Indigo** pulse mark:

- `icon.png` (256×256) — default app icon
- `32x32.png`, `128x128.png`, `128x128@2x.png` — bundle sizes
- `icon.ico` (16/32/48/64 + 256) — Windows
- `icon.icns` (128 + 256) — macOS

## Regenerating

If the brand mark changes, regenerate from a single 1024×1024 source PNG:

```sh
cargo tauri icon path/to/workpulse-logo.png
```

That overwrites every file here to match the `bundle.icon` / `trayIcon.iconPath` paths in
`tauri.conf.json`. Do not borrow any other product's mark.
