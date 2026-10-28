import { startApp, showApp } from "./app";
import { load, state } from "./state";
import { mountWidgets } from "./widgets";
import { openWizard } from "./wizard";

// Windows paints Mica/Acrylic behind the page; elsewhere use a solid backdrop.
if (navigator.userAgent.includes("Windows")) {
  document.documentElement.classList.add("native-glass");
}

async function start() {
  await load();
  mountWidgets();
  const rerun = () => openWizard(showApp);
  await startApp(rerun);
  if (state.settings.setupComplete) showApp();
  else rerun();
}

start();
