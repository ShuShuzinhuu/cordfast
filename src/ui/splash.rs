//! Discord's updater window, the one its desktop app shows before the app:
//! "Checking for updates…", the download's progress, then "Starting…". Taken
//! from the desktop host (`app.asar`'s `splashScreen` and `splash/index.css`):
//! a 300×350 frameless window in `--account-grey`, the logo spinning (its
//! `connecting.webp`, 64 frames in 1066 ms), the status in white and an
//! 8-point bar under it. Discord retries forever when the check fails; this
//! fails open at once, as Discord does past its update deadline, since nothing
//! the app needs comes with an update.
//!
//! It is the main window itself, at the splash's size, which grows into the
//! app when it is done: eframe runs one event loop, and a second window would
//! need one of its own.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::icons::{self, Icon};
use crate::updates::{self, AtStart};

/// The splash window's size on Windows (`LOADING_WINDOW_HEIGHT`).
pub const SIZE: [f32; 2] = [300.0, 350.0];
/// The app's window, which the splash grows into (as `main.rs` opens it).
pub const APP_SIZE: [f32; 2] = [1240.0, 800.0];
pub const APP_MIN_SIZE: [f32; 2] = [760.0, 480.0];

/// `--account-grey`, `--white`, the bar's track and `--grey-3` fill.
const BACKGROUND: Color32 = Color32::from_rgb(0x28, 0x2b, 0x30);
const TRACK: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
const FILL: Color32 = Color32::from_rgb(0x73, 0x7f, 0x8d);
/// The animation's loop.
const LOOP: f32 = 1.066;
/// How long "Starting…" shows before the app takes the window.
const STARTING: Duration = Duration::from_millis(400);
/// The longest the check is waited for: a slow network should not hold the
/// app back (Discord's own fail-open asks 15 s of its manifest request).
const DEADLINE: Duration = Duration::from_secs(10);

pub struct Splash {
    since: Instant,
    /// When "Starting…" began.
    starting: Option<Instant>,
    /// The update was handed to the helper.
    installing: bool,
    /// The app window's place as eframe restored it (maximized, or where and
    /// how big), given back when the splash grows into it.
    restore: Option<(bool, Option<Rect>)>,
    /// Frames since the window opened, unseen until it is the splash's size.
    frames: u8,
}

impl Splash {
    pub fn new() -> Self {
        Self { since: Instant::now(), starting: None, installing: false, restore: None, frames: 0 }
    }
}

