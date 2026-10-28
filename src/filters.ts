// The Filters page: one card per check, each with its own switch and a choice
// between hiding failing streams and showing them at the bottom.
import type { Action, Filters, TagCheck } from "./api";
import { h, savePreferences, state } from "./state";

type CheckId = keyof Filters;

export function switchInput(checked: boolean, label: string, onChange: (on: boolean) => void) {
  const input = h("input", { type: "checkbox", checked, ariaLabel: label });
  input.onchange = () => onChange(input.checked);
  return h("label", { class: "switch" }, input, h("span", { class: "track" }, h("span", { class: "thumb" })));
}

function save(change: (f: Filters) => void) {
  return savePreferences((p) => change(p.filters));
}

function actionRow(id: Exclude<CheckId, "limits">) {
  const current = state.settings.filters[id].action;
  const option = (action: Action, label: string) => {
    const b = h("button", { type: "button", class: action === current ? "on" : "", ariaPressed: String(action === current) }, label);
    b.onclick = () => save((f) => (f[id].action = action));
    return b;
  };
  return h(
    "div",
    { class: "row action-row" },
    h("span", { class: "subtle small" }, "Streams that don't pass"),
    h("div", { class: "segmented" }, option("hide", "Hide"), option("demote", "Show at the bottom")),
  );
}

function card(id: CheckId, title: string, hint: string, ...body: (Node | null)[]) {
  const check = state.settings.filters[id];
  return h(
    "div",
    { class: `glass check${check.enabled ? "" : " off"}` },
    h(
      "div",
      { class: "row" },
      h("div", {}, h("div", { class: "label" }, title), h("div", { class: "subtle small" }, hint)),
      switchInput(check.enabled, title, (on) => save((f) => (f[id].enabled = on))),
    ),
    h("div", { class: "check-body" }, ...body),
  );
}

/** A number box that saves on change; empty means "no limit". */
export function numberField(label: string, value: number | null | undefined, unit: string, onSave: (v: number | null) => void) {
  const input = h("input", { type: "number", min: "0", step: "any", value: value ? String(value) : "", placeholder: "—", ariaLabel: label });
  input.onchange = () => {
    const v = Number(input.value);
    onSave(input.value.trim() && v > 0 ? v : null);
  };
  return h("label", { class: "num-field" }, h("span", { class: "subtle small" }, label), h("span", { class: "num-input" }, input, h("span", { class: "subtle small" }, unit)));
}

/** Chips that cycle: neutral → preferred → avoided → neutral. */
function tagChips(id: "hdr" | "codec" | "audio", options: [string, string][]) {
  const check = state.settings.filters[id];
  return h(
    "div",
    { class: "chips" },
    ...options.map(([value, label]) => {
      const rank = check.prefer.indexOf(value);
      const avoided = check.avoid.includes(value);
      const mark = rank >= 0 ? `★${check.prefer.length > 1 ? rank + 1 : ""} ` : avoided ? "✕ " : "";
      const chip = h(
        "button",
        { class: `toggle-chip${rank >= 0 ? " prefer" : avoided ? " avoid" : ""}`, type: "button", title: "Click to prefer, again to avoid, again to reset" },
        mark + label,
      );
      chip.onclick = () =>
        save((f) => {
          const c: TagCheck = f[id];
          if (rank >= 0) {
            c.prefer = c.prefer.filter((v) => v !== value);
            c.avoid = [...c.avoid, value];
          } else if (avoided) {
            c.avoid = c.avoid.filter((v) => v !== value);
          } else {
            c.prefer = [...c.prefer, value];
          }
        });
      return chip;
    }),
  );
}

const legend = () => h("div", { class: "subtle small legend" }, "Click once to prefer (★, ranked in click order), twice to avoid (✕), three times to reset.");

const QUALITIES: [string, string][] = [
  ["2160p", "4K"],
  ["1440p", "1440p"],
  ["1080p", "1080p"],
  ["720p", "720p"],
  ["480p", "480p and lower"],
  ["remux", "Remux"],
  ["bluray", "BluRay"],
  ["web", "WEB"],
  ["tv", "HDTV / DVD"],
  ["scr", "Screener"],
  ["cam", "CAM"],
];

export const LANGUAGES: [string, string][] = [
  ["en", "English"], ["de", "German"], ["fr", "French"], ["es", "Spanish"], ["it", "Italian"],
  ["pt", "Portuguese"], ["nl", "Dutch"], ["pl", "Polish"], ["ru", "Russian"], ["uk", "Ukrainian"],
  ["cs", "Czech"], ["hu", "Hungarian"], ["tr", "Turkish"], ["el", "Greek"], ["sv", "Swedish"],
  ["da", "Danish"], ["no", "Norwegian"], ["fi", "Finnish"], ["ar", "Arabic"], ["he", "Hebrew"],
  ["hi", "Hindi"], ["ta", "Tamil"], ["te", "Telugu"], ["ja", "Japanese"], ["ko", "Korean"],
  ["zh", "Chinese"], ["th", "Thai"], ["vi", "Vietnamese"], ["multi", "Multi-audio"],
];

const HDR: [string, string][] = [
  ["dv", "Dolby Vision"],
  ["dv-only", "Dolby Vision without HDR10"],
  ["hdr10plus", "HDR10+"],
  ["hdr", "HDR"],
  ["sdr", "SDR"],
];

const CODECS: [string, string][] = [
  ["av1", "AV1"],
  ["hevc", "HEVC / x265"],
  ["avc", "AVC / x264"],
  ["xvid", "XviD"],
];

