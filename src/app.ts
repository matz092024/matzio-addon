// The app after setup: a sidebar of sections.
import { api, type UpdateStatus } from "./api";
import { listen } from "@tauri-apps/api/event";
import { h, onChange, savePreferences, state } from "./state";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const app = $("app");

function go(page: string) {
  app.querySelectorAll<HTMLElement>("[data-page]").forEach((el) => {
    const match = el.dataset.page === page;
    if (el.classList.contains("nav-item")) el.classList.toggle("active", match);
    else el.hidden = !match;
  });
}
app.querySelectorAll<HTMLElement>(".nav-item").forEach((item) => (item.onclick = () => go(item.dataset.page!)));

// ---------- Status ----------

function showStatus() {
  const pill = $("status");
  if (state.addon.state === "failed") {
    $("status-text").textContent = "Not running";
    pill.classList.add("bad");
    $("error-text").textContent = state.addon.error;
    $("error").hidden = false;
  } else {
    $("status-text").textContent = "Running";
    pill.classList.add("good");
  }
}

// ---------- Filters ----------

const QUALITIES = [
  ["2160p", "4K"],
  ["1080p", "1080p"],
  ["720p", "720p"],
  ["480p", "480p and lower"],
  ["cam", "CAM"],
  ["scr", "Screener"],
];

function renderFilters() {
  const s = state.settings;
  $<HTMLInputElement>("filter-by-speed").checked = s.filterBySpeed;
  $("speed-filter-hint").textContent = s.speedMbps
    ? `Uses your ${Math.round(s.speedMbps)} Mbps connection and each title's runtime.`
    : "Measure your speed under Connection first.";
  $("exclude-chips").replaceChildren(
    ...QUALITIES.map(([value, label]) => {
      const on = s.excludeResolutions.includes(value);
      const chip = h("button", { class: `toggle-chip${on ? " on" : ""}`, type: "button", ariaPressed: String(on) }, label);
      chip.onclick = () =>
        savePreferences((p) => {
          p.excludeResolutions = on ? p.excludeResolutions.filter((v) => v !== value) : [...p.excludeResolutions, value];
        });
      return chip;
    }),
  );
}
$<HTMLInputElement>("filter-by-speed").onchange = (e) =>
  savePreferences((p) => (p.filterBySpeed = (e.target as HTMLInputElement).checked));

// ---------- Stream display ----------

const DISPLAY_STYLES: Record<string, { label: string; hint: string; name: string; title: string }> = {
  compact: {
    label: "Compact",
    hint: "Resolution and size only. Easy to scan.",
    name: "Matzio\n4K",
    title: "18.4 GB · RD ⚡",
  },
  detailed: {
    label: "Detailed",
    hint: "Everything that sets streams apart.",
    name: "Matzio ⚡\n4K HDR",
    title: "Dune Part Two (2024) 2160p BluRay REMUX\nEnglish 5.1 · 18.4 GB · Real-Debrid cached",
  },
};

function renderDisplay() {
  const current = state.settings.displayStyle;
  $("display-choices").replaceChildren(
    ...Object.entries(DISPLAY_STYLES).map(([id, style]) => {
      const card = h(
        "button",
        { class: `glass choice${id === current ? " selected" : ""}`, type: "button" },
        h("b", {}, style.label),
        h("span", { class: "subtle small" }, style.hint),
      );
      card.onclick = () => savePreferences((p) => (p.displayStyle = id));
      return card;
    }),
  );
  const style = DISPLAY_STYLES[current] ?? DISPLAY_STYLES.detailed;
  $("display-preview").replaceChildren(
    h("div", { class: "preview-name" }, style.name),
    h("div", { class: "preview-title" }, style.title),
  );
}

// ---------- Updates ----------

function renderUpdate(status: UpdateStatus) {
  const text = $("update-text");
  const button = $<HTMLButtonElement>("update-button");
  text.title = "";
  button.disabled = false;
  button.textContent = "Check now";
  button.classList.remove("primary");
  button.classList.add("ghost");

  switch (status.state) {
    case "notChecked":
      text.textContent = "Not checked yet";
      break;
    case "checking":
      text.textContent = "Checking for updates…";
      button.disabled = true;
      break;
    case "upToDate":
      text.textContent = "You're up to date";
      break;
    case "noRelease":
      text.textContent = "No updates published yet";
      break;
    case "available":
      text.textContent = `Version ${status.version} is available`;
      button.textContent = "Install and restart";
      button.classList.replace("ghost", "primary");
      break;
    case "installing":
      text.textContent = `Installing ${status.version}…`;
      button.disabled = true;
      break;
    case "failed":
      text.textContent = "Couldn't check for updates. Check your internet connection and try again.";
      text.title = status.error;
      break;
  }
  button.dataset.state = status.state;
}

async function setUpUpdates() {
  const startup = $<HTMLInputElement>("start-with-windows");
  startup.checked = state.settings.startWithWindows;
  startup.addEventListener("change", async () => {
    try {
      state.settings = await api.setStartWithWindows(startup.checked);
    } catch (e) {
      startup.checked = !startup.checked;
      alert(`Could not change startup setting: ${e}`);
    }
  });

  const toggle = $<HTMLInputElement>("auto-update");
  toggle.checked = state.settings.autoUpdate;
  toggle.addEventListener("change", async () => {
    try {
      state.settings = await api.setAutoUpdate(toggle.checked);
    } catch (e) {
      toggle.checked = !toggle.checked;
      alert(`Could not save setting: ${e}`);
    }
  });

  renderUpdate(await api.updateStatus());
  await listen<UpdateStatus>("update-status", (event) => renderUpdate(event.payload));

  $("update-button").addEventListener("click", async () => {
    renderUpdate(await ($("update-button").dataset.state === "available" ? api.installUpdate() : api.checkForUpdates()));
  });
}

export async function startApp(rerunSetup: () => void) {
  $("version").textContent = `Version ${(await api.appInfo()).version}`;
  $("rerun-setup").onclick = rerunSetup;
  showStatus();
  onChange(() => {
    renderFilters();
    renderDisplay();
  });
  renderFilters();
  renderDisplay();
  go("home");
  setUpUpdates();
}

export function showApp() {
  app.hidden = false;
}
