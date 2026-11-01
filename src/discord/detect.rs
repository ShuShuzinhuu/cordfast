//! Game detection by process, as Discord (and arRPC, for Vesktop) does it:
//! most games never talk to the IPC pipe, Discord recognises them from the
//! running executable against `/applications/detectable`. A scan every few
//! seconds compares each process's path, from its file name up through its
//! folders, with the executables listed for this platform. Like Discord's
//! RunningGameStore, it reports one game at a time: the one in the window
//! in front, or else the one already shown (helpers that run beside a game
//! match other entries: PUBG's execpubg.exe is "PUBG: Test Server").

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
    /// How many executables each game lists: a game with several (its
    /// client, its anti-cheat) is likelier the real one than a helper's
    /// single entry, when nothing else tells them apart.
    weight: HashMap<Id, usize>,
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
                *table.weight.entry(id).or_default() += 1;
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
    // Game id to when it was first seen running, and the one shown.
    let mut running: HashMap<Id, u64> = HashMap::new();
    let mut shown: Option<Id> = None;
    loop {
        let scan = table.clone();
        let (found, focused): (HashSet<Id>, Option<Id>) = tokio::task::spawn_blocking(move || {
            let found = processes().iter().filter_map(|p| scan.find(p)).collect();
            (found, foreground().and_then(|p| scan.find(&p)))
        })
        .await
        .unwrap_or_default();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        for id in &found {
            if !running.contains_key(id) {
                log::info!("detect: game {id} running");
                running.insert(*id, now);
            }
        }
        running.retain(|id, _| found.contains(id));
        // The game in front wins; else the one shown stays; else the most
        // likely real game among those running.
        let pick = focused
            .filter(|id| found.contains(id))
            .or_else(|| shown.filter(|id| found.contains(id)))
            .or_else(|| found.iter().copied().max_by_key(|id| (table.weight.get(id).copied().unwrap_or(0), std::cmp::Reverse(*id))));
        if pick != shown {
            if let Some(old) = shown {
                log::info!("detect: game {old} no longer shown");
                let _ = out.send(RpcEvent::Closed { socket: SOCKET | (old & (SOCKET - 1)) });
            }
            if let Some(id) = pick {
                log::info!("detect: showing game {id}");
                let _ = out.send(RpcEvent::Activity {
                    socket: SOCKET | (id & (SOCKET - 1)),
                    application: id,
                    activity: Some(json!({
                        "name": table.titles.get(&id),
                        "timestamps": { "start": running.get(&id).copied().unwrap_or(now) },
                    })),
                });
            }
            shown = pick;
        }
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

/// The executable of the window in front, as Discord picks the game it
/// shows.
#[cfg(windows)]
fn foreground() -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries on the foreground window and its process; the
    // handle opened is closed before returning.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size) != 0;
        CloseHandle(handle);
        ok.then(|| String::from_utf16_lossy(&buffer[..size as usize]))
    }
}

#[cfg(not(windows))]
fn foreground() -> Option<String> {
    None
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

    /// PUBG's helper matches "PUBG: Test Server" by its one executable;
    /// with nothing in front, the game listing more of them is shown.
    #[test]
    fn weighs_the_real_game_over_a_helper() {
        let list: Vec<Detectable> = serde_json::from_value(json!([
            { "id": "452140668265955339", "name": "PUBG: Test Server", "executables": [{ "name": "execpubg.exe", "os": OS }] },
            { "id": "1402418717781921935", "name": "PUBG: BATTLEGROUNDS", "executables": [{ "name": "tslgame.exe", "os": OS }, { "name": "win64/tslgame.exe", "os": OS }, { "name": "tslgame_be.exe", "os": OS }] },
        ]))
        .unwrap();
        let table = Table::new(list);
        assert_eq!(table.weight[&1402418717781921935], 3);
        assert!(table.weight[&452140668265955339] < table.weight[&1402418717781921935]);
    }
}
