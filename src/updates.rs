//! Updates, as Discord does them: at start the splash waits for the check and
//! installs what it finds before the app opens (`ui::splash`); after that a
//! check every few hours, the download in the background, then a green button
//! to restart into the new version. fastframe-update reads the GitHub
//! release, verifies the signed checksums, swaps the executable (or runs the
//! next setup program silently) through a helper and rolls back if the new
//! version does not start.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fastframe_update::{ReqwestTransport, UpdateConfig, Updater};

/// The repository releases come from.
pub const REPOSITORY: &str = "ShuShuzinhuu/cordfast";

pub const CONFIG: UpdateConfig = UpdateConfig {
    publisher_key: Some(include_str!("../assets/update-public-key.hex")),
    ..UpdateConfig::new(REPOSITORY, "CordFast", "cordfast", env!("CARGO_PKG_VERSION"))
};

#[derive(Default)]
pub enum State {
    #[default]
    Idle,
    Downloading { version: String, percent: u8 },
    /// Downloaded and verified: restarting installs it.
    Ready { version: String, prepared: Option<fastframe_update::Prepared> },
    /// This copy cannot replace itself (not installed the portable way).
    Unsupported { version: String, url: String },
    Failed(String),
}

pub type Shared = Arc<Mutex<State>>;

fn updater() -> anyhow::Result<Updater> {
    let transport = ReqwestTransport::new(
        reqwest::blocking::Client::builder().user_agent(concat!("CordFast/", env!("CARGO_PKG_VERSION"))),
    )?;
    Ok(Updater::new(CONFIG, transport))
}

/// The first check has answered, found nothing, or failed: the splash stops
/// waiting for it (see `ui::splash`). A download it started goes on.
pub static CHECKED: AtomicBool = AtomicBool::new(false);

/// Whether this copy can update itself at all: an installed or portable copy,
/// not a build run from `target`. Reads files only, so it is quick.
pub fn installable() -> bool {
    updater().is_ok_and(|updater| updater.installation().is_ok())
}

/// Checks now and every six hours, downloading what it finds.
pub fn spawn(state: Shared, wake: crate::backend::Wake) {
    std::thread::Builder::new()
        .name("cordfast-updates".into())
        .spawn(move || loop {
            let result = check(&state, &wake, &|| {
                CHECKED.store(true, Ordering::Relaxed);
                wake();
            });
            if let Err(error) = result {
                log::info!("update: {error:#}");
            }
            CHECKED.store(true, Ordering::Relaxed);
            wake();
            std::thread::sleep(Duration::from_secs(6 * 3600));
        })
        .expect("unable to start the update thread");
}

/// What starting up waits for, as Discord's splash does: nothing to update
/// goes on to the app; an update found is downloaded and installed first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AtStart {
    Wait,
    Install,
    Launch,
}

pub fn at_start(state: &Shared) -> AtStart {
    match &*state.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        State::Downloading { .. } => AtStart::Wait,
        State::Ready { prepared: Some(_), .. } => AtStart::Install,
        _ if CHECKED.load(Ordering::Relaxed) => AtStart::Launch,
        _ => AtStart::Wait,
    }
}

fn set(state: &Shared, value: State, wake: &crate::backend::Wake) {
    *state.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = value;
    wake();
}

/// `found` is told when the check found something to download, before the
/// download, so the splash shows its progress instead of moving on.
fn check(state: &Shared, wake: &crate::backend::Wake, found: &dyn Fn()) -> anyhow::Result<()> {
    if matches!(*state.lock().unwrap_or_else(std::sync::PoisonError::into_inner), State::Ready { .. }) {
        return Ok(());
    }
    let updater = updater()?;
    let Some(release) = updater.check()? else { return Ok(()) };
    log::info!("update: version {} available", release.version);
    if let Err(reason) = updater.installation() {
        log::info!("update: {reason}");
        set(state, State::Unsupported { version: release.version, url: release.url }, wake);
        return Ok(());
    }
    set(state, State::Downloading { version: release.version.clone(), percent: 0 }, wake);
    found();
    let version = release.version.clone();
    let prepared = updater.download(&release, |received, total| {
        let percent = if total > 0 { (received * 100 / total) as u8 } else { 0 };
        set(state, State::Downloading { version: version.clone(), percent }, wake);
    });
    match prepared {
        Ok(prepared) => set(state, State::Ready { version, prepared: Some(prepared) }, wake),
        Err(error) => set(state, State::Failed(format!("{error:#}")), wake),
    }
    Ok(())
}

/// Hands the downloaded version to the helper, which starts it with
/// `arguments`; the app must quit after.
pub fn restart(state: &Shared, arguments: Vec<String>) -> anyhow::Result<()> {
    let prepared = match &mut *state.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        State::Ready { prepared, .. } => prepared.take(),
        _ => None,
    };
    let Some(prepared) = prepared else { anyhow::bail!("no update ready") };
    updater()?.handoff(prepared, arguments)
}
