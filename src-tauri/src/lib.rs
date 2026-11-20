mod commands;
mod custom_groups;
mod startup_proxies;
mod routing;
mod app_rules;
#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
#[path = "desktop_android.rs"]
mod desktop;
#[cfg(not(target_os = "android"))]
mod instance;
mod login;
mod session;
mod core_config;
#[cfg(not(target_os = "android"))]
mod core;
#[cfg(target_os = "android")]
#[path = "core_android.rs"]
mod core;
#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod core_update;
#[cfg(target_os = "android")]
#[path = "core_update_android.rs"]
mod core_update;
mod error;
mod download_route;
mod privilege;
#[cfg(not(target_os = "android"))]
mod processes;
#[cfg(target_os = "android")]
#[path = "processes_android.rs"]
mod processes;
mod profiles;
mod settings;
mod subscription;
mod subscription_refresh;
mod subscription_links;
mod util;
#[cfg(windows)]
mod windows_webview;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;
#[cfg(not(target_os = "android"))]
use tauri::RunEvent;

use crate::core::CoreManager;
use crate::profiles::ProfileStore;
use crate::settings::Settings;

pub struct Paths {
    pub settings_file: PathBuf,
    pub app_rules_file: PathBuf,
    pub custom_groups_file: PathBuf,
    pub routing_file: PathBuf,
    pub profiles_dir: PathBuf,
    /// mihomo working directory (`-d`): runtime.yaml, GEO databases, cache.
    pub core_dir: PathBuf,
    /// App-managed core binary (downloaded from GitHub releases).
    pub core_bin_dir: PathBuf,
}

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    pub profiles: Mutex<ProfileStore>,
    pub core: Mutex<CoreManager>,
    pub app_rules: Mutex<app_rules::RulesFile>,
    pub custom_groups: Mutex<custom_groups::GroupsStore>,
    pub routing: Mutex<routing::Store>,
    pub session: Mutex<session::Session>,
    pub core_operation: tokio::sync::Mutex<()>,
    pub subscription_refresh: subscription_refresh::RefreshState,
    pub exiting: std::sync::atomic::AtomicBool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = login::maintenance(&args) { std::process::exit(code); }
    let background = args.iter().any(|a| a == "--autostart");
    #[cfg(windows)]
    if !windows_webview::ensure_available(!background) {
        std::process::exit(1);
    }
    #[cfg(not(target_os = "android"))]
    let handoff = args.iter().any(|a| a == "--elevated-handoff");
    #[cfg(not(target_os = "android"))]
    let launch_link = args.get(1).filter(|arg| args.len() == 2 && subscription_links::is_link(arg));
    #[cfg(not(target_os = "android"))]
    let launch_link = launch_link.cloned();
    let builder = tauri::Builder::default()
        .manage(subscription_links::SubscriptionLinks::default())
        .plugin(tauri_plugin_deep_link::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_android_vpn::init());
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
                .build(),
        )
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            #[cfg(target_os = "linux")]
            {
                util::ensure_private_dir(&config_dir)?;
                util::ensure_private_dir(&data_dir)?;
            }
            #[allow(unused_mut)]
            #[cfg(not(target_os = "android"))]
            let Some(mut instance) = instance::Instance::acquire(&config_dir, background, handoff, launch_link.as_deref())? else {
                app.handle().exit(0);
                return Ok(());
            };
            #[cfg(windows)]
            if !background && !handoff && !privilege::is_elevated() {
                if let Ok(status) = login::status() {
                    if status.enabled && !status.needs_repair {
                        util::write_atomic(
                            &config_dir.join("show-on-launch"),
                            launch_link.as_deref().unwrap_or("open").as_bytes(),
                        )?;
                        // Release the lock before starting the elevated replacement.
                        drop(instance);
                        if let Err(error) = login::run() {
                            let _ = std::fs::remove_file(config_dir.join("show-on-launch"));
                            log::error!("Could not launch the elevated login task: {error}");
                            let Some(reacquired) = instance::Instance::acquire(&config_dir, false, false, launch_link.as_deref())? else {
                                app.handle().exit(0);
                                return Ok(());
                            };
                            instance = reacquired;
                        } else {
                            app.handle().exit(0);
                            return Ok(());
                        }
                    }
                }
            }
            #[cfg(not(target_os = "android"))]
            instance.listen(app.handle().clone())?;
            #[cfg(not(target_os = "android"))]
            app.manage(instance);
            let paths = Paths {
                settings_file: config_dir.join("settings.json"),
                app_rules_file: config_dir.join("app-rules.json"),
                custom_groups_file: config_dir.join("custom-groups.json"),
                routing_file: config_dir.join("routing.json"),
                profiles_dir: data_dir.join("profiles"),
                core_dir: data_dir.join("core"),
                core_bin_dir: data_dir.join("core-bin"),
            };
            std::fs::create_dir_all(&config_dir)?;
            std::fs::create_dir_all(&paths.profiles_dir)?;
            std::fs::create_dir_all(&paths.core_dir)?;
            log::info!("config: {}, data: {}", config_dir.display(), data_dir.display());

            let session = session::Session::load(&config_dir.join("session.json"));
            #[cfg(not(target_os = "android"))]
            let resume = session.core_running;
            app.manage(AppState {
                session: Mutex::new(session),
                app_rules: Mutex::new(app_rules::load(&paths.app_rules_file)?),
                custom_groups: Mutex::new(custom_groups::GroupsStore::load(&paths.custom_groups_file)?),
                routing: Mutex::new(routing::Store::load(&paths.routing_file)?),
                core_operation: tokio::sync::Mutex::new(()),
                subscription_refresh: subscription_refresh::RefreshState::new(),
                exiting: std::sync::atomic::AtomicBool::new(false),
                settings: Mutex::new(Settings::load(&paths.settings_file)),
                profiles: Mutex::new(ProfileStore::load(paths.profiles_dir.clone())),
                core: Mutex::new(CoreManager::default()),
                paths,
            });
            desktop::setup(app.handle())?;
            subscription_refresh::start_scheduler(app.handle().clone());
            let foreground_request = config_dir.join("show-on-launch");
            let foreground = foreground_request.exists();
            if foreground {
                #[cfg(windows)]
                if let Ok(link) = std::fs::read_to_string(&foreground_request) {
                    subscription_links::enqueue(app.handle(), link);
                }
                let _ = std::fs::remove_file(foreground_request);
            }
            #[cfg(not(target_os = "android"))]
            if let Some(link) = launch_link.clone() {
                subscription_links::enqueue(app.handle(), link);
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                #[cfg(target_os = "linux")]
                if app.env().appimage.is_some()
                    && matches!(app.deep_link().is_registered("engys"), Ok(false))
                {
                    if let Err(error) = app.deep_link().register_all() {
                        log::warn!("Could not register subscription links: {error}");
                    }
                }
                #[cfg(target_os = "android")]
                {
                    let handle = app.handle().clone();
                    app.deep_link().on_open_url(move |event| {
                        for url in event.urls() {
                            subscription_links::enqueue(&handle, url.to_string());
                        }
                    });
                }
            }
            if !background || foreground { desktop::show(app.handle()); }
            #[cfg(target_os = "android")]
            android::initialize(app.handle());
            #[cfg(not(target_os = "android"))]
            if resume {
                let handle = app.handle().clone();
                let interactive = !background || foreground;
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = core::start(handle, interactive).await {
                        log::error!("Could not restore core state: {error}");
                    }
                });
            }
            Ok(())
        })
        .on_window_event(desktop::window_event)
        .invoke_handler(tauri::generate_handler![
            commands::core_status,
            commands::core_start,
            commands::core_stop,
            commands::core_logs,
            commands::core_reload,
            commands::core_binary_info,
            commands::work_dir,
            commands::login_status,
            commands::login_set,
            commands::settings_get,
            commands::settings_inherited,
            commands::settings_save,
            commands::profiles_list,
            commands::profile_create_inline,
            commands::profile_import_file,
            commands::profile_add_remote,
            subscription_links::pending_subscription_link,
            subscription_links::dismiss_subscription_link,
            commands::profile_update_remote,
            commands::profile_resolve_refresh,
            commands::pending_subscription_reviews,
            commands::resolve_pending_subscription,
            commands::profile_set_update_interval,
            commands::profile_reset_update_interval,
            commands::profile_read,
            commands::profile_read_final,
            commands::profile_save,
            commands::profile_duplicate,
            commands::profile_rename,
            commands::profile_delete,
            commands::profile_activate,
            commands::processes_list,
            commands::installed_app_icon,
            commands::platform_info,
            commands::android_setup_status,
            commands::android_request_battery,
            commands::android_request_notifications,
            commands::android_vpn_active,
            commands::client_memory,
            commands::android_memory,
            commands::routing_get,
            commands::routing_save,
            startup_proxies::startup_proxies_get,
            startup_proxies::startup_proxies_save,
            commands::custom_groups_get,
            commands::custom_groups_save,
            commands::app_rules_get,
            commands::app_rules_save,
            commands::tun_permission,
            commands::tun_grant_permission,
            commands::core_check_update,
            commands::core_install_update,
            commands::core_set_download_without_proxy,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_handle, _event| {
        // Closing the last window can leave the tray and core running.
        #[cfg(target_os = "linux")]
        if let RunEvent::ExitRequested { code: None, api, .. } = &_event {
            if desktop::close_to_tray(_handle) {
                api.prevent_exit();
            }
        }
        // Never leave an orphaned core behind.
        #[cfg(not(target_os = "android"))]
        if let RunEvent::Exit = _event {
            if let Some(state) = _handle.try_state::<AppState>() {
                state.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
                state.core.lock().unwrap().shutdown_blocking();
            }
        }
    });
}
