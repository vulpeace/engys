use crate::{AppState, error::Result, session::Geometry};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WindowEvent};

struct TrayState {
    status: MenuItem<tauri::Wry>,
}

pub fn show(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "linux")]
        if handle.get_webview_window("main").is_none() {
            let Some(config) = handle
                .config()
                .app
                .windows
                .iter()
                .find(|config| config.label == "main")
            else {
                log::error!("Missing main window configuration");
                return;
            };
            if let Err(error) = tauri::WebviewWindowBuilder::from_config(&handle, config)
                .and_then(|builder| builder.build())
            {
                log::error!("Could not reopen the main window: {error}");
                return;
            }
            restore_geometry(&handle);
        }
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit("main-window-shown", ());
        }
    });
}

pub fn setup(app: &AppHandle) -> Result<()> {
    let status = MenuItem::with_id(app, "status", "Core stopped", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open engys", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &separator, &open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("engys — core stopped")
        .show_menu_on_left_click(!cfg!(windows))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } | TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayState { status });
    restore_geometry(app);
    Ok(())
}

pub fn update(app: &AppHandle, running: bool, error: Option<&str>) {
    let text = if error.is_some() {
        "Core error — open app for details"
    } else if running {
        "Core running"
    } else {
        "Core stopped"
    };
    if let Some(state) = app.try_state::<TrayState>() {
        let _ = state.status.set_text(text);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("engys — {text}")));
    }
}

fn restore_geometry(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let saved = app.state::<AppState>().session.lock().unwrap().window.clone();
    let Some(g) = saved else { return };
    if g.width < 100 || g.height < 100 {
        return;
    }
    // Only restore a position if the title bar is still reachable on a monitor.
    let reachable = window.available_monitors().unwrap_or_default().iter().any(|m| {
        let p = m.position();
        let s = m.size();
        g.x as i64 + 100 > p.x as i64
            && (g.x as i64) < p.x as i64 + s.width as i64 - 100
            && g.y >= p.y
            && (g.y as i64) < p.y as i64 + s.height as i64 - 40
    });
    if reachable {
        let _ = window.set_size(PhysicalSize::new(g.width, g.height));
        let _ = window.set_position(PhysicalPosition::new(g.x, g.y));
    }
    if g.maximized {
        let _ = window.maximize();
    }
}

pub fn close_to_tray(app: &AppHandle) -> bool {
    app.try_state::<AppState>().is_some_and(|state| {
        state.settings.lock().unwrap().minimize_to_tray
            && !state.exiting.load(std::sync::atomic::Ordering::SeqCst)
            && app.tray_by_id("main").is_some()
    })
}

pub fn window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main"
        || !matches!(
            event,
            WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::CloseRequested { .. }
        )
    {
        return;
    }
    let Some(state) = window.app_handle().try_state::<AppState>() else {
        return;
    };
    // Capture visibility before a platform hides or closes the window.
    let save_geometry = !window.is_minimized().unwrap_or(false) && window.is_visible().unwrap_or(false);
    #[cfg(not(target_os = "linux"))]
    if let WindowEvent::CloseRequested { api, .. } = event {
        if close_to_tray(window.app_handle()) {
            api.prevent_close();
            if let Err(error) = window.hide() {
                log::warn!("Could not close to tray: {error}");
            }
        }
    }
    if !save_geometry {
        return;
    }
    let mut session = state.session.lock().unwrap();
    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        if let Some(g) = session.window.as_mut() {
            g.maximized = true;
        }
    } else if let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) {
        if size.width > 0 && size.height > 0 {
            session.window = Some(Geometry {
                x: pos.x,
                y: pos.y,
                width: size.width,
                height: size.height,
                maximized,
            });
        }
    }
    if let Err(e) = session.save() {
        log::warn!("Could not save window state: {e}");
    }
}
