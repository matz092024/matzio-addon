// Building blocks used by both the setup wizard and the app's sections.
import { api, type DebridService, type KeyCheck } from "./api";
import { emit, h, onChange, savePreferences, state, toast } from "./state";

/** Render into every element matching `selector`, and again whenever settings change. */
function mountAll(selector: string, render: (el: HTMLElement) => void) {
  const draw = () => document.querySelectorAll<HTMLElement>(selector).forEach(render);
  onChange(draw);
  draw();
}

// ---------- Install in Stremio ----------

function renderInstall(el: HTMLElement) {
  const addon = state.addon;
  if (addon.state === "failed") {
    el.replaceChildren(
      h("h2", {}, "Matzio isn't running"),
      h("p", { class: "subtle" }, addon.error),
    );
    return;
  }
  const copy = h("button", { class: "ghost", type: "button" }, "Copy");
  copy.onclick = async () => {
    await navigator.clipboard.writeText(addon.manifestUrl);
    toast("Link copied");
  };
  const install = h("button", { class: "primary wide", type: "button" }, "Install in Stremio");
  install.onclick = () => api.installInStremio().catch((e) => toast(`Could not open Stremio: ${e}`));
  el.replaceChildren(
    h("div", { class: "field" }, h("input", { readOnly: true, value: addon.manifestUrl, ariaLabel: "Addon link" }), copy),
    install,
    h("p", { class: "subtle small center" }, "No Stremio app on this PC? Paste the link into Stremio's addon search instead."),
  );
}

// ---------- Debrid accounts ----------

const SERVICES: { id: DebridService; name: string; field: "realDebridKey" | "torboxKey"; where: string }[] = [
  { id: "realdebrid", name: "Real-Debrid", field: "realDebridKey", where: "real-debrid.com/apitoken" },
  { id: "torbox", name: "TorBox", field: "torboxKey", where: "torbox.app/settings" },
];

// Result of checking each saved or entered key, shared by every card for that service.
const checks: Partial<Record<DebridService, KeyCheck | "checking">> = {};

function describe(check: KeyCheck) {
  if (!check.ok) return check.error ?? "That key didn't work";
  const who = check.username ? `${check.username} · ` : "";
  if (!check.premium) return `${who}Connected, but the account isn't premium`;
  const date = check.expires ? new Date(check.expires) : null;
  const until = date && !isNaN(date.getTime()) ? ` until ${date.toLocaleDateString(undefined, { dateStyle: "medium" })}` : "";
  return `${who}Premium${until}`;
}

async function check(service: DebridService, key: string) {
  checks[service] = "checking";
  emit();
  checks[service] = await api
    .checkDebridKey(service, key)
    .catch((e) => ({ ok: false, username: null, premium: false, expires: null, error: String(e) }));
  emit();
  return checks[service] as KeyCheck;
}

// Half-typed keys, kept when something else (like a key check) re-renders the cards.
const drafts: Partial<Record<DebridService, string>> = {};

function debridCard(service: (typeof SERVICES)[number]) {
  const keys = state.keys!;
  const saved = keys[service.field];
  const status = checks[service.id];
  const input = h("input", {
    type: "password",
    placeholder: saved ? "Saved · paste to replace" : "Paste your API key",
    ariaLabel: `${service.name} API key`,
    value: drafts[service.id] ?? "",
  });
  input.oninput = () => (drafts[service.id] = input.value);
  const connect = h("button", { class: "ghost", type: "button" }, "Connect");
  connect.onclick = async () => {
    const key = input.value.trim();
    if (!key) return input.focus();
    if (!(await check(service.id, key)).ok) return;
    delete drafts[service.id];
    state.keys = await api.setDebridKeys({ ...state.keys!, [service.field]: key });
    toast(`${service.name} connected`);
    emit();
  };
  input.onkeydown = (e) => e.key === "Enter" && connect.click();

  const remove = h("button", { class: "link", type: "button" }, "Remove");
  remove.onclick = async () => {
    state.keys = await api.setDebridKeys({ ...state.keys!, [service.field]: null });
    delete checks[service.id];
    emit();
  };

  let line: HTMLElement | null = null;
  if (status === "checking") line = h("div", { class: "status-line" }, "Checking…");
  else if (status) line = h("div", { class: `status-line ${status.ok && status.premium ? "good" : "bad"}` }, describe(status));
  else if (saved) line = h("div", { class: "status-line" }, "Saved");

  return h(
    "div",
    { class: `glass service${saved ? " selected" : ""}` },
    h("div", { class: "row" }, h("h3", {}, service.name), saved ? remove : null),
    h("div", { class: "subtle small" }, `Get your key at ${service.where}`),
    h("div", { class: "field" }, input, connect),
    line,
  );
}

function renderDebrid(el: HTMLElement) {
  if (!state.keys) {
    el.replaceChildren(
      h("div", { class: "glass span" }, h("h2", {}, "Coming in the next update"), h("p", { class: "subtle" }, "This version of Matzio can't connect debrid accounts yet. Updates install automatically, so this step will be ready soon.")),
    );
    return;
  }
  el.replaceChildren(...SERVICES.map(debridCard));
}

/** Check saved keys once so the cards can say "Premium until …" instead of just "Saved". */
function checkSavedKeys() {
  if (!state.keys) return;
  for (const s of SERVICES) {
    const key = state.keys[s.field];
    if (key) check(s.id, key);
  }
}

// ---------- Internet speed ----------

const SPEED_TEST_URL = "https://speed.cloudflare.com/__down?bytes=";
let measuring = false;

