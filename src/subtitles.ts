// The Subtitles page: providers with the user's own free keys, languages in order,
// and which kinds of subtitles to show.
import { api, type Placement, type SubtitleCheck, type SubtitleKeys, type SubtitleProvider, type Subtitles } from "./api";
import { numberField, switchInput } from "./filters";
import { emit, h, savePreferences, state, toast } from "./state";

const LANGUAGES: [string, string][] = [
  ["en", "English"], ["de", "German"], ["fr", "French"], ["es", "Spanish"], ["it", "Italian"],
  ["pt", "Portuguese"], ["pt-br", "Portuguese (Brazil)"], ["nl", "Dutch"], ["pl", "Polish"], ["ru", "Russian"],
  ["uk", "Ukrainian"], ["cs", "Czech"], ["sk", "Slovak"], ["hu", "Hungarian"], ["ro", "Romanian"],
  ["bg", "Bulgarian"], ["hr", "Croatian"], ["sr", "Serbian"], ["sl", "Slovenian"], ["tr", "Turkish"],
  ["el", "Greek"], ["sv", "Swedish"], ["da", "Danish"], ["no", "Norwegian"], ["fi", "Finnish"],
  ["et", "Estonian"], ["lv", "Latvian"], ["lt", "Lithuanian"], ["ar", "Arabic"], ["he", "Hebrew"],
  ["fa", "Persian"], ["hi", "Hindi"], ["ta", "Tamil"], ["te", "Telugu"], ["ja", "Japanese"],
  ["ko", "Korean"], ["zh", "Chinese"], ["th", "Thai"], ["vi", "Vietnamese"], ["id", "Indonesian"], ["ms", "Malay"],
];

type Field = keyof SubtitleKeys;
const PROVIDERS: {
  id: SubtitleProvider;
  pref: "openSubtitles" | "subdl";
  name: string;
  hint: string;
  where: string;
  fields: { field: Field; label: string; secret: boolean; optional?: boolean }[];
}[] = [
  {
    id: "opensubtitles",
    pref: "openSubtitles",
    name: "OpenSubtitles",
    hint: "The biggest library. A free key allows 5 downloads a day; sign in with your free account for 20.",
    where: "opensubtitles.com/consumers",
    fields: [
      { field: "openSubtitlesKey", label: "API key", secret: true },
      { field: "openSubtitlesUsername", label: "Username (optional)", secret: false, optional: true },
      { field: "openSubtitlesPassword", label: "Password (optional)", secret: true, optional: true },
    ],
  },
  {
    id: "subdl",
    pref: "subdl",
    name: "SubDL",
    hint: "Large library with no daily download limit.",
    where: "subdl.com/panel/api",
    fields: [{ field: "subdlKey", label: "API key", secret: true }],
  },
];

function save(change: (s: Subtitles) => void) {
  return savePreferences((p) => change(p.subtitles));
}

// Half-typed values and check results, kept when the page re-renders.
const drafts: Partial<Record<Field, string>> = {};
const checks: Partial<Record<SubtitleProvider, SubtitleCheck | "checking">> = {};

function providerCard(p: (typeof PROVIDERS)[number]) {
  const keys = state.subtitleKeys!;
  const on = state.settings.subtitles[p.pref];
  const saved = keys[p.fields[0].field];
  const status = checks[p.id];

  const inputs = p.fields.map((f) => {
    const current = keys[f.field];
    const input = h("input", {
      type: f.secret ? "password" : "text",
      value: drafts[f.field] ?? (f.secret ? "" : current ?? ""),
      placeholder: f.secret && current ? (f.optional ? "Saved" : "Saved · type to replace") : f.optional ? "" : "Paste your key",
      ariaLabel: `${p.name} ${f.label}`,
    });
    input.oninput = () => (drafts[f.field] = input.value);
    return { f, input };
  });

  const connect = h("button", { class: "ghost", type: "button" }, saved ? "Check" : "Connect");
  connect.onclick = async () => {
    const next: SubtitleKeys = { ...keys };
    for (const { f, input } of inputs) {
      const typed = input.value.trim();
      // An empty secret box keeps what is saved; an emptied username removes it.
      if (typed || !f.secret) next[f.field] = typed || null;
    }
    if (!next[p.fields[0].field]) return inputs[0].input.focus();
    checks[p.id] = "checking";
    emit();
    checks[p.id] = await api.checkSubtitleKey(p.id, next).catch((e) => ({ ok: false, message: String(e) }));
    if ((checks[p.id] as SubtitleCheck).ok) {
      p.fields.forEach(({ field }) => delete drafts[field]);
      state.subtitleKeys = await api.setSubtitleKeys(next);
      toast(`${p.name} connected`);
    }
    emit();
  };

  const remove = h("button", { class: "link", type: "button" }, "Remove");
  remove.onclick = async () => {
    const next: SubtitleKeys = { ...keys };
    p.fields.forEach(({ field }) => (next[field] = null));
    state.subtitleKeys = await api.setSubtitleKeys(next);
    delete checks[p.id];
    emit();
  };

  let line: HTMLElement | null = null;
  if (status === "checking") line = h("div", { class: "status-line" }, "Checking…");
  else if (status) line = h("div", { class: `status-line ${status.ok ? "good" : "bad"}` }, status.message);
  else if (saved) line = h("div", { class: "status-line" }, "Saved");

  const [key, ...account] = inputs;
  return h(
    "div",
    { class: `glass check${on ? "" : " off"}` },
    h(
      "div",
      { class: "row" },
      h("div", {}, h("div", { class: "label" }, p.name), h("div", { class: "subtle small" }, `${p.hint} Get a free key at ${p.where}.`)),
      switchInput(on, p.name, (v) => save((s) => (s[p.pref] = v))),
    ),
    h(
      "div",
      { class: "check-body" },
      h("div", { class: "field" }, key.input, connect),
      account.length ? h("div", { class: "num-grid" }, ...account.map(({ f, input }) => h("label", { class: "words-field" }, h("span", { class: "subtle small" }, f.label), input))) : null,
      line || saved ? h("div", { class: "row" }, line ?? h("span", {}), saved ? remove : null) : null,
    ),
  );
}

