//! Game detection by process, as Discord (and arRPC, for Vesktop) does it:
//! most games never talk to the IPC pipe, Discord recognises them from the
//! running executable against `/applications/detectable`. A scan every few
//! seconds compares each process's path, from its file name up through its
//! folders, with the executables listed for this platform.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;

use super::model::Id;
use super::rpc::RpcEvent;

/// Detected games get their own "socket" numbers, far from the pipe's.
const SOCKET: u64 = 1 << 62;
const SCAN: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
pub struct Detectable {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    executables: Option<Vec<Executable>>,
}

#[derive(Deserialize)]
struct Executable {
    name: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    is_launcher: bool,
}

#[cfg(windows)]
const OS: &str = "win32";
#[cfg(target_os = "macos")]
const OS: &str = "darwin";
#[cfg(not(any(windows, target_os = "macos")))]
const OS: &str = "linux";

/// Executable paths ("bin/game.exe", lowercased) to the game's id, and the
/// bare file names among them for a cheap first look at each process.
#[derive(Default)]
struct Table {
    paths: HashMap<String, Id>,
    names: HashSet<String>,
    titles: HashMap<Id, String>,
}

impl Table {
    fn new(list: Vec<Detectable>) -> Self {
        let mut table = Self::default();
        for app in list {
            let Ok(id) = app.id.parse::<Id>() else { continue };
            table.titles.insert(id, app.name);
            for exe in app.executables.into_iter().flatten() {
                if exe.is_launcher || exe.os != OS {
                    continue;
                }
                let path = exe.name.trim_start_matches('>').to_lowercase().replace('\\', "/");
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                if name.is_empty() {
                    continue;
                }
                table.names.insert(name);
                table.paths.entry(path).or_insert(id);
            }
        }
        table
    }

    /// The game this executable path belongs to, trying the longest
    /// trailing part first ("steamapps/common/x/bin/x.exe", …, "x.exe"),
    /// and each without a "64" suffix, as arRPC does.
    fn find(&self, path: &str) -> Option<Id> {
        let path = path.to_lowercase().replace('\\', "/");
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && !p.ends_with(':')).collect();
        if !self.names.contains(*parts.last()?) {
            return None;
        }
        for start in 0..parts.len() {
            let tail = parts[start..].join("/");
            for candidate in [tail.clone(), tail.replace("64", ""), tail.replace(".x64", ""), tail.replace("x64", ""), tail.replace("_64", "")] {
                if let Some(id) = self.paths.get(&candidate) {
                    return Some(*id);
                }
            }
        }
        None
    }
}

/// Watches the running processes and reports detected games as activities
/// on `out`, the way the pipe reports games that set their own.
pub async fn run(api: super::rest::Api, out: mpsc::UnboundedSender<RpcEvent>) {
    let table = loop {
        match api.detectable().await {
            Ok(list) => break Table::new(list),
            Err(error) => {
                log::warn!("detect: {error:#}");
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
        }
    };
    log::info!("detect: {} executables known", table.paths.len());
    let table = std::sync::Arc::new(table);
    // Game id to when it was first seen running.
    let mut running: HashMap<Id, u64> = HashMap::new();
    loop {
        let scan = table.clone();
        let found: HashSet<Id> = tokio::task::spawn_blocking(move || {
            processes().iter().filter_map(|p| scan.find(p)).collect()
        })
        .await
        .unwrap_or_default();
        for id in &found {
            if running.contains_key(id) {
                continue;
            }
            let start = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            running.insert(*id, start);
            log::info!("detect: game {id} started");
            let _ = out.send(RpcEvent::Activity {
                socket: SOCKET | (*id & (SOCKET - 1)),
                application: *id,
                activity: Some(json!({
                    "name": table.titles.get(id),
                    "timestamps": { "start": start },
                })),
            });
        }
        running.retain(|id, _| {
            let alive = found.contains(id);
            if !alive {
                log::info!("detect: game {id} closed");
                let _ = out.send(RpcEvent::Closed { socket: SOCKET | (id & (SOCKET - 1)) });
            }
            alive
        });
        tokio::time::sleep(SCAN).await;
    }
}

/// Whether an activity "socket" is one of ours, not the pipe's.
pub fn detected(socket: u64) -> bool {
    socket & SOCKET != 0
}

/// Every running process's executable path.
#[cfg(windows)]
fn processes() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    let mut out = Vec::new();
    let own = std::process::id();
    // SAFETY: a process snapshot walked with a correctly sized entry; every
    // handle opened here is closed before returning.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            if entry.th32ProcessID != own && entry.th32ProcessID != 0 {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                // The full path is needed only where the folder decides.
                let mut path = name.clone();
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !handle.is_null() {
                    let mut buffer = [0u16; 1024];
                    let mut size = buffer.len() as u32;
                    if QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size) != 0 {
                        path = String::from_utf16_lossy(&buffer[..size as usize]);
                    }
                    CloseHandle(handle);
                }
                out.push(path);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    out
}

/// On Linux the command line's first word (a Wine or Proton game shows its
/// .exe there), or else the executable's link.
#[cfg(unix)]
fn processes() -> Vec<String> {
    let own = std::process::id().to_string();
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()) && n != own))
        .filter_map(|e| {
            let cmdline = std::fs::read(e.path().join("cmdline")).ok()?;
            let first = cmdline.split(|&b| b == 0).next().filter(|f| !f.is_empty());
            match first {
                Some(first) => Some(String::from_utf8_lossy(first).into_owned()),
                None => std::fs::read_link(e.path().join("exe")).ok().map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}

#[cfg(not(any(windows, unix)))]
fn processes() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_trailing_paths() {
        let list: Vec<Detectable> = serde_json::from_value(json!([
            { "id": "1", "executables": [{ "name": "game/bin/game.exe", "os": OS }] },
            { "id": "2", "executables": [{ "name": "launcher.exe", "os": OS, "is_launcher": true }] },
            { "id": "3", "executables": [{ "name": "other.exe", "os": "nowhere" }] },
        ]))
        .unwrap();
        let table = Table::new(list);
        assert_eq!(table.find(r"C:\Steam\steamapps\common\Game\bin\Game.exe"), Some(1));
        assert_eq!(table.find(r"C:\x\launcher.exe"), None);
        assert_eq!(table.find(r"C:\x\other.exe"), None);
        assert_eq!(table.find(r"C:\x\bin\game.exe"), None);
    }
}
