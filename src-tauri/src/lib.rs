//! Tray app shell. It owns no window while idle: the settings window is created
//! when opened and destroyed when closed, so only the tray icon and the small
//! addon server stay resident.

mod settings;
mod updates;

use std::sync::{Arc, RwLock};

use matzio_addon::{
    base_url, bind, debrid::Service, labels, manifest_url, serve, streams, subtitles::Provider, AppState, Config, Endpoints,
    SharedConfig, DEFAULT_PORT,
};
use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, State, Theme, WebviewUrl, WebviewWindowBuilder,
};
#[cfg(windows)]
use tauri::window::{Effect, EffectsBuilder};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt;

use settings::{Preferences, Settings, SettingsStore, SubtitleKeys};
use updates::{UpdateStatus, Updates};

const SETTINGS_LABEL: &str = "settings";

/// Whether the addon server came up, as shown in the settings window.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum AddonStatus {
    Running { manifest_url: String },
    Failed { error: String },
}

impl AddonStatus {
    fn stremio_link(&self) -> Option<String> {
        match self {
            // Stremio installs addons from stremio:// links to the manifest.
            AddonStatus::Running { manifest_url } => {
                Some(manifest_url.replacen("http://", "stremio://", 1))
            }
            AddonStatus::Failed { .. } => None,
        }
    }
}

#[tauri::command]
fn addon_status(status: State<AddonStatus>) -> AddonStatus {
    status.inner().clone()
}

