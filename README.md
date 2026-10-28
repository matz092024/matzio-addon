# Matzio Addon

A self-hosted Stremio addon for Windows that finds streams for movies and shows
and plays them instantly through Real-Debrid or TorBox. It runs quietly in the
system tray on your own PC and is part of the Matzio family.

## Status

Step 1 of 4: the tray app runs, the addon can be installed in Stremio, and it
answers stream requests (with no streams yet). Debrid lookups come in step 2.

## How it works

- **Tray app** (`src-tauri/`): a [Tauri](https://tauri.app) app with a tray
  icon. Left-click it, or pick *Open settings*, to open the settings window.
  The window is destroyed when closed, so while idle only the tray icon and
  the addon server are running. Closing the window never quits; use
  *Quit Matzio* in the tray menu.
- **Addon server** (`crates/addon/`): serves the Stremio addon protocol on
  `http://127.0.0.1:7777/manifest.json`, reachable only from this PC.
- **Settings window** (`index.html`, `src/`): plain HTML and TypeScript built
  with Vite.

## Installing in Stremio

With Matzio running, open the settings window and click **Install in Stremio**,
or paste `http://127.0.0.1:7777/manifest.json` into Stremio's addon search box.

## Development

Requirements: [Rust](https://rustup.rs), Node.js 20+, and on Windows the
WebView2 runtime (preinstalled on Windows 10 and 11).

```sh
npm install
npm run tauri dev      # run with hot reload
cargo test -p matzio-addon
npm run tauri build    # Windows installer in target/release/bundle/nsis
```

Every pull request builds the Windows installer in CI; download it from the
run's **Artifacts** section.
