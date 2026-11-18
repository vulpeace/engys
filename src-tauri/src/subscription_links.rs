use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(target_os = "android")]
use tauri_plugin_deep_link::DeepLinkExt;

pub const MAX_LINK_BYTES: usize = 16 * 1024;

pub fn is_link(value: &str) -> bool {
    const PREFIX: &str = "mihomo-client://";
    value.len() <= MAX_LINK_BYTES
        && value
            .get(..PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
}

#[derive(Default)]
pub struct SubscriptionLinks {
    pending: Mutex<Vec<String>>,
    #[cfg(target_os = "android")]
    read_initial: std::sync::atomic::AtomicBool,
}

pub fn enqueue(app: &AppHandle, link: String) {
    if !is_link(&link) {
        return;
    }
    let links = app.state::<SubscriptionLinks>();
    let mut pending = links.pending.lock().unwrap();
    if pending.len() < 16 && !pending.contains(&link) {
        pending.push(link);
        drop(pending);
        let _ = app.emit("subscription-links-pending", ());
    }
}

#[tauri::command]
pub async fn take_subscription_links(app: AppHandle) -> Vec<String> {
    let links = app.state::<SubscriptionLinks>();
    #[cfg(target_os = "android")]
    if !links
        .read_initial
        .swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        if let Ok(Some(urls)) = app.deep_link().get_current() {
            for url in urls {
                enqueue(&app, url.to_string());
            }
        }
    }
    let pending = std::mem::take(&mut *links.pending.lock().unwrap());
    pending
}