function languagesCard() {
  const chosen = state.settings.subtitles.languages;
  return h(
    "div",
    { class: "glass check" },
    h("div", { class: "label" }, "Languages"),
    h("div", { class: "subtle small" }, "Pick the languages you want, in order. Stremio lists them in this order, best match for your file first."),
    h(
      "div",
      { class: "check-body" },
      h(
        "div",
        { class: "chips" },
        ...LANGUAGES.map(([code, label]) => {
          const rank = chosen.indexOf(code);
          const chip = h("button", { class: `toggle-chip${rank >= 0 ? " prefer" : ""}`, type: "button", ariaPressed: String(rank >= 0) }, rank >= 0 ? `${rank + 1}  ${label}` : label);
          chip.onclick = () => save((s) => (s.languages = rank >= 0 ? s.languages.filter((c) => c !== code) : [...s.languages, code]));
          return chip;
        }),
      ),
    ),
  );
}

function placementRow(label: string, hint: string, key: "hearingImpaired" | "machineTranslated") {
  const current = state.settings.subtitles[key];
  const option = (value: Placement, text: string) => {
    const b = h("button", { type: "button", class: value === current ? "on" : "", ariaPressed: String(value === current) }, text);
    b.onclick = () => save((s) => (s[key] = value));
    return b;
  };
  return h(
    "div",
    { class: "row action-row" },
    h("div", {}, h("div", { class: "label" }, label), h("div", { class: "subtle small" }, hint)),
    h("div", { class: "segmented" }, option("show", "Show"), option("demote", "At the bottom"), option("hide", "Hide")),
  );
}

function optionsCard() {
  const s = state.settings.subtitles;
  return h(
    "div",
    { class: "glass check" },
    h("div", { class: "label" }, "Which subtitles"),
    h(
      "div",
      { class: "check-body" },
      h("div", { class: "num-grid" }, numberField("Most per language", s.perLanguage, "", (v) => save((x) => (x.perLanguage = Math.round(v ?? 0))))),
      placementRow("Hearing impaired", "Include sound descriptions like [door creaks].", "hearingImpaired"),
      placementRow("Machine translated", "Made by a translation tool; often clumsy.", "machineTranslated"),
    ),
  );
}

export function renderSubtitles(el: HTMLElement) {
  const s = state.settings.subtitles;
  const master = h(
    "div",
    { class: `glass check${s.enabled ? "" : " off"}` },
    h(
      "div",
      { class: "row" },
      h(
        "div",
        {},
        h("div", { class: "label" }, "Subtitles in Stremio"),
        h("div", { class: "subtle small" }, "Matzio adds subtitles from the providers below to every movie and episode. A subtitle is only downloaded when you pick it in the player."),
      ),
      switchInput(s.enabled, "Subtitles in Stremio", (on) => save((x) => (x.enabled = on))),
    ),
  );
  if (!state.subtitleKeys) {
    el.replaceChildren(h("div", { class: "glass" }, h("h2", {}, "Coming in the next update"), h("p", { class: "subtle" }, "This version of Matzio can't fetch subtitles yet.")));
    return;
  }
  const body = s.enabled ? [...PROVIDERS.map(providerCard), languagesCard(), optionsCard()] : [];
  el.replaceChildren(master, ...body);
}