const AUDIO: [string, string][] = [
  ["atmos", "Atmos"],
  ["truehd", "TrueHD"],
  ["dts-x", "DTS:X"],
  ["dts-hd", "DTS-HD"],
  ["dts", "Any DTS"],
  ["ddp", "Dolby Digital+"],
  ["dd", "Dolby Digital"],
  ["aac", "AAC"],
  ["flac", "FLAC"],
  ["7.1", "7.1"],
  ["5.1", "5.1"],
  ["2.0", "Stereo"],
];

function speedCard() {
  const mbps = state.settings.speedMbps;
  return card(
    "speed",
    "Too big for my connection",
    mbps
      ? `Uses your ${Math.round(mbps)} Mbps connection and each title's runtime, so a 4K film and a 1080p episode are judged fairly.`
      : "Measure your speed under Connection first.",
    actionRow("speed"),
  );
}

function sizeCard() {
  const s = state.settings.filters.size;
  const byRes = (key: string, label: string) =>
    numberField(label, s.maxGbByResolution[key], "GB", (v) =>
      save((f) => {
        if (v) f.size.maxGbByResolution[key] = v;
        else delete f.size.maxGbByResolution[key];
      }),
    );
  return card(
    "size",
    "Size limits",
    "Plain limits in GB, on top of the connection check. Leave a box empty for no limit.",
    h(
      "div",
      { class: "num-grid" },
      numberField("Smallest", s.minGb, "GB", (v) => save((f) => (f.size.minGb = v))),
      numberField("Largest", s.maxGb, "GB", (v) => save((f) => (f.size.maxGb = v))),
    ),
    h("div", { class: "subtle small" }, "Largest per resolution (overrides the above)"),
    h("div", { class: "num-grid" }, byRes("2160p", "4K"), byRes("1080p", "1080p"), byRes("720p", "720p")),
    actionRow("size"),
  );
}

function qualityCard() {
  const avoid = state.settings.filters.quality.avoid;
  return card(
    "quality",
    "Resolution and source",
    "Streams with these never show up (or go to the bottom).",
    h(
      "div",
      { class: "chips" },
      ...QUALITIES.map(([value, label]) => {
        const on = avoid.includes(value);
        const chip = h("button", { class: `toggle-chip${on ? " avoid" : ""}`, type: "button", ariaPressed: String(on) }, (on ? "✕ " : "") + label);
        chip.onclick = () => save((f) => (f.quality.avoid = on ? f.quality.avoid.filter((v) => v !== value) : [...f.quality.avoid, value]));
        return chip;
      }),
    ),
    actionRow("quality"),
  );
}

function languageCard() {
  const lang = state.settings.filters.language;
  const chips = h(
    "div",
    { class: "chips" },
    ...LANGUAGES.map(([code, label]) => {
      const rank = lang.preferred.indexOf(code);
      const chip = h("button", { class: `toggle-chip${rank >= 0 ? " prefer" : ""}`, type: "button", ariaPressed: String(rank >= 0) }, rank >= 0 ? `${rank + 1}  ${label}` : label);
      chip.onclick = () =>
        save((f) => (f.language.preferred = rank >= 0 ? f.language.preferred.filter((c) => c !== code) : [...f.language.preferred, code]));
      return chip;
    }),
  );
  return card(
    "language",
    "Languages",
    "Pick your languages in order. Matched by release names and Torrentio's flags; a release that names no language counts as English.",
    chips,
    h(
      "div",
      { class: "row" },
      h("div", {}, h("div", { class: "label" }, "Only these languages"), h("div", { class: "subtle small" }, "Off: other languages are only ranked lower.")),
      switchInput(lang.onlyPreferred, "Only these languages", (on) => save((f) => (f.language.onlyPreferred = on))),
    ),
    lang.onlyPreferred ? actionRow("language") : null,
  );
}

function wordsField(label: string, words: string[], onSave: (words: string[]) => void) {
  const input = h("input", { value: words.join(", "), placeholder: "e.g. HC, 3D, YIFY", ariaLabel: label });
  input.onchange = () => onSave(input.value.split(",").map((w) => w.trim()).filter(Boolean));
  return h("label", { class: "words-field" }, h("span", { class: "subtle small" }, label), input);
}

function keywordsCard() {
  const k = state.settings.filters.keywords;
  return card(
    "keywords",
    "Words and release groups",
    "Separate with commas. Matches whole words in the release name; write /like this/ for a regular expression.",
    wordsField("Avoid", k.avoid, (w) => save((f) => (f.keywords.avoid = w))),
    wordsField("Prefer (first ones count most)", k.prefer, (w) => save((f) => (f.keywords.prefer = w))),
    actionRow("keywords"),
  );
}

function limitsCard() {
  const l = state.settings.filters.limits;
  return card(
    "limits",
    "Result limits",
    "Keep Stremio's list short. Counts after sorting, so the best streams stay.",
    h(
      "div",
      { class: "num-grid" },
      numberField("Streams in total", l.total, "", (v) => save((f) => (f.limits.total = Math.round(v ?? 0)))),
      numberField("Per resolution", l.perResolution, "", (v) => save((f) => (f.limits.perResolution = Math.round(v ?? 0)))),
    ),
  );
}

export function renderFilters(el: HTMLElement) {
  el.replaceChildren(
    speedCard(),
    sizeCard(),
    qualityCard(),
    languageCard(),
    card("hdr", "HDR and Dolby Vision", "Dolby Vision without HDR10 shows wrong colours on many TVs.", tagChips("hdr", HDR), legend(), actionRow("hdr")),
    card("codec", "Video codec", "Older TVs and sticks can't play AV1, some not even HEVC.", tagChips("codec", CODECS), legend(), actionRow("codec")),
    card("audio", "Audio", "TrueHD and DTS often play without sound on TVs.", tagChips("audio", AUDIO), legend(), actionRow("audio")),
    keywordsCard(),
    limitsCard(),
  );
}
