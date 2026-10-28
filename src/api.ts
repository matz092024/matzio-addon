import { invoke } from "@tauri-apps/api/core";

export type AddonStatus =
  | { state: "running"; manifestUrl: string }
  | { state: "failed"; error: string };

export type UpdateStatus =
  | { state: "notChecked" | "checking" | "upToDate" | "noRelease" }
  | { state: "available" | "installing"; version: string }
  | { state: "failed"; error: string };

export type Action = "hide" | "demote";
export type Check = { enabled: boolean; action: Action };
export type TagCheck = Check & { avoid: string[]; prefer: string[] };
export type Filters = {
  speed: Check;
  size: Check & { minGb: number | null; maxGb: number | null; maxGbByResolution: Record<string, number> };
  quality: TagCheck;
  language: Check & { preferred: string[]; onlyPreferred: boolean };
  hdr: TagCheck;
  codec: TagCheck;
  audio: TagCheck;
  keywords: TagCheck;
  limits: { enabled: boolean; total: number; perResolution: number };
};

export type Sources = {
  torrentio: boolean;
  comet: boolean;
  mediafusion: boolean;
  torbox: boolean;
  zilean: boolean;
  knaben: boolean;
  urls: Record<string, string>;
};

export type Placement = "show" | "demote" | "hide";
export type Subtitles = {
  enabled: boolean;
  openSubtitles: boolean;
  subdl: boolean;
  languages: string[];
  perLanguage: number;
  hearingImpaired: Placement;
  machineTranslated: Placement;
};

export type Preferences = {
  sortOrder: string[];
  speedMbps: number | null;
  displayStyle: string;
  nameTemplate: string;
  titleTemplate: string;
  filters: Filters;
  sources: Sources;
  subtitles: Subtitles;
};

export type Settings = Preferences & { autoUpdate: boolean; startWithWindows: boolean; setupComplete: boolean };

export type DebridService = "realdebrid" | "torbox";
export type DebridKeys = { realDebridKey: string | null; torboxKey: string | null };
export type KeyCheck = {
  ok: boolean;
  username: string | null;
  premium: boolean;
  expires: string | null;
  error: string | null;
};

export type SubtitleProvider = "opensubtitles" | "subdl";
export type SubtitleKeys = {
  openSubtitlesKey: string | null;
  openSubtitlesUsername: string | null;
  openSubtitlesPassword: string | null;
  subdlKey: string | null;
};
export type SubtitleCheck = { ok: boolean; message: string };

export const api = {
  addonStatus: () => invoke<AddonStatus>("addon_status"),
  installInStremio: () => invoke("install_in_stremio"),
  appInfo: () => invoke<{ version: string }>("app_info"),
  getSettings: () => invoke<Settings>("get_settings"),
  setAutoUpdate: (enabled: boolean) => invoke<Settings>("set_auto_update", { enabled }),
  setStartWithWindows: (enabled: boolean) => invoke<Settings>("set_start_with_windows", { enabled }),
  setSetupComplete: (complete: boolean) => invoke<Settings>("set_setup_complete", { complete }),
  savePreferences: (preferences: Preferences) => invoke<Settings>("save_preferences", { preferences }),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  checkForUpdates: () => invoke<UpdateStatus>("check_for_updates"),
  installUpdate: () => invoke<UpdateStatus>("install_update"),
  getDebridKeys: () => invoke<DebridKeys>("get_debrid_keys"),
  setDebridKeys: (keys: DebridKeys) => invoke<DebridKeys>("set_debrid_keys", keys),
  checkDebridKey: (service: DebridService, key: string) => invoke<KeyCheck>("check_debrid_key", { service, key }),
  previewStreamLabel: (displayStyle: string, nameTemplate: string, titleTemplate: string) =>
    invoke<{ name: string; title: string }>("preview_stream_label", { displayStyle, nameTemplate, titleTemplate }),
  streamLabelInfo: () =>
    invoke<{ variables: [string, string][]; defaultName: string; defaultTitle: string }>("stream_label_info"),
  getSubtitleKeys: () => invoke<SubtitleKeys>("get_subtitle_keys"),
  setSubtitleKeys: (keys: SubtitleKeys) => invoke<SubtitleKeys>("set_subtitle_keys", { keys }),
  checkSubtitleKey: (provider: SubtitleProvider, keys: SubtitleKeys) => invoke<SubtitleCheck>("check_subtitle_key", { provider, keys }),
};
