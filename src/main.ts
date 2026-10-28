import { startApp, showApp } from "./app";
import { load, state } from "./state";
import { mountWidgets } from "./widgets";
import { openWizard } from "./wizard";

async function start() {
  await load();
  mountWidgets();
  const rerun = () => openWizard(showApp);
  await startApp(rerun);
  if (state.settings.setupComplete) showApp();
  else rerun();
}

start();