#[tauri::command]
fn install_in_stremio(app: AppHandle, status: State<AddonStatus>) -> Result<(), String> {
    let link = status.stremio_link().ok_or("The addon is not running")?;
    app.opener().open_url(link, None::<&str>).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct AppInfo {
    version: String,
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    AppInfo { version: app.package_info().version.to_string() }
}

#[tauri::command]
fn get_settings(store: State<SettingsStore>) -> Settings {
    store.get()
}

/// Makes Windows' sign-in startup entry match the setting. Debug builds never
/// register themselves, so `tauri dev` doesn't leave a startup entry behind.
fn apply_start_with_windows(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let launcher = app.autolaunch();
    let registered = launcher.is_enabled().map_err(|e| e.to_string())?;
    match (enabled, registered) {
        (true, false) => launcher.enable().map_err(|e| e.to_string()),
        (false, true) => launcher.disable().map_err(|e| e.to_string()),
        _ => Ok(()),
    }
}

#[tauri::command]
fn set_start_with_windows(app: AppHandle, store: State<SettingsStore>, enabled: bool) -> Result<Settings, String> {
    apply_start_with_windows(&app, enabled)?;
    store.update(|s| s.start_with_windows = enabled)
}

#[tauri::command]
fn set_auto_update(store: State<SettingsStore>, enabled: bool) -> Result<Settings, String> {
    store.update(|s| s.auto_update = enabled)
}

#[tauri::command]
fn set_setup_complete(store: State<SettingsStore>, complete: bool) -> Result<Settings, String> {
    store.update(|s| s.setup_complete = complete)
}

#[tauri::command]
fn save_preferences(
    store: State<SettingsStore>,
    config: State<SharedConfig>,
    preferences: Preferences,
) -> Result<Settings, String> {
    let saved = store.update(|s| s.preferences = preferences)?;
    *config.write().unwrap() = config_from(&saved);
    Ok(saved)
}

#[tauri::command]
fn update_status(updates: State<Updates>) -> UpdateStatus {
    updates.status()
}

#[tauri::command]
async fn check_for_updates(app: AppHandle) -> UpdateStatus {
    // A manual check never installs on its own; the window offers an Install button.
    updates::check(&app, false).await
}

#[tauri::command]
async fn install_update(app: AppHandle) -> UpdateStatus {
    updates::install_pending(&app).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DebridKeys {
    real_debrid_key: Option<String>,
    torbox_key: Option<String>,
}

impl From<&Settings> for DebridKeys {
    fn from(s: &Settings) -> Self {
        Self { real_debrid_key: s.real_debrid_key.clone(), torbox_key: s.torbox_key.clone() }
    }
}

fn config_from(settings: &Settings) -> Config {
    Config::new(settings.real_debrid_key.clone(), settings.torbox_key.clone())
        .with_subtitle_keys(settings.subtitle_keys.clone())
        .with_preferences(settings.preferences.clone())
}

#[tauri::command]
fn get_debrid_keys(store: State<SettingsStore>) -> DebridKeys {
    DebridKeys::from(&store.get())
}

#[tauri::command]
fn set_debrid_keys(
    store: State<SettingsStore>,
    config: State<SharedConfig>,
    real_debrid_key: Option<String>,
    torbox_key: Option<String>,
) -> Result<DebridKeys, String> {
    let clean = |k: Option<String>| k.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
    let saved = store.update(|s| {
        s.real_debrid_key = clean(real_debrid_key);
        s.torbox_key = clean(torbox_key);
    })?;
    // The running addon picks the new keys up on its next request.
    *config.write().unwrap() = config_from(&saved);
    Ok(DebridKeys::from(&saved))
}

#[derive(Serialize)]
struct KeyCheck {
    ok: bool,
    username: Option<String>,
    premium: bool,
    expires: Option<String>,
    error: Option<String>,
}

#[tauri::command]
async fn check_debrid_key(addon: State<'_, AppState>, service: String, key: String) -> Result<KeyCheck, String> {
    let service = Service::from_slug(&service).ok_or("service must be \"realdebrid\" or \"torbox\"")?;
    Ok(match addon.check_key(service, &key).await {
        Ok(a) => KeyCheck { ok: true, username: a.username, premium: a.premium, expires: a.expires, error: None },
        Err(e) => KeyCheck { ok: false, username: None, premium: false, expires: None, error: Some(e.to_string()) },
    })
}

#[derive(Serialize)]
struct LabelPreview {
    name: String,
    title: String,
}

/// How a made-up stream looks in Stremio's picker with these display settings.
#[tauri::command]
fn preview_stream_label(display_style: String, name_template: String, title_template: String) -> LabelPreview {
    let prefs = Preferences { display_style, name_template, title_template, ..Preferences::default() };
    let s = streams::stream_for("", Service::RealDebrid, true, &labels::sample(), None, &prefs);
    LabelPreview {
        name: s["name"].as_str().unwrap_or_default().to_owned(),
        title: s["title"].as_str().unwrap_or_default().to_owned(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LabelInfo {
    variables: Vec<(&'static str, &'static str)>,
    default_name: &'static str,
    default_title: &'static str,
}

#[tauri::command]
fn stream_label_info() -> LabelInfo {
    LabelInfo { variables: labels::VARIABLES.to_vec(), default_name: labels::DEFAULT_NAME, default_title: labels::DEFAULT_TITLE }
}

#[tauri::command]
fn get_subtitle_keys(store: State<SettingsStore>) -> SubtitleKeys {
    store.get().subtitle_keys
}

#[tauri::command]
fn set_subtitle_keys(
    store: State<SettingsStore>,
    config: State<SharedConfig>,
    keys: SubtitleKeys,
) -> Result<SubtitleKeys, String> {
    let saved = store.update(|s| s.subtitle_keys = keys.cleaned())?;
    *config.write().unwrap() = config_from(&saved);
    Ok(saved.subtitle_keys)
}

#[derive(Serialize)]
struct SubtitleCheck {
    ok: bool,
    message: String,
}

#[tauri::command]
async fn check_subtitle_key(addon: State<'_, AppState>, provider: String, keys: SubtitleKeys) -> Result<SubtitleCheck, String> {
    let provider = Provider::from_slug(&provider).ok_or("provider must be \"opensubtitles\" or \"subdl\"")?;
    Ok(match addon.check_subtitle_key(provider, keys).await {
        Ok(message) => SubtitleCheck { ok: true, message },
        Err(e) => SubtitleCheck { ok: false, message: e.to_string() },
    })
}

fn start_addon(config: SharedConfig) -> (AppState, AddonStatus) {
    match tauri::async_runtime::block_on(bind(DEFAULT_PORT)) {
        Ok(listener) => {
            let addr = listener.local_addr().expect("bound socket has an address");
            let state = AppState::new(config, Endpoints::default(), &base_url(addr));
            let url = manifest_url(addr);
            let server_state = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = serve(listener, server_state).await {
                    eprintln!("addon server stopped: {e}");
                }
            });
            (state, AddonStatus::Running { manifest_url: url })
        }
        Err(e) => {
            // Still build the state so keys can be checked and saved from the window.
            let state = AppState::new(config, Endpoints::default(), &format!("http://127.0.0.1:{DEFAULT_PORT}"));
            (state, AddonStatus::Failed { error: format!("Could not use port {DEFAULT_PORT}: {e}") })
        }
    }
}

fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
        .title("Matzio")
        .inner_size(940.0, 640.0)
        .min_inner_size(760.0, 540.0)
        .theme(Some(Theme::Dark))
        .center();
    // Real frosted glass behind the page: Mica on Windows 11, Acrylic on Windows 10.
    #[cfg(windows)]
    let builder = builder.transparent(true).effects(
        EffectsBuilder::new().effects([Effect::Mica, Effect::Acrylic]).build(),
    );
    let built = builder.build();
    if let Err(e) = built {
        eprintln!("could not open settings window: {e}");
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Matzio", true, None::<&str>)?;
    let install = MenuItem::with_id(app, "install", "Install in Stremio", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Matzio", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &install, &update, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("bundle has an icon").clone())
        .tooltip("Matzio")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_settings(app),
            "install" => {
                let _ = install_in_stremio(app.clone(), app.state());
            }
            "update" => {
                // Show progress in the window, then follow the auto-update setting.
                open_settings(app);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let auto = app.state::<SettingsStore>().get().auto_update;
                    updates::check(&app, auto).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                open_settings(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        // A second launch just brings up the running instance's settings.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| open_settings(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            addon_status,
            install_in_stremio,
            app_info,
            get_settings,
            set_auto_update,
            set_setup_complete,
            save_preferences,
            update_status,
            check_for_updates,
            install_update,
            get_debrid_keys,
            set_debrid_keys,
            check_debrid_key,
            get_subtitle_keys,
            set_subtitle_keys,
            preview_stream_label,
            stream_label_info,
            check_subtitle_key,
            set_start_with_windows,
        ])
        .setup(|app| {
            let store = SettingsStore::load(app.handle());
            let config: SharedConfig = Arc::new(RwLock::new(config_from(&store.get())));
            let (addon, status) = start_addon(config.clone());
            let failed = matches!(status, AddonStatus::Failed { .. });
            app.manage(status);
            let first_run = !store.get().setup_complete;
            if let Err(e) = apply_start_with_windows(app.handle(), store.get().start_with_windows) {
                eprintln!("could not update the Windows startup entry: {e}");
            }
            app.manage(store);
            app.manage(config);
            app.manage(addon);
            app.manage(Updates::default());
            updates::start_background_checks(app.handle().clone());
            build_tray(app.handle())?;
            if failed || first_run {
                // Greet a new install with setup, and surface a startup failure instead of
                // failing silently in the tray.
                open_settings(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Matzio")
        .run(|_app, event| {
            // Closing the settings window must not quit the app; only "Quit" does.
            if let RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addon_status_fields_are_camel_case() {
        let running = AddonStatus::Running { manifest_url: "http://127.0.0.1:7777/manifest.json".into() };
        assert_eq!(
            serde_json::to_value(running).unwrap(),
            serde_json::json!({ "state": "running", "manifestUrl": "http://127.0.0.1:7777/manifest.json" })
        );
    }
}
