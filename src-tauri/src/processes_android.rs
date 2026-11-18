use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub path: Option<String>,
    pub package_name: String,
    pub system: bool,
    pub icon: Option<String>,
}
pub fn client_memory(_: Option<u32>, _: impl FnOnce() -> Option<u32>) -> Option<u64> {
    None
}
