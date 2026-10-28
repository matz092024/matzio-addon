// Settings shared by the wizard and the app. Widgets appear in both, so every
// change re-renders whatever is listening rather than patching the DOM in place.
import { api, type AddonStatus, type DebridKeys, type Preferences, type Settings, type SubtitleKeys } from "./api";

export const state = {
  settings: null as unknown as Settings,
  addon: null as unknown as AddonStatus,
  // null until loaded; stays null when this build has no debrid support yet.
  keys: null as DebridKeys | null,
  subtitleKeys: null as SubtitleKeys | null,
};

const listeners = new Set<() => void>();
export const onChange = (fn: () => void) => listeners.add(fn);
export const emit = () => listeners.forEach((fn) => fn());

export async function load() {
  [state.settings, state.addon] = await Promise.all([api.getSettings(), api.addonStatus()]);
  [state.keys, state.subtitleKeys] = await Promise.all([
    api.getDebridKeys().catch(() => null),
    api.getSubtitleKeys().catch(() => null),
  ]);
}

function preferences(s: Settings): Preferences {
  const { sortOrder, speedMbps, displayStyle, filters, sources, subtitles } = s;
  // A deep copy, so a change only shows once it's saved.
  return structuredClone({ sortOrder, speedMbps, displayStyle, filters, sources, subtitles });
}

export async function savePreferences(change: (p: Preferences) => void) {
  const next = preferences(state.settings);
  change(next);
  try {
    state.settings = await api.savePreferences(next);
    toast("Saved");
  } catch (e) {
    toast(`Couldn't save: ${e}`);
  }
  emit();
}

let toastTimer = 0;
export function toast(text: string) {
  const el = document.getElementById("toast")!;
  el.textContent = text;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.classList.remove("show"), 1600);
}

/** Small element builder so widgets read as markup without innerHTML. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { class?: string; data?: Record<string, string> } = {},
  ...children: (Node | string | null | false)[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  const { class: cls, data, ...rest } = props;
  if (cls) el.className = cls;
  if (data) Object.assign(el.dataset, data);
  Object.assign(el, rest);
  for (const child of children) if (child) el.append(child);
  return el;
}
