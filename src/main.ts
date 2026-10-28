import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type AddonStatus =
  | { state: "running"; manifestUrl: string }
  | { state: "failed"; error: string };

type UpdateStatus =
  | { state: "notChecked" | "checking" | "upToDate" | "noRelease" }
  | { state: "available" | "installing"; version: string }
  | { state: "failed"; error: string };

type Settings = { autoUpdate: boolean };

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

// Windows paints Mica/Acrylic behind the page; elsewhere use a solid backdrop.
if (navigator.userAgent.includes("Windows")) {
  document.documentElement.classList.add("native-glass");
}

async function showAddon() {
  const status = await invoke<AddonStatus>("addon_status");
  const pill = $("status");

  if (status.state === "failed") {
    $("status-text").textContent = "Not running";
    pill.classList.add("bad");
    $("error-text").textContent = status.error;
    $("error").hidden = false;
    return;
  }

  $("status-text").textContent = "Running";
  pill.classList.add("good");
  $<HTMLInputElement>("manifest-url").value = status.manifestUrl;
  $("install").hidden = false;

  $("copy").addEventListener("click", async () => {
    await navigator.clipboard.writeText(status.manifestUrl);
    $("copy").textContent = "Copied";
    setTimeout(() => ($("copy").textContent = "Copy"), 1500);
  });
  $("install-button").addEventListener("click", () => {
    invoke("install_in_stremio").catch((e) => alert(`Could not open Stremio: ${e}`));
  });
}

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

async function showUpdates() {
  const { version } = await invoke<{ version: string }>("app_info");
  $("version").textContent = `Version ${version}`;

  const toggle = $<HTMLInputElement>("auto-update");
  toggle.checked = (await invoke<Settings>("get_settings")).autoUpdate;
  toggle.addEventListener("change", async () => {
    try {
      await invoke<Settings>("set_auto_update", { enabled: toggle.checked });
    } catch (e) {
      toggle.checked = !toggle.checked;
      alert(`Could not save setting: ${e}`);
    }
  });

  renderUpdate(await invoke<UpdateStatus>("update_status"));
  await listen<UpdateStatus>("update-status", (event) => renderUpdate(event.payload));

  $("update-button").addEventListener("click", async () => {
    const command = $("update-button").dataset.state === "available" ? "install_update" : "check_for_updates";
    renderUpdate(await invoke<UpdateStatus>(command));
  });
}

type DebridKeys = { realDebridKey: string | null; torboxKey: string | null };
type KeyCheck = { ok: boolean; username: string | null; premium: boolean; expires: string | null; error: string | null };

function describe(check: KeyCheck): string {
  if (!check.ok) return `✕ ${check.error ?? "Key didn't work"}`;
  const who = check.username ? ` as ${check.username}` : "";
  if (!check.premium) return `⚠ Connected${who}, but the account isn't premium`;
  const until = check.expires ? ` until ${new Date(check.expires).toLocaleDateString()}` : "";
  return `✓ Connected${who}, premium${until}`;
}

async function checkKey(service: "realdebrid" | "torbox", key: string, statusId: string) {
  const status = $(statusId);
  if (!key) {
    status.textContent = "";
    return;
  }
  status.textContent = "Checking…";
  status.textContent = describe(await invoke<KeyCheck>("check_debrid_key", { service, key }));
}

async function showDebrid() {
  const rd = $<HTMLInputElement>("rd-key");
  const tb = $<HTMLInputElement>("tb-key");
  const keys = await invoke<DebridKeys>("get_debrid_keys");
  rd.value = keys.realDebridKey ?? "";
  tb.value = keys.torboxKey ?? "";
  checkKey("realdebrid", rd.value, "rd-status");
  checkKey("torbox", tb.value, "tb-status");

  const save = $<HTMLButtonElement>("save-keys");
  save.addEventListener("click", async () => {
    save.disabled = true;
    try {
      const saved = await invoke<DebridKeys>("set_debrid_keys", { realDebridKey: rd.value, torboxKey: tb.value });
      rd.value = saved.realDebridKey ?? "";
      tb.value = saved.torboxKey ?? "";
      await Promise.all([checkKey("realdebrid", rd.value, "rd-status"), checkKey("torbox", tb.value, "tb-status")]);
    } catch (e) {
      alert(`Could not save keys: ${e}`);
    } finally {
      save.disabled = false;
    }
  });
}

showAddon();
showDebrid();
showUpdates();
