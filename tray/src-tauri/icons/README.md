# Tray icons

Tauri needs real icon assets to build; they are binary and not checked in as part of the scaffold.
Generate them once from a single source PNG (1024×1024 recommended):

```sh
cargo tauri icon path/to/workpulse-logo.png
```

This produces `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns` (macOS), and `icon.ico`
(Windows) here, matching the `bundle.icon` / `trayIcon.iconPath` paths in `tauri.conf.json`.

Use the WorkPulse **Graphite & Indigo** identity — do not borrow any other product's mark.
