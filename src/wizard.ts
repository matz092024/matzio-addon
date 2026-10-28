// First-run setup: a few full-window steps, then hand over to the app.
import { api } from "./api";
import { h, state } from "./state";

const wizard = document.getElementById("wizard")!;
const steps = [...wizard.querySelectorAll<HTMLElement>(".step")];
const dots = document.getElementById("step-dots")!;
const back = document.getElementById("wizard-back") as HTMLButtonElement;
const next = document.getElementById("wizard-next") as HTMLButtonElement;
const skip = document.getElementById("wizard-skip") as HTMLButtonElement;

let current = 0;
let onDone = () => {};

function show(index: number) {
  current = index;
  steps.forEach((step, i) => step.classList.toggle("active", i === index));
  dots.replaceChildren(...steps.map((_, i) => h("span", { class: i < index ? "done" : i === index ? "on" : "" })));
  const first = index === 0;
  const last = index === steps.length - 1;
  // The welcome step has its own big button.
  back.style.visibility = first ? "hidden" : "visible";
  next.style.visibility = first ? "hidden" : "visible";
  skip.hidden = last;
  next.textContent = last ? "Finish" : "Continue";
}

async function finish() {
  state.settings = await api.setSetupComplete(true);
  wizard.hidden = true;
  onDone();
}

back.onclick = () => current > 0 && show(current - 1);
next.onclick = () => (current === steps.length - 1 ? finish() : show(current + 1));
skip.onclick = finish;
wizard.querySelectorAll<HTMLElement>("[data-next]").forEach((b) => (b.onclick = () => show(current + 1)));

export function openWizard(done: () => void) {
  onDone = done;
  document.getElementById("app")!.hidden = true;
  wizard.hidden = false;
  show(0);
}