/** Download for a few seconds and report megabits per second. */
async function measureSpeed(): Promise<number> {
  const controller = new AbortController();
  const limit = setTimeout(() => controller.abort(), 8000);
  const started = performance.now();
  let bytes = 0;
  try {
    const response = await fetch(SPEED_TEST_URL + 100_000_000, { signal: controller.signal, cache: "no-store" });
    const reader = response.body!.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.length;
    }
  } catch (e) {
    // Hitting the time limit is expected on slower lines; anything else is a real failure.
    if (!controller.signal.aborted || bytes === 0) throw e;
  } finally {
    clearTimeout(limit);
  }
  const seconds = (performance.now() - started) / 1000;
  return (bytes * 8) / seconds / 1_000_000;
}

function renderSpeed(el: HTMLElement) {
  const mbps = state.settings.speedMbps;
  // 1 Mbps sustained is 0.45 GB per hour of video.
  const gbPerHour = mbps ? mbps * 0.45 : null;
  const measure = h("button", { class: "primary", type: "button", disabled: measuring }, measuring ? "Measuring…" : mbps ? "Measure again" : "Measure my speed");
  measure.onclick = async () => {
    measuring = true;
    emit();
    try {
      const result = await measureSpeed();
      await savePreferences((p) => (p.speedMbps = Math.round(result)));
    } catch {
      toast("Couldn't measure. Type your speed instead.");
    } finally {
      measuring = false;
      emit();
    }
  };
  const manual = h("input", { type: "number", min: "1", step: "1", value: mbps ? String(mbps) : "", placeholder: "Mbps", ariaLabel: "Speed in megabits per second" });
  manual.onchange = () => {
    const value = Number(manual.value);
    savePreferences((p) => (p.speedMbps = value > 0 ? value : null));
  };

  el.replaceChildren(
    h("div", { class: "speed-number" }, h("span", {}, measuring ? "…" : mbps ? String(Math.round(mbps)) : "—"), h("small", {}, "Mbps")),
    h(
      "p",
      { class: "subtle center" },
      gbPerHour
        ? `Plays files up to about ${gbPerHour.toFixed(0)} GB per hour of runtime, so a 2 hour film up to ${(gbPerHour * 2).toFixed(0)} GB.`
        : "Not measured yet. The test downloads for a few seconds.",
    ),
    h("div", { class: "speed-actions" }, measure, h("span", { class: "subtle small" }, "or type it"), manual),
  );
}

// ---------- Sort ranking ----------

export const SORT_KEYS: Record<string, { label: string; hint: string }> = {
  cached: { label: "Cached first", hint: "Instant play on your debrid" },
  resolution: { label: "Resolution", hint: "4K → 1080p → 720p" },
  language: { label: "Language", hint: "Your languages first" },
  size: { label: "Size", hint: "Largest that plays smoothly" },
  quality: { label: "Source", hint: "Remux → BluRay → WEB" },
  seeders: { label: "Seeders", hint: "Most shared first" },
};

function saveOrder(list: HTMLElement) {
  const order = [...list.children].map((row) => (row as HTMLElement).dataset.key!);
  if (order.join() !== state.settings.sortOrder.join()) savePreferences((p) => (p.sortOrder = order));
}

function renumber(list: HTMLElement) {
  [...list.children].forEach((row, i) => (row.querySelector(".num")!.textContent = String(i + 1)));
}

/** Pointer-driven reordering; Alt+Up/Down does the same from the keyboard. */
function makeSortable(list: HTMLElement) {
  list.addEventListener("pointerdown", (down) => {
    const row = (down.target as HTMLElement).closest<HTMLElement>(".rank-row");
    if (!row || down.button !== 0) return;
    down.preventDefault();
    row.setPointerCapture(down.pointerId);
    row.classList.add("dragging");
    const move = (e: PointerEvent) => {
      const over = [...list.children].find((other) => {
        const box = other.getBoundingClientRect();
        return other !== row && e.clientY > box.top && e.clientY < box.bottom;
      });
      if (!over) return;
      const after = e.clientY > over.getBoundingClientRect().top + over.clientHeight / 2;
      over[after ? "after" : "before"](row);
      renumber(list);
    };
    const up = () => {
      row.classList.remove("dragging");
      row.removeEventListener("pointermove", move);
      saveOrder(list);
    };
    row.addEventListener("pointermove", move);
    row.addEventListener("pointerup", up, { once: true });
    row.addEventListener("pointercancel", up, { once: true });
  });
  list.addEventListener("keydown", (e) => {
    const row = e.target as HTMLElement;
    if (!e.altKey || !row.classList.contains("rank-row")) return;
    if (e.key === "ArrowUp" && row.previousElementSibling) row.previousElementSibling.before(row);
    else if (e.key === "ArrowDown" && row.nextElementSibling) row.nextElementSibling.after(row);
    else return;
    e.preventDefault();
    row.focus();
    renumber(list);
    saveOrder(list);
  });
}

function renderSorting(el: HTMLElement) {
  const list = h("div", { class: "glass rank" });
  state.settings.sortOrder
    .filter((key) => key in SORT_KEYS)
    .forEach((key, i) =>
      list.append(
        h(
          "div",
          { class: "rank-row", tabIndex: 0, data: { key }, title: "Drag, or press Alt+Up/Down" },
          h("span", { class: "num" }, String(i + 1)),
          h("b", {}, SORT_KEYS[key].label),
          h("span", { class: "chip" }, SORT_KEYS[key].hint),
          h("span", { class: "grip", ariaHidden: "true" }, "⋮⋮"),
        ),
      ),
    );
  makeSortable(list);
  el.replaceChildren(list);
}

export function mountWidgets() {
  mountAll("[data-install]", renderInstall);
  mountAll("[data-debrid]", renderDebrid);
  mountAll("[data-speed]", renderSpeed);
  mountAll("[data-sorting]", renderSorting);
  checkSavedKeys();
}
