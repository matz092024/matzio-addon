import { invoke } from "@tauri-apps/api/core";

export type AddonStatus =
  | { state: "running"; manifestUrl: string }
  | { state: "failed"; error: string };

export type UpdateStatus =
  | { state: "notChecked" | "checking" | "upToDate" | "noRelease" }
  | { state: "available" | "installing"; version: string }
  | { state: "failed"; error: string };

export type Preferences = {
  sortOrder: string[];
  speedMbps: number | null;
  filterBySpeed: boolean;
  excludeResolutions: string[];
  displayStyle: string;
};

export type Settings = Preferences & { autoUpdate: boolean; setupComplete: boolean };

export type DebridService = "realdebrid" | "torbox";
export type DebridKeys = { realDebridKey: string | null; torboxKey: string | null };
export type KeyCheck = {
  ok: boolean;
  username: string | null;
  premium: boolean;
  expires: string | null;
  error: string | null;
};

export const api = {
  addonStatus: () => invoke<AddonStatus>("addon_status"),
  installInStremio: () => invoke("install_in_stremio"),
  appInfo: () => invoke<{ version: string }>("app_info"),
  getSettings: () => invoke<Settings>("get_settings"),
  setAutoUpdate: (enabled: boolean) => invoke<Settings>("set_auto_update", { enabled }),
  setSetupComplete: (complete: boolean) => invoke<Settings>("set_setup_complete", { complete }),
  savePreferences: (preferences: Preferences) => invoke<Settings>("save_preferences", { preferences }),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  checkForUpdates: () => invoke<UpdateStatus>("check_for_updates"),
  installUpdate: () => invoke<UpdateStatus>("install_update"),
  getDebridKeys: () => invoke<DebridKeys>("get_debrid_keys"),
  setDebridKeys: (keys: DebridKeys) => invoke<DebridKeys>("set_debrid_keys", keys),
  checkDebridKey: (service: DebridService, key: string) => invoke<KeyCheck>("check_debrid_key", { service, key }),
};
