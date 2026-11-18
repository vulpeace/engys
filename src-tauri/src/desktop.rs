use crate::{AppState, error::Result, session::Geometry};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WindowEvent};

struct TrayState {
    status: MenuItem<tauri::Wry>,
}

pub fn show(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
}

pub fn setup(app: &AppHandle) -> Result<()> {
    let status = MenuItem::with_id(app, "status", "Core stopped", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Mihomo Client", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &separator, &open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Mihomo Client — core stopped")
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
        let _ = tray.set_tooltip(Some(format!("Mihomo Client — {text}")));
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
    // Capture visibility before handling close so we only save visible window geometry.
    let save_geometry = !window.is_minimized().unwrap_or(false) && window.is_visible().unwrap_or(false);
    if let WindowEvent::CloseRequested { api, .. } = event {
        let close_to_tray = state.settings.lock().unwrap().minimize_to_tray
            && !state.exiting.load(std::sync::atomic::Ordering::SeqCst)
            && window.app_handle().tray_by_id("main").is_some();
        if close_to_tray {
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
