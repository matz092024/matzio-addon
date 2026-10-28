# Project context (handover)

Read this first if you are picking up Matzio Addon, whether you are Matz or an
AI assistant. It covers what the project is for, how it is built, how to
release it, where it stands and what comes next. Keep it updated as you go.

## Goal

A self-hosted Stremio addon that finds streams for movies and shows and plays
them instantly through a debrid service. It is for Matz and other people, and
should be more customizable than existing addons (Torrentio, Comet). The
configuration should feel like AIOStreams, but as a real desktop app.

Requirements:

- Debrid services: **Real-Debrid** and **TorBox**.
- Runs on each user's own PC. **Windows first**, maybe Linux later.
- Lives in the **system tray**; settings open in a real desktop window, not a
  browser tab. **Very low resource use while idle**: the window is destroyed
  when closed, only the tray icon and the addon server keep running.
- Look: like Stremio, **monochrome glass**, iOS-like.
- Features:
  - Customizable stream display in Stremio's picker.
  - User-ranked sort priorities with **drag to rank** (e.g. 1 language, 2 size).
  - "Too big for my connection" filter using the user's **internet speed and the
    title's runtime** (needed bitrate = size / runtime), not a flat GB cap.
  - Update checks and automatic updates.
- Companion to Matz's `matz092024/matzio` repo (the "Matzio family").

## Stack and architecture

Tauri 2 (Rust core, HTML/TypeScript window). Node/Docker and Electron were
rejected for idle resource use.

```
crates/addon/            Stremio addon server (Rust library, axum)
  src/lib.rs             routes: /manifest.json, /stream/..., /play/{service}/{hash}
  src/config.rs          debrid keys + preferences (shared, live-updated) + service base URLs
  src/streams.rs         ranks and formats Stremio stream entries
  src/torrent.rs         parses torrent titles (resolution, quality, languages, HDR, codec, audio...)
  src/filters.rs         the user's checks (hide / show at bottom), preference ranking, result limits
  src/sources/addon.rs       Stremio-addon sources (Torrentio, Comet, MediaFusion)
  src/sources/{torbox,zilean,knaben}.rs   API sources
  src/sources/cinemeta.rs    runtime (speed filter) and title (Knaben) lookup
  src/debrid/{realdebrid,torbox}.rs   add torrent, pick file, get direct link
  src/tests.rs           tests against local mock servers
src-tauri/               Tauri tray app
  src/lib.rs             tray menu, window, Tauri commands, starts the addon
  src/settings.rs        settings.json (keys, autoUpdate, setupComplete, preferences)
  src/updates.rs         update checks (start + every 6 h) and install
  tauri.conf.json        app config, updater public key, NSIS installer
  installer/*.bmp        branded installer header/sidebar images
index.html, src/*.ts     settings window (Vite + plain TS, no framework)
  wizard.ts              first-run setup wizard
  app.ts                 sidebar app: Home, Debrid, Sorting, Filters,
                         Stream display, Connection, Updates
  widgets.ts             drag-to-rank list and other controls
  api.ts / state.ts      calls into Tauri commands, UI state
.github/workflows/ci.yml       PRs: cargo test + build Windows installer artifact
.github/workflows/release.yml  main: publish release when version is new
```

How a stream plays: Stremio asks `127.0.0.1:7777` for streams, the addon gets
torrents from Torrentio, filters excluded resolutions/qualities and files too
large for the connection's speed over the title's runtime (from Cinemeta),
ranks them by the user's sort order, and formats them in the chosen display
style. TorBox results are filtered to cached ones (shown with ⚡), Real-Debrid
results are listed unchecked. Picking one opens `/play/{service}/{hash}`, which
adds the torrent to the debrid account, picks the file (exact name, then
episode number, then largest video) and redirects to the direct link. Links are
cached for 30 minutes.

Settings live in `%APPDATA%\com.matzio.addon\settings.json`. Debrid keys are
only ever sent to the debrid service itself.

## Build, run, test

Needs Rust, Node.js 20+, and WebView2 on Windows (preinstalled on 10/11).

```sh
npm install
npm run tauri dev          # run with hot reload
cargo test -p matzio-addon # addon tests (mocked services)
npm run tauri build        # installer in target/release/bundle/nsis
```

Every PR builds the Windows installer in CI; download it from the run's
**Artifacts** section to test before merging.

## Releasing and updates

1. Bump `version` in `package.json` (the app reads its version from there).
2. Merge to `main`. `release.yml` sees a version with no `v<version>` release,
   builds, signs and publishes the installer plus `latest.json`. It can also
   be started by hand from the Actions tab.

The app checks
`https://github.com/matz092024/matzio-addon/releases/latest/download/latest.json`.
Updates are signed: the public key is in `src-tauri/tauri.conf.json`, the
private key is the repo secret **`TAURI_SIGNING_PRIVATE_KEY`** (no password,
so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is unused). Matz generated the key and
keeps it locally. **Never commit the private key.** If it is lost, existing
installs can no longer auto-update; users would have to reinstall by hand.

Note for AI sessions: Claude cloud sessions could not push tags or create refs
through the API, which is why releases are driven by the version bump.

## Current status (2026-10-28)

- **v0.2.0 released**: tray app, addon server, signed auto-updates,
  glass settings window.
- **Merged on main, tested live**: real streams via Real-Debrid and
  TorBox (PR #5), and the "real app" UI with first-run wizard, sidebar app and
  branded installer (PR #6).
- **Step 3 implemented**: preferences (`sortOrder`, `speedMbps`,
  `filterBySpeed`, `excludeResolutions`, `displayStyle`) are wired into the
  addon `Config` and applied live to stream results (including Cinemeta runtime
  lookup for speed-based size filtering).

## Filters (since 0.4)

Preferences hold a `filters` object with one entry per check: speed, size
(min/max GB, per-resolution max), quality, language, hdr, codec, audio,
keywords and limits. Each has `enabled` and (except limits) `action`
(`hide` or `demote`). Tag checks have `avoid` and `prefer` lists; prefer lists
rank streams through the matching sort key. Settings from 0.3 and older
(`filterBySpeed`, `excludeResolutions`) are migrated on load in `config.rs`.

## Known caveats

- Real-Debrid removed its instant-availability check, so RD results are only
  checked at play time. Cached ones play instantly; uncached ones are left
  downloading on the RD account and Stremio shows an error.
- Sources (0.4): Torrentio, Comet, MediaFusion, TorBox search (TorBox users)
  on by default; Zilean and Knaben off. Searched in parallel with a 10 s cap
  each, merged by info hash. Public instance URLs can be replaced per source
  (`sources.urls`); those URLs are never included in exported settings. The
  Comet, MediaFusion, TorBox search, Zilean and Knaben request formats were
  written from their docs without live access; check them on a real install.
- Settings export/import (Home): sorting, display, filters and source switches
  only. Never debrid keys, custom source URLs or the measured speed.
- The addon listens on `127.0.0.1` only, so phones and TVs cannot reach it yet.

## Roadmap

Step 1 (tray app), step 2 (debrid streams) and step 3 (make the settings work)
are done. Remaining:

- **Step 4: polish and reach.** Start with Windows option (autostart), HTTPS / LAN
  access so phones and TVs running Stremio can use it, more sources.
- **Later:** Linux build.
