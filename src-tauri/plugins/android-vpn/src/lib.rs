use serde::{Serialize, de::DeserializeOwned};
use tauri::{Manager, Runtime, plugin::{Builder, TauriPlugin}};
#[cfg(target_os = "android")]
pub struct Vpn<R: Runtime>(tauri::plugin::PluginHandle<R>);
#[cfg(target_os = "android")]
impl<R: Runtime> Vpn<R> {
    pub fn call<T: DeserializeOwned>(&self, command: &str, payload: impl Serialize) -> Result<T, String> {
        self.0.run_mobile_plugin(command, payload).map_err(|e| e.to_string())
    }
}
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("android-vpn").setup(|app, api| {
        #[cfg(target_os = "android")]
        app.manage(Vpn(api.register_android_plugin("dev.mihomo.vpn", "VpnPlugin")?));
        Ok(())
    }).build()
}
