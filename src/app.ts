// The app after setup: a sidebar of sections.
import { api, type UpdateStatus } from "./api";
import { listen } from "@tauri-apps/api/event";
import { exportSettings, importSettings } from "./backup";
import { renderFilters } from "./filters";
import { renderSources } from "./sources";
import { renderSubtitles } from "./subtitles";
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

const renderFilterPage = () => {
  renderFilters($("filter-cards"));
  renderSources($("source-cards"));
  renderSubtitles($("subtitle-cards"));
};

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
  $("export-settings").onclick = exportSettings;
  $("import-settings").onclick = importSettings;
  showStatus();
  onChange(() => {
    renderFilterPage();
    renderDisplay();
  });
  renderFilterPage();
  renderDisplay();
  go("home");
  setUpUpdates();
}

export function showApp() {
  app.hidden = false;
}
