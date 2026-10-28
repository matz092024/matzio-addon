import { invoke } from "@tauri-apps/api/core";

type AddonStatus =
  | { state: "running"; manifestUrl: string }
  | { state: "failed"; error: string };

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

async function init() {
  const status = await invoke<AddonStatus>("addon_status");
  const statusEl = $("status");

  if (status.state === "failed") {
    statusEl.textContent = "Not running";
    statusEl.classList.add("bad");
    $("error-text").textContent = status.error;
    $("error").hidden = false;
    return;
  }

  statusEl.textContent = "Running";
  statusEl.classList.add("good");
  const urlInput = $<HTMLInputElement>("manifest-url");
  urlInput.value = status.manifestUrl;
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

init();
