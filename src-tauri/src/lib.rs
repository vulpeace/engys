mod commands;
mod custom_groups;
mod routing;
mod app_rules;
mod desktop;
mod instance;
mod login;
mod session;
mod core;
mod core_update;
mod error;
mod privilege;
mod processes;
mod profiles;
mod settings;
mod subscription;
mod subscription_refresh;
mod util;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, RunEvent};

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
    let handoff = args.iter().any(|a| a == "--elevated-handoff");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info })
                .build(),
        )
        .setup(move |app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            #[allow(unused_mut)]
            let Some(mut instance) = instance::Instance::acquire(&config_dir, background, handoff)? else {
                app.handle().exit(0);
                return Ok(());
            };
            #[cfg(windows)]
            if !background && !handoff && !privilege::is_elevated() {
                if let Ok(status) = login::status() {
                    if status.enabled && !status.needs_repair {
                        std::fs::write(config_dir.join("show-on-launch"), b"open")?;
                        // Release the lock before starting the elevated replacement.
                        drop(instance);
                        if let Err(error) = login::run() {
                            let _ = std::fs::remove_file(config_dir.join("show-on-launch"));
                            log::error!("Could not launch the elevated login task: {error}");
                            let Some(reacquired) = instance::Instance::acquire(&config_dir, false, false)? else {
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
            instance.listen(app.handle().clone())?;
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
            if foreground { let _ = std::fs::remove_file(foreground_request); }
            if !background || foreground { desktop::show(app.handle()); }
            if resume {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = core::start(handle).await {
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
            commands::profile_update_remote,
            commands::profile_set_update_interval,
            commands::profile_reset_update_interval,
            commands::profile_read,
            commands::profile_save,
            commands::profile_rename,
            commands::profile_delete,
            commands::profile_activate,
            commands::processes_list,
            commands::client_memory,
            commands::routing_get,
            commands::routing_save,
            commands::custom_groups_get,
            commands::custom_groups_save,
            commands::app_rules_get,
            commands::app_rules_save,
            commands::tun_permission,
            commands::tun_grant_permission,
            commands::core_check_update,
            commands::core_install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        // Never leave an orphaned core behind.
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
                state.core.lock().unwrap().shutdown_blocking();
            }
        }
    });
}
