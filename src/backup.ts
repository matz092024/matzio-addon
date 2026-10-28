// Export and import of Matzio's stream settings as a file to keep or share.
// Debrid keys and custom source addresses are never part of it, and importing keeps
// this PC's keys, addresses and speed.
import { api, type Preferences } from "./api";
import { emit, state, toast } from "./state";

const FORMAT = "matzio-settings";

export function exportSettings() {
  const { sortOrder, displayStyle, filters } = state.settings;
  // Custom source addresses can be configured addon links with someone's debrid key inside.
  const { urls: _private, ...sources } = state.settings.sources;
  const file = { format: FORMAT, version: 1, preferences: { sortOrder, displayStyle, filters, sources } };
  const blob = new Blob([JSON.stringify(file, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "matzio-settings.json";
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  toast("Saved to your Downloads folder");
}

export function importSettings() {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".json,application/json";
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    try {
      const parsed = JSON.parse(await file.text());
      if (parsed?.format !== FORMAT || typeof parsed.preferences !== "object") throw new Error("not a Matzio settings file");
      const { sortOrder, displayStyle, filters, sources } = parsed.preferences as Partial<Preferences>;
      // Only the shareable parts; anything missing falls back to defaults in the app.
      const urls = state.settings.sources.urls;
      const next = { sortOrder, displayStyle, filters, sources: { ...sources, urls }, speedMbps: state.settings.speedMbps } as Preferences;
      state.settings = await api.savePreferences(next);
      toast("Settings imported");
      emit();
    } catch (e) {
      toast(`Couldn't import: ${e instanceof Error ? e.message : e}`);
    }
  };
  input.click();
}