/// eframe opens the window where it was last time, maximized say, and hidden
/// (`main.rs`): the first frame notes that place and shrinks it to the
/// splash, centred on the screen, and the next shows it.
fn place(splash: &mut Splash, ctx: &egui::Context) {
    splash.frames = splash.frames.saturating_add(1);
    if splash.restore.is_none() {
        let (maximized, outer, monitor) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().outer_rect, i.viewport().monitor_size));
        splash.restore = Some((maximized, outer));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::ZERO));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(SIZE.into()));
        if let Some(monitor) = monitor {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2((monitor.x - SIZE[0]) / 2.0, (monitor.y - SIZE[1]) / 2.0)));
        }
        ctx.request_repaint();
    } else if splash.frames == 3 {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

enum Status {
    Checking,
    Downloading(u8),
    Installing,
    Starting,
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let at_start = updates::at_start(&app.updates);
    let percent = percent(app);
    let quitting = app.quit;
    let Some(splash) = app.splash.as_mut() else { return };
    place(splash, ui.ctx());
    let mut status = match at_start {
        // Handed over: the app quits now. Not handed over (the helper did
        // not start): the app opens, and the error shows there.
        _ if splash.installing && quitting => Status::Installing,
        AtStart::Wait if splash.since.elapsed() < DEADLINE || percent.is_some() => match percent {
            Some(percent) => Status::Downloading(percent),
            None => Status::Checking,
        },
        AtStart::Install if !splash.installing => {
            splash.installing = true;
            app.actions.push(Action::InstallUpdate);
            Status::Installing
        }
        _ => Status::Starting,
    };
    if matches!(status, Status::Starting) {
        let since = *splash.starting.get_or_insert_with(Instant::now);
        if since.elapsed() >= STARTING && splash.frames > 3 {
            let restore = splash.restore.unwrap_or((false, None));
            app.splash = None;
            grow(ui.ctx(), restore);
            return;
        }
    } else if splash.starting.is_some() {
        status = Status::Starting;
    }
    let elapsed = splash.since.elapsed().as_secs_f32();
    egui::CentralPanel::default().frame(Frame::new().fill(BACKGROUND)).show(ui, |ui| {
        let rect = ui.max_rect();
        // The whole window drags, as `-webkit-app-region: drag` makes it.
        let response = ui.interact(rect, ui.id().with("splash-drag"), Sense::drag());
        if response.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        // `#splash` centres `.splash-inner` (a 200-point image, then the text
        // pulled up 30 points) over 10 points of bottom padding.
        let block = 200.0 + 20.0 + 10.0 + 8.0;
        let top = rect.top() + (rect.height() - 10.0 - block) / 2.0;
        let image = Rect::from_min_size(pos2(rect.center().x - 100.0, top), Vec2::splat(200.0));
        logo(ui, image, elapsed);
        let text = match status {
            Status::Checking => t!("Checking for updates…").to_owned(),
            Status::Downloading(_) => t!("Downloading update {current} of {total}…", current = 1, total = 1),
            Status::Installing => t!("Installing update {current} of {total}…", current = 1, total = 1),
            Status::Starting => t!("Starting…").to_owned(),
        };
        let y = image.bottom() - 30.0;
        ui.painter().text(pos2(rect.center().x, y + 10.0), Align2::CENTER_CENTER, text, FontId::proportional(16.0), Color32::WHITE);
        let progress = match status {
            Status::Downloading(percent) => Some(f32::from(percent) / 100.0),
            Status::Installing => Some(1.0),
            _ => None,
        };
        if let Some(progress) = progress {
            let bar = Rect::from_min_size(pos2(rect.center().x - 90.0, y + 30.0), vec2(180.0, 8.0));
            ui.painter().rect_filled(bar, CornerRadius::same(4), TRACK);
            let done = Rect::from_min_size(bar.min, vec2(bar.width() * progress.clamp(0.0, 1.0), bar.height()));
            ui.painter().rect_filled(done, CornerRadius::same(4), FILL);
        }
    });
    // The logo spins; 30 frames a second is plenty for it.
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}

fn percent(app: &App) -> Option<u8> {
    match &*app.updates.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        updates::State::Downloading { percent, .. } => Some(*percent),
        _ => None,
    }
}

/// Discord's `connecting.webp`: the logo at rest for a third of the loop,
/// then a whirl round with its blur, overshooting and settling back.
fn logo(ui: &egui::Ui, image: Rect, elapsed: f32) {
    let t = elapsed % LOOP / LOOP;
    let angle = |t: f32| -> f32 {
        if t < 0.3 {
            0.0
        } else if t < 0.75 {
            let u = (t - 0.3) / 0.45;
            // Fast at first, slowing into the end of the turn.
            std::f32::consts::TAU * (1.0 - (1.0 - u).powi(3)) * 1.04
        } else {
            // The overshoot, damped back to upright.
            let u = (t - 0.75) / 0.25;
            std::f32::consts::TAU + std::f32::consts::TAU * 0.04 * (1.0 - u).powi(2) * (u * 9.0).cos()
        }
    };
    let now = angle(t);
    // Its size in the 200-point image: about 72 points across.
    let grow = if (0.3..0.75).contains(&t) { 1.0 + 0.12 * ((t - 0.3) / 0.45 * std::f32::consts::PI).sin() } else { 1.0 };
    let logo = Rect::from_center_size(image.center(), Vec2::splat(72.0 * grow));
    // The blur: the turn's last moments, fainter.
    let speed = (now - angle((t - 0.03).max(0.0))).abs();
    if speed > 0.05 {
        for step in 1..=3 {
            let back = angle((t - 0.012 * step as f32).max(0.0));
            let alpha = (60 / step) as u8;
            icons::paint_rotated(ui, logo, Icon::Discord, Color32::from_white_alpha(alpha), back);
        }
    }
    icons::paint_rotated(ui, logo, Icon::Discord, Color32::WHITE, now);
}

/// The splash becomes the app's window, where eframe had put it: maximized
/// again, or back at its place and size, resizable, with the system's frame
/// if that was chosen.
fn grow(ctx: &egui::Context, (maximized, outer): (bool, Option<Rect>)) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!fastframe_titlebar::enabled()));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(APP_MIN_SIZE.into()));
    if maximized {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
    } else {
        let size = outer.map_or(APP_SIZE.into(), |r| r.size()).max(APP_MIN_SIZE.into());
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        if let Some(outer) = outer {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(outer.min));
        }
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}
