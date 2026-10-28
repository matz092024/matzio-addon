//! Update checks against the signed `latest.json` published with each GitHub
//! release. One small HTTP request every few hours, so idle cost stays tiny.

use std::{sync::Mutex, time::Duration};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use crate::settings::SettingsStore;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    NotChecked,
    Checking,
    UpToDate,
    /// The update server answered but has no release yet (e.g. a 404 before the first one).
    NoRelease,
    Available { version: String },
    Installing { version: String },
    Failed { error: String },
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
}

impl Default for Updates {
    fn default() -> Self {
        Self { status: Mutex::new(UpdateStatus::NotChecked), pending: Mutex::new(None) }
    }
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }
}

fn set_status(app: &AppHandle, status: UpdateStatus) {
    *app.state::<Updates>().status.lock().unwrap() = status.clone();
    // Only reaches the settings window if it is open.
    let _ = app.emit("update-status", status);
}

/// Checks for an update. When `install` is true, an update found is installed
/// straight away; on Windows the installer then closes and restarts the app.
pub async fn check(app: &AppHandle, install: bool) -> UpdateStatus {
    if matches!(app.state::<Updates>().status(), UpdateStatus::Checking | UpdateStatus::Installing { .. }) {
        return app.state::<Updates>().status();
    }
    set_status(app, UpdateStatus::Checking);
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let status = match found {
        Ok(None) => UpdateStatus::UpToDate,
        Ok(Some(update)) => {
            let version = update.version.clone();
            *app.state::<Updates>().pending.lock().unwrap() = Some(update);
            if install {
                return install_pending(app).await;
            }
            UpdateStatus::Available { version }
        }
        Err(UpdaterError::ReleaseNotFound) => UpdateStatus::NoRelease,
        Err(e) => UpdateStatus::Failed { error: e.to_string() },
    };
    set_status(app, status.clone());
    status
}

/// Installs the update found by the last check.
pub async fn install_pending(app: &AppHandle) -> UpdateStatus {
    let Some(update) = app.state::<Updates>().pending.lock().unwrap().take() else {
        return app.state::<Updates>().status();
    };
    set_status(app, UpdateStatus::Installing { version: update.version.clone() });
    match update.download_and_install(|_, _| {}, || {}).await {
        // Windows exits during install; other platforms need an explicit restart.
        Ok(()) => app.restart(),
        Err(e) => {
            let status = UpdateStatus::Failed { error: e.to_string() };
            set_status(app, status.clone());
            status
        }
    }
}

/// Background loop: first check shortly after start, then every few hours.
pub fn start_background_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            let auto = app.state::<SettingsStore>().get().auto_update;
            check(&app, auto).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}
