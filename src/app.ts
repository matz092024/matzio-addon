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

const DISPLAY_STYLES: Record<string, { label: string; hint: string }> = {
  compact: { label: "Compact", hint: "Resolution and size only. Easy to scan." },
  detailed: { label: "Detailed", hint: "Everything that sets streams apart." },
  custom: { label: "Custom", hint: "Your own layout, built from the details you pick." },
};

let labelInfo: Awaited<ReturnType<typeof api.streamLabelInfo>> | null = null;
// Templates being typed, so the preview can follow before they're saved.
let drafts: { name: string; title: string } | null = null;

async function showPreview() {
  const s = state.settings;
  const name = drafts?.name ?? s.nameTemplate;
  const title = drafts?.title ?? s.titleTemplate;
  const label = await api.previewStreamLabel(s.displayStyle, name, title).catch(() => null);
  if (!label) return;
  $("display-preview").replaceChildren(
    h("div", { class: "preview-name" }, label.name),
    h("div", { class: "preview-title" }, label.title),
  );
}

// The editor stays in place while its text matches what's saved, so saving doesn't
// take the cursor out of the box being typed in.
let editor: { el: HTMLElement; matches: () => boolean } | null = null;

function templateEditor() {
  const s = state.settings;
  let last: HTMLTextAreaElement | null = null;
  const area = (key: "name" | "title", label: string, value: string, rows: number) => {
    const input = h("textarea", { value: drafts?.[key] ?? value, rows, spellcheck: false, ariaLabel: label });
    input.onfocus = () => (last = input);
    input.oninput = () => {
      drafts = { name: nameBox.value, title: titleBox.value };
      showPreview();
    };
    input.onchange = save;
    return { input, field: h("label", { class: "words-field" }, h("span", { class: "subtle small" }, label), input) };
  };
  const { input: nameBox, field: nameField } = area("name", "Left column (the name)", s.nameTemplate, 2);
  const { input: titleBox, field: titleField } = area("title", "Right column (the description)", s.titleTemplate, 4);
  function save() {
    const [nameTemplate, titleTemplate] = [nameBox.value, titleBox.value];
    if (nameTemplate === s.nameTemplate && titleTemplate === s.titleTemplate) return;
    drafts = null;
    savePreferences((p) => Object.assign(p, { nameTemplate, titleTemplate }));
  }

  const chips = (labelInfo?.variables ?? []).map(([name, hint]) => {
    const chip = h("button", { class: "toggle-chip", type: "button", title: hint }, `{${name}}`);
    // Keep the text box focused, so the variable goes where the cursor was.
    chip.onmousedown = (e) => e.preventDefault();
    chip.onclick = () => {
      const box = last ?? titleBox;
      box.setRangeText(`{${name}}`, box.selectionStart, box.selectionEnd, "end");
      box.focus();
      box.dispatchEvent(new Event("input"));
      save();
    };
    return chip;
  });

  const reset = h("button", { class: "link", type: "button" }, "Back to the default");
  reset.onclick = () => {
    if (!labelInfo) return;
    drafts = null;
    savePreferences((p) => Object.assign(p, { nameTemplate: labelInfo!.defaultName, titleTemplate: labelInfo!.defaultTitle }));
  };

  const el = h(
    "div",
    { class: "glass check" },
    h("div", { class: "row" }, h("div", { class: "label" }, "Your layout"), reset),
    h(
      "div",
      { class: "check-body" },
      nameField,
      titleField,
      h("div", { class: "subtle small" }, "Click a detail to add it where the cursor is. Put a part in [brackets] to leave it out when a detail inside is unknown, like [👤 {seeders}]. Each line shows as its own line in Stremio."),
      h("div", { class: "chips" }, ...chips),
    ),
  );
  const matches = () => nameBox.value === state.settings.nameTemplate && titleBox.value === state.settings.titleTemplate;
  return { el, matches };
}

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
  if (current !== "custom") editor = null;
  else if (!editor?.matches()) editor = templateEditor();
  // Re-inserting the same element would still take focus away.
  const box = $("display-editor");
  if (box.firstChild !== (editor?.el ?? null)) box.replaceChildren(...(editor ? [editor.el] : []));
  showPreview();
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
  // Before anything renders the display page, so its editor has the list of details.
  labelInfo = await api.streamLabelInfo().catch(() => null);
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
