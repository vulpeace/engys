use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(target_os = "android")]
use tauri_plugin_deep_link::DeepLinkExt;

pub const MAX_LINK_BYTES: usize = 16 * 1024;

pub fn is_link(value: &str) -> bool {
    const PREFIX: &str = "engys://";
    value.len() <= MAX_LINK_BYTES
        && value
            .get(..PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
}

#[derive(Default)]
pub struct SubscriptionLinks {
    pending: Mutex<Vec<String>>,
    pub(crate) import: tokio::sync::Mutex<()>,
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
pub async fn pending_subscription_link(app: AppHandle) -> Option<String> {
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
    let next = links.pending.lock().unwrap().first().cloned();
    next
}

impl SubscriptionLinks {
    pub(crate) fn is_next(&self, link: &str) -> bool {
        self.pending.lock().unwrap().first().is_some_and(|next| next == link)
    }

    pub(crate) fn complete(&self, app: &AppHandle, link: &str) {
        let mut pending = self.pending.lock().unwrap();
        if pending.first().is_some_and(|next| next == link) {
            pending.remove(0);
            drop(pending);
            let _ = app.emit("subscription-links-pending", ());
        }
    }
}

#[tauri::command]
pub async fn dismiss_subscription_link(app: AppHandle, link: String) {
    let links = app.state::<SubscriptionLinks>();
    let _guard = links.import.lock().await;
    links.complete(&app, &link);
}
