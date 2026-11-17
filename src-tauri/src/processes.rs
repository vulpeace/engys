use std::collections::HashSet;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub path: Option<String>,
}

/// Running processes, one entry per executable (deduplicated by path), sorted by name.
pub fn list() -> Vec<ProcessEntry> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let mut seen = HashSet::new();
    let mut out: Vec<ProcessEntry> = sys
        .processes()
        .values()
        // Skip kernel threads / helpers (no executable path).
        .filter(|p| p.thread_kind().is_none())
        .filter_map(|p| {
            let path = p.exe().map(|e| e.to_string_lossy().into_owned());
            let key = path.clone().unwrap_or_else(|| p.name().to_string_lossy().into_owned());
            if !seen.insert(key) {
                return None;
            }
            Some(ProcessEntry {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                path,
            })
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

#[derive(Clone, Copy)]
struct MemoryProcess {
    pid: u32,
    parent: Option<u32>,
    bytes: u64,
}

/// Resident memory of the client process tree, excluding the managed core tree.
/// OS process enumeration is used instead of summing Linux threads (which share
/// their process's memory). Exclude both core PIDs observed across the snapshot
/// so a concurrent core restart does not inflate the client figure.
pub fn client_memory(core_before: Option<u32>, core_after: impl FnOnce() -> Option<u32>) -> Option<u64> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory().without_tasks(),
    );
    let processes = system
        .processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| MemoryProcess {
            pid: p.pid().as_u32(),
            parent: p.parent().map(|pid| pid.as_u32()),
            bytes: p.memory(),
        })
        .collect::<Vec<_>>();
    let excluded = [core_before, core_after()].into_iter().flatten().collect();
    sum_client_memory(&processes, std::process::id(), &excluded)
}

fn sum_client_memory(processes: &[MemoryProcess], root: u32, excluded: &HashSet<u32>) -> Option<u64> {
    use std::collections::{HashMap, VecDeque};
    if excluded.contains(&root) || !processes.iter().any(|p| p.pid == root) {
        return None;
    }
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut memory = HashMap::new();
    for process in processes {
        memory.insert(process.pid, process.bytes);
        if let Some(parent) = process.parent {
            children.entry(parent).or_default().push(process.pid);
        }
    }
    let mut queue = VecDeque::from([root]);
    let mut seen = HashSet::new();
    let mut total = 0u64;
    while let Some(pid) = queue.pop_front() {
        if excluded.contains(&pid) || !seen.insert(pid) {
            continue;
        }
        total = total.saturating_add(memory.get(&pid).copied().unwrap_or(0));
        if let Some(descendants) = children.get(&pid) {
            queue.extend(descendants);
        }
    }
    Some(total)
}

#[cfg(test)]
mod memory_tests {
    use super::*;
    #[test]
    fn includes_webview_descendants_but_excludes_core_and_unrelated_processes() {
        let processes = [
            MemoryProcess {
                pid: 1,
                parent: None,
                bytes: 100,
            },
            MemoryProcess {
                pid: 2,
                parent: Some(1),
                bytes: 200,
            }, // WebView browser
            MemoryProcess {
                pid: 3,
                parent: Some(2),
                bytes: 50,
            }, // Renderer
            MemoryProcess {
                pid: 4,
                parent: Some(1),
                bytes: 300,
            }, // Core
            MemoryProcess {
                pid: 5,
                parent: Some(4),
                bytes: 30,
            }, // Core helper
            MemoryProcess {
                pid: 6,
                parent: None,
                bytes: 999,
            },
        ];
        assert_eq!(sum_client_memory(&processes, 1, &HashSet::from([4])), Some(350));
        assert_eq!(sum_client_memory(&processes, 1, &HashSet::new()), Some(680));
    }
    #[test]
    fn core_restarts_are_excluded_and_missing_client_is_unavailable() {
        let processes = [
            MemoryProcess {
                pid: 1,
                parent: None,
                bytes: 100,
            },
            MemoryProcess {
                pid: 2,
                parent: Some(1),
                bytes: 200,
            }, // Old core
            MemoryProcess {
                pid: 3,
                parent: Some(1),
                bytes: 300,
            }, // New core
        ];
        assert_eq!(sum_client_memory(&processes, 1, &HashSet::from([2, 3])), Some(100));
        assert_eq!(sum_client_memory(&processes, 9, &HashSet::new()), None);
    }
}
