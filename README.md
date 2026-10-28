# Matzio Addon

A self-hosted Stremio addon for Windows that finds streams for movies and shows
and plays them instantly through Real-Debrid or TorBox. It runs quietly in the
system tray on your own PC and is part of the Matzio family.

## Status

Step 2 of 4: real streams. Matzio finds torrents for the movie or episode,
lists TorBox results that are already cached (⚡) and Real-Debrid results,
and plays them through your own debrid account.

## How it works

- **Tray app** (`src-tauri/`): a [Tauri](https://tauri.app) app with a tray
  icon. Left-click it, or pick *Open settings*, to open the settings window.
  The window is destroyed when closed, so while idle only the tray icon and
  the addon server are running. Closing the window never quits; use
  *Quit Matzio* in the tray menu.
- **Addon server** (`crates/addon/`): serves the Stremio addon protocol on
  `http://127.0.0.1:7777/manifest.json`, reachable only from this PC.
  - Torrent results come from Torrentio (`sources/`).
  - TorBox results are filtered to cached ones with its bulk cache check.
    Real-Debrid has no cache check any more, so its results are checked when
    you press play: cached ones start instantly, others are left downloading
    on your Real-Debrid account and show an error in Stremio.
  - Picking a stream opens `/play/{service}/{hash}`, which adds the torrent to
    your debrid account, picks the right file (exact name, then episode
    number, then largest video) and redirects Stremio to the direct link.
    Links are reused for 30 minutes.
  - Debrid keys are saved in `%APPDATA%\com.matzio.addon\settings.json` and
    only ever sent to the debrid service itself.
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

## Releasing and auto-updates

Matzio checks
`https://github.com/matz092024/matzio-addon/releases/latest/download/latest.json`
shortly after starting and then every 6 hours. Updates are signed: the app
only installs files signed with the key whose public half is in
`src-tauri/tauri.conf.json`.

To publish a release:

1. Bump `version` in `package.json` (the app reads its version from there).
2. Merge to `main`. The **Release** workflow sees a version with no release
   yet, then builds, signs, and publishes the installer and `latest.json`
   under a `v<version>` tag. It can also be started by hand from the Actions tab. It needs the `TAURI_SIGNING_PRIVATE_KEY` repository secret
   (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if the key has a password).
