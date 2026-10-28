// The Sources page: where Matzio searches for torrents, each with its own switch.
import type { Sources } from "./api";
import { h, savePreferences, state } from "./state";

type SourceId = Exclude<keyof Sources, "urls">;

const SOURCES: { id: SourceId; name: string; hint: string; address?: string }[] = [
  { id: "torrentio", name: "Torrentio", hint: "The biggest public index. Covers most movies and shows.", address: "https://torrentio.strem.fun" },
  { id: "comet", name: "Comet", hint: "Fast, with lots of results that are often already cached on debrid.", address: "https://comet.elfhosted.com" },
  { id: "mediafusion", name: "MediaFusion", hint: "Strong for sports, live TV and non-English content.", address: "https://mediafusion.elfhosted.com" },
  { id: "torbox", name: "TorBox search", hint: "TorBox's own search. Only used when a TorBox key is connected." },
  { id: "zilean", name: "Zilean", hint: "Torrents from Debrid Media Manager lists. Mostly overlaps with Comet.", address: "https://zilean.elfhosted.com" },
  { id: "knaben", name: "Knaben", hint: "Text search over many public sites. Slower, and can find unrelated titles." },
];

function sourceCard(source: (typeof SOURCES)[number]) {
  const s = state.settings.sources;
  const on = s[source.id];
  const toggle = h("input", { type: "checkbox", checked: on, ariaLabel: source.name });
  toggle.onchange = () => savePreferences((p) => (p.sources[source.id] = toggle.checked));

  let address: HTMLElement | null = null;
  if (source.address) {
    const input = h("input", { value: s.urls[source.id] ?? "", placeholder: source.address, ariaLabel: `${source.name} address` });
    input.onchange = () =>
      savePreferences((p) => {
        const value = input.value.trim();
        if (value) p.sources.urls[source.id] = value;
        else delete p.sources.urls[source.id];
      });
    address = h("label", { class: "words-field" }, h("span", { class: "subtle small" }, "Address (leave empty for the public one, or paste your own instance or configured addon link)"), input);
  }

  const noKey = source.id === "torbox" && !state.keys?.torboxKey;
  return h(
    "div",
    { class: `glass check${on ? "" : " off"}` },
    h(
      "div",
      { class: "row" },
      h(
        "div",
        {},
        h("div", { class: "label" }, source.name),
        h("div", { class: "subtle small" }, noKey ? `${source.hint} Connect TorBox under Debrid to use it.` : source.hint),
      ),
      h("label", { class: "switch" }, toggle, h("span", { class: "track" }, h("span", { class: "thumb" }))),
    ),
    address && h("div", { class: "check-body" }, address),
  );
}

export function renderSources(el: HTMLElement) {
  el.replaceChildren(...SOURCES.map(sourceCard));
}
