//! The game overlay, as Discord's new one (its "out of process" overlay, the
//! bundle's `OVERLAY_V3`): a transparent window of the game's size above the
//! game, that clicks pass through while it is locked. Nothing enters the game's
//! process, so anti-cheats have nothing to object to; the price, the same as
//! Discord's, is that a game in exclusive fullscreen hides it while locked ("You
//! must run this game in borderless windowed mode"; unlocking still shows it, as
//! the game then loses the screen). Discord's legacy overlay, the one that injects
//! a DLL, is deliberately not here.
//!
//! The widgets, where they start and which are pinned come from the bundle's
//! widget table. Locked, only the pinned ones show, without their frames.
//! Unlocked (Shift+`), everything shows over a dimmed game with the bar on top;
//! Esc or a click on the background locks it again and Alt held hides the widgets.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::discord::detect;
use crate::discord::model::Id;
use crate::icons::{self, Icon};
use crate::theme;

use super::{form, initials, paint_truncated};

/// How long a message stays on screen, as the bundle's notifications store.
const NOTE_LIFE: Duration = Duration::from_secs(5);
/// Discord's overlay always uses its darkest theme, whatever the app's is.
const PANEL: Color32 = Color32::from_rgba_premultiplied(16, 17, 19, 240);
const PILL: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 170);
const DIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 128);
const HEADER: f32 = 32.0;

fn viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("cordfast-overlay")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    Voice,
    QuickActions,
    Notifications,
    Chat,
    Friends,
    Activity,
}

const ALL: [Widget; 6] = [Widget::Voice, Widget::QuickActions, Widget::Notifications, Widget::Chat, Widget::Friends, Widget::Activity];

/// Where a widget starts: the left edge or, from the right edge, as a part of
/// the game's window (the bundle's `anchor`).
enum Edge {
    Left(f32),
    Right(f32),
}

impl Widget {
    fn id(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::QuickActions => "quick_actions",
            Self::Notifications => "notifications",
            Self::Chat => "chat",
            Self::Friends => "friends",
            Self::Activity => "activity",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Voice => t!("Voice"),
            Self::QuickActions => t!("Quick Actions"),
            Self::Notifications => t!("Notifications"),
            Self::Chat => t!("Text Chat"),
            Self::Friends => t!("Friends"),
            Self::Activity => t!("Activity"),
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Voice => t!("Who's talking in your call"),
            Self::QuickActions => t!("Mute, deafen and more"),
            Self::Notifications => t!("Messages, calls and invites"),
            Self::Chat => t!("Read and send messages"),
            Self::Friends => t!("See who's online"),
            Self::Activity => t!("What friends are playing"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Voice => Icon::Voice,
            Self::QuickActions => Icon::Microphone,
            Self::Notifications => Icon::Bell,
            Self::Chat => Icon::Chat,
            Self::Friends => Icon::Friends,
            Self::Activity => Icon::Controller,
        }
    }

    /// The bundle's `defaultSettings`.
    fn default_anchor(self) -> (Edge, f32) {
        match self {
            Self::Voice => (Edge::Left(0.01), 0.35),
            Self::QuickActions => (Edge::Left(0.34), 0.02),
            Self::Notifications => (Edge::Left(0.01), 0.02),
            Self::Chat => (Edge::Right(0.26), 0.14),
            Self::Friends => (Edge::Right(0.01), 0.14),
            Self::Activity => (Edge::Right(0.01), 0.35),
        }
    }

    fn pinned_by_default(self) -> bool {
        matches!(self, Self::Voice | Self::Notifications)
    }

    /// Streamer Mode hides what shows other people, as the bundle's
    /// `hideableInStreamerMode`.
    fn hidden_by_streamer_mode(self) -> bool {
        matches!(self, Self::Voice | Self::Chat | Self::Friends)
    }
}

/// A message on screen. Sound and the system's notice are the app's as usual.
pub struct Note {
    pub title: String,
    pub body: String,
    pub channel: Id,
    pub avatar: Option<String>,
    pub at: Instant,
}

pub struct Overlay {
    pub locked: bool,
    /// The bar's Widgets button.
    pub panel_open: bool,
    pub notes: Vec<Note>,
    /// Set by the global shortcut; the next frame acts on it.
    pub toggle: bool,
    /// `CORDFAST_OVERLAY_INLINE`: drawn inside the main window, where the
    /// self capture reaches (eframe takes no screenshot of an immediate viewport).
    inline: bool,
    /// Whether it was on screen, to write it down when that changes.
    showing: bool,
    /// The locked window has been told its opacity (see `detect::show_layered`).
    layered: bool,
    /// Its window exists (see `show`).
    open: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            locked: std::env::var_os("CORDFAST_OVERLAY_UNLOCKED").is_none(),
            panel_open: false,
            notes: Vec::new(),
            toggle: false,
            inline: std::env::var_os("CORDFAST_OVERLAY_INLINE").is_some(),
            showing: false,
            layered: false,
            open: false,
        }
    }
}

/// Whether the overlay has reason to keep the window system alive: the window
/// hidden to the tray stays as a hidden window during a call, so that the
/// overlay can still appear over a game.
pub fn wanted_in_tray(app: &App) -> bool {
    cfg!(windows) && app.settings.overlay_enabled && app.voice.channel.is_some()
}

pub fn note_message(app: &mut App, title: &str, body: &str, channel: Id, avatar: Option<String>) {
    if !app.settings.overlay_enabled || !app.settings.overlay_notify_messages || detect::front().is_none() {
        return;
    }
    app.overlay.notes.push(Note { title: title.to_owned(), body: body.to_owned(), channel, avatar, at: Instant::now() });
    let excess = app.overlay.notes.len().saturating_sub(4);
    app.overlay.notes.drain(..excess);
}

fn hidden(app: &App, widget: Widget) -> bool {
    app.settings.overlay_hidden.iter().any(|id| id == widget.id()) || (widget.hidden_by_streamer_mode() && app.streamer_on() && app.settings.streamer_hide_personal)
}

fn pinned(app: &App, widget: Widget) -> bool {
    app.settings.overlay_widgets.get(widget.id()).and_then(|saved| saved.pinned).unwrap_or_else(|| widget.pinned_by_default())
}

/// The widget's height and width, in points.
fn size(app: &App, widget: Widget, locked: bool, screen: Vec2) -> Vec2 {
    let chrome = if locked { 0.0 } else { HEADER };
    match widget {
        Widget::Voice => {
            let rows = voice_rows(app).len().max(1) as f32;
            vec2(272.0, chrome + rows * (voice_avatar(app) + 4.0) + 8.0)
        }
        Widget::QuickActions => vec2(216.0, chrome + 104.0),
        Widget::Notifications => {
            let rows = (app.overlay.notes.len() + usize::from(app.incoming_call().is_some())).max(1) as f32;
            vec2(320.0, chrome + rows * 68.0 + 8.0)
        }
        Widget::Chat => vec2(500.0, 372.0 + chrome),
        // From 14% of the height to 30% from the bottom, as the bundle's anchor.
        Widget::Friends => vec2(350.0, (screen.y * (1.0 - 0.14 - 0.3)).max(350.0)),
        Widget::Activity => {
            let rows = playing(app).len().max(1) as f32;
            vec2(320.0, chrome + rows * 52.0 + 8.0)
        }
    }
}

fn position(app: &App, widget: Widget, size: Vec2, screen: Vec2) -> egui::Pos2 {
    let (x, y) = match app.settings.overlay_widgets.get(widget.id()).and_then(|saved| saved.at) {
        Some([x, y]) => (x * screen.x, y * screen.y),
        None => {
            let (edge, top) = widget.default_anchor();
            let x = match edge {
                Edge::Left(left) => left * screen.x,
                Edge::Right(right) => screen.x * (1.0 - right) - size.x,
            };
            (x, top * screen.y)
        }
    };
    pos2(x.clamp(0.0, (screen.x - size.x).max(0.0)), y.clamp(0.0, (screen.y - size.y).max(0.0)))
}

/// Draws the overlay's window.
///
/// With no game in front that window still exists, a pixel out of sight, whenever
/// the main window might go out of sight itself, because eframe:
/// - runs no frames for a minimized or hidden window that shows no other window,
/// - and draws such a window's frames where it cannot open a new window (asking
///   for one there is a panic: "the user callback was never called").
///
/// So it opens before that can happen: when the main window loses the focus, or the
/// pointer is over its title bar with the minimize button. Not while the main window
/// is in use: each window's swap waits for the screen, which would halve its frames.
/// `Win+D` from the focused window slips through; the overlay is then back at the
/// next time the window has been seen.
pub fn show(app: &mut App, ctx: &egui::Context, focused: bool, unseen: bool) {
    if app.overlay.inline {
        egui::Area::new(egui::Id::new("overlay-inline")).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(ctx, |ui| content(app, ui));
        return;
    }
    let front = detect::front().filter(|_| app.settings.overlay_enabled);
    let near_title_bar = ctx.input(|i| i.pointer.latest_pos().is_some_and(|at| at.y < 40.0));
    let may_open = cfg!(windows) && app.settings.overlay_enabled && (!focused || unseen || near_title_bar);
    if std::mem::take(&mut app.overlay.toggle) && front.is_some() {
        set_locked(app, ctx, !app.overlay.locked);
    }
    if front.is_some() != app.overlay.showing {
        app.overlay.showing = front.is_some();
        log::info!("overlay: {}", if app.overlay.showing { "over a game" } else { "gone" });
    }
    // Only a frame of a window in sight can open it.
    if unseen && !app.overlay.open {
        return;
    }
    let Some(front) = front else {
        // Locked again for the next game.
        app.overlay.locked = std::env::var_os("CORDFAST_OVERLAY_UNLOCKED").is_none();
        app.overlay.notes.clear();
        app.overlay.open = may_open;
        if may_open {
            let builder = locked_builder(pos2(-200.0, -200.0), vec2(1.0, 1.0), true);
            ctx.show_viewport_immediate(viewport(), builder, |_, _| ());
            keep_layered(app, ctx);
        } else {
            // Its window goes too, and the next one starts over.
            app.overlay.layered = false;
        }
        return;
    };
    app.overlay.open = true;
    app.overlay.notes.retain(|note| note.at.elapsed() < NOTE_LIFE);
    if let Some(next) = app.overlay.notes.iter().map(|note| NOTE_LIFE.saturating_sub(note.at.elapsed())).min() {
        ctx.request_repaint_after(next);
    }
    // The game's pixels, in this window's points.
    let scale = ctx.pixels_per_point().max(0.1);
    let [left, top, right, bottom] = front.rect;
    let locked = app.overlay.locked;
    let at = pos2(left as f32 / scale, top as f32 / scale);
    let size = vec2((right - left) as f32 / scale, (bottom - top) as f32 / scale);
    ctx.show_viewport_immediate(viewport(), locked_builder(at, size, locked), |ui, _| content(app, ui));
    if locked {
        keep_layered(app, ctx);
    } else {
        app.overlay.layered = false;
    }
}

fn locked_builder(at: egui::Pos2, size: Vec2, locked: bool) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(detect::OVERLAY_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_drag_and_drop(false)
        .with_mouse_passthrough(locked)
        .with_position(at)
        .with_inner_size(size)
}

/// Click-through makes the window "layered", and Windows shows nothing of a
/// layered window until it is told how opaque it is. Leaving that state forgets
/// it, so it is said again on each lock.
fn keep_layered(app: &mut App, ctx: &egui::Context) {
    if !app.overlay.layered {
        app.overlay.layered = detect::show_layered();
        // Not there yet, or there and with nothing drawn since it became layered.
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn set_locked(app: &mut App, ctx: &egui::Context, locked: bool) {
    app.overlay.locked = locked;
    app.overlay.panel_open = false;
    if locked {
        // Back to the game, as the keys and the mouse should be.
        if let Some(front) = detect::front() {
            detect::focus(front.window);
        }
    } else {
        ctx.send_viewport_cmd_to(viewport(), egui::ViewportCommand::Focus);
    }
    ctx.request_repaint();
}

fn content(app: &mut App, ui: &mut egui::Ui) {
    let screen = ui.ctx().content_rect();
    let locked = app.overlay.locked;
    if !locked {
        ui.painter().rect_filled(screen, 0.0, DIM);
        // Registered first: what is drawn over it takes its clicks.
        let background = ui.interact(screen, ui.id().with("overlay-background"), Sense::click());
        if background.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            set_locked(app, &ui.ctx().clone(), true);
            return;
        }
    }
    // Alt held shows the game under the open overlay.
    let peek = !locked && ui.input(|i| i.modifiers.alt);
    if !peek {
        for widget in ALL {
            if hidden(app, widget) || (locked && !pinned(app, widget)) {
                continue;
            }
            // A pinned widget with nothing in it stays out of the game's way.
            if locked && widget == Widget::Voice && app.voice.channel.is_none() {
                continue;
            }
            if locked && widget == Widget::Notifications && app.overlay.notes.is_empty() && app.incoming_call().is_none() {
                continue;
            }
            draw_widget(app, ui, widget, locked, screen.size());
        }
    }
    if !locked {
        bar(app, ui, screen);
        if app.overlay.panel_open {
            widgets_panel(app, ui, screen);
        } else if !peek && ALL.iter().all(|widget| hidden(app, *widget)) {
            empty_state(app, ui, screen);
        }
    }
}

// ---- the frame around a widget ------------------------------------------------------

fn draw_widget(app: &mut App, ui: &mut egui::Ui, widget: Widget, locked: bool, screen: Vec2) {
    let size = size(app, widget, locked, screen);
    let rect = Rect::from_min_size(position(app, widget, size, screen), size);
    let p = theme::p();
    let body = if locked {
        rect
    } else {
        ui.painter().rect_filled(rect, 8.0, PANEL);
        ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.0, Color32::from_white_alpha(24)), egui::StrokeKind::Inside);
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
        let drag = ui.interact(header, ui.id().with(("overlay-drag", widget.id())), Sense::click_and_drag());
        if drag.dragged() {
            let next = rect.min + drag.drag_delta();
            app.settings.overlay_widgets.entry(widget.id().to_owned()).or_default().at = Some([next.x / screen.x, next.y / screen.y]);
        }
        if drag.drag_stopped() {
            app.actions.push(Action::SettingsChanged);
        }
        let chat = (widget == Widget::Chat).then(|| app.selected_channel.or_else(|| app.voice.channel.map(|(_, channel)| channel))).flatten().and_then(|id| app.channel(id)).map(crate::discord::model::Channel::title);
        paint_truncated(ui, Rect::from_min_max(header.min + vec2(10.0, 0.0), pos2(header.right() - 60.0, header.bottom())), chat.as_deref().unwrap_or(widget.label()), theme::strong(13.0), p.strong);
        let close = Rect::from_center_size(pos2(header.right() - 16.0, header.center().y), Vec2::splat(22.0));
        if icon_button(ui, close, Icon::Close, false, t!("Hide widget"), ("overlay-hide", widget.id())) {
            app.settings.overlay_hidden.push(widget.id().to_owned());
            app.actions.push(Action::SettingsChanged);
        }
        let pin = Rect::from_center_size(pos2(close.left() - 13.0, header.center().y), Vec2::splat(22.0));
        let is_pinned = pinned(app, widget);
        if icon_button(ui, pin, Icon::Pin, is_pinned, if is_pinned { t!("Unpin") } else { t!("Pin") }, ("overlay-pin", widget.id())) {
            app.settings.overlay_widgets.entry(widget.id().to_owned()).or_default().pinned = Some(!is_pinned);
            app.actions.push(Action::SettingsChanged);
        }
        Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max)
    };
    let mut inner = ui.new_child(egui::UiBuilder::new().id_salt(("overlay-widget", widget.id())).max_rect(body));
    inner.set_clip_rect(body);
    match widget {
        Widget::Voice => voice(app, &mut inner, locked),
        Widget::QuickActions => quick_actions(app, &mut inner),
        Widget::Notifications => notifications(app, &mut inner, locked),
        Widget::Chat => super::chat::overlay_widget(app, &mut inner),
        Widget::Friends => friends(app, &mut inner),
        Widget::Activity => activity(app, &mut inner),
    }
}

/// A small round button; whether it was clicked.
fn icon_button(ui: &mut egui::Ui, rect: Rect, icon: Icon, on: bool, tooltip: &str, id: impl egui::AsIdSalt) -> bool {
    let p = theme::p();
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, Color32::from_white_alpha(20));
    }
    icons::paint(ui, rect.shrink(4.0), icon, if on || response.hovered() { p.strong } else { p.muted });
    let clicked = response.clicked();
    response.on_hover_text(tooltip);
    clicked
}

// ---- voice ---------------------------------------------------------------------------

struct VoiceRow {
    name: String,
    avatar: String,
    speaking: bool,
    muted: bool,
    deaf: bool,
}

fn voice_avatar(app: &App) -> f32 {
    if app.settings.overlay_avatar_large { 32.0 } else { 24.0 }
}

/// Who shows, in Discord's order, up to "Max Users Displayed" (0 is no limit).
fn voice_rows(app: &App) -> Vec<VoiceRow> {
    let Some((guild, channel)) = app.voice.channel else { return Vec::new() };
    let Some(channel) = app.channel(channel) else { return Vec::new() };
    let me = app.me_id();
    let mut rows: Vec<VoiceRow> = app
        .voice_members(channel)
        .into_iter()
        .filter_map(|id| {
            let user = if id == me { app.me.as_ref() } else { app.users.get(&id) }?;
            let name = guild.and_then(|g| app.members.get(&(g, id))).and_then(|m| m.nick.clone()).unwrap_or_else(|| user.display_name().to_owned());
            let (muted, deaf) = if id == me { (app.voice.mute, app.voice.deaf) } else { app.voice_flags.get(&id).copied().unwrap_or_default() };
            Some(VoiceRow { name, avatar: user.avatar_url(), speaking: app.voice.speaking.contains(&id), muted, deaf })
        })
        .collect();
    // 1: "Only While Speaking" (the unlocked overlay shows everyone to arrange it).
    if app.settings.overlay_users == 1 && app.overlay.locked {
        rows.retain(|row| row.speaking);
    }
    rows
}

fn voice(app: &mut App, ui: &mut egui::Ui, locked: bool) {
    let p = theme::p();
    let rows = voice_rows(app);
    let area = ui.max_rect();
    if rows.is_empty() {
        if !locked {
            ui.painter().text(area.center(), Align2::CENTER_CENTER, t!("No one is currently in voice"), FontId::proportional(13.0), p.muted);
        }
        return;
    }
    let limit = usize::from(app.settings.overlay_max_users);
    let size = voice_avatar(app);
    let mut y = area.top() + 4.0;
    let total = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        if limit > 0 && index >= limit {
            let hidden = total - limit;
            let rect = Rect::from_min_size(pos2(area.left() + 4.0, y), vec2(area.width() - 8.0, size));
            paint_truncated(ui, rect, &tn!(hidden, "+{count} other in voice", "+{count} others in voice"), FontId::proportional(12.0), p.strong);
            break;
        }
        let avatar = Rect::from_min_size(pos2(area.left() + 4.0, y), Vec2::splat(size));
        app.images.circle(ui, avatar, Some(&row.avatar), &initials(&row.name), size / 2.0);
        if row.speaking {
            ui.painter().circle_stroke(avatar.center(), size / 2.0 + 1.0, Stroke::new(2.0, p.green));
        }
        // 0 always, 1 only while speaking, 2 never.
        let show_name = match app.settings.overlay_names {
            0 => true,
            1 => row.speaking,
            _ => false,
        };
        let mut x = avatar.right() + 6.0;
        let right = area.right() - 4.0;
        if show_name {
            let font = FontId::proportional(13.0);
            let width = ui.painter().layout_no_wrap(row.name.clone(), font.clone(), p.strong).size().x;
            let flags = if row.muted || row.deaf { 42.0 } else { 0.0 };
            let pill = Rect::from_min_size(pos2(x, avatar.center().y - 11.0), vec2((width + 14.0).min(right - x - flags).max(20.0), 22.0));
            ui.painter().rect_filled(pill, 11.0, PILL);
            paint_truncated(ui, pill.shrink2(vec2(7.0, 0.0)), &row.name, font, if row.speaking { p.strong } else { Color32::from_gray(225) });
            x = pill.right() + 4.0;
        }
        for (on, icon) in [(row.muted, Icon::MicrophoneSlash), (row.deaf, Icon::HeadphonesSlash)] {
            if on {
                let badge = Rect::from_center_size(pos2(x + 9.0, avatar.center().y), Vec2::splat(18.0));
                ui.painter().circle_filled(badge.center(), 9.0, PILL);
                icons::paint(ui, badge.shrink(3.0), icon, p.red);
                x += 20.0;
            }
        }
        y += size + 4.0;
    }
}

// ---- quick actions -------------------------------------------------------------------

/// Discord's panel: microphone, headphones and, connected, the call's name and
/// the way out. Camera and Go Live are not in CordFast yet.
fn quick_actions(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let area = ui.max_rect().shrink(8.0);
    let connected = app.voice.channel.is_some();
    let size = vec2((area.width() - 12.0) / 3.0, 40.0);
    let mut x = area.left();
    let (mute, deaf) = (app.voice.mute, app.voice.deaf);
    let buttons: [(Icon, bool, &str, Option<Action>); 3] = [
        (if mute { Icon::MicrophoneSlash } else { Icon::Microphone }, mute, if mute { t!("Unmute") } else { t!("Mute") }, Some(Action::ToggleMute)),
        (if deaf { Icon::HeadphonesSlash } else { Icon::Headphones }, deaf, if deaf { t!("Undeafen") } else { t!("Deafen") }, Some(Action::ToggleDeaf)),
        (Icon::PhoneCall, false, t!("Disconnect"), connected.then_some(Action::LeaveVoice)),
    ];
    for (index, (icon, on, tooltip, action)) in buttons.into_iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, area.top()), size);
        let enabled = action.is_some();
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let response = ui.interact(rect, ui.id().with(("overlay-action", index)), sense);
        let leave = index == 2;
        let fill = match (leave && enabled, on, response.hovered() && enabled) {
            (true, _, false) => p.red.gamma_multiply(0.8),
            (true, _, true) => p.red,
            (_, true, _) => p.red.gamma_multiply(0.3),
            (_, _, true) => Color32::from_white_alpha(36),
            _ => Color32::from_white_alpha(18),
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        let color = if !enabled { p.faint } else if on { p.red } else { p.strong };
        icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), icon, color);
        if response.on_hover_text(tooltip).clicked()
            && let Some(action) = action
        {
            app.actions.push(action);
        }
        x += size.x + 6.0;
    }
    let status = Rect::from_min_max(pos2(area.left(), area.top() + 48.0), area.max);
    if let Some(channel) = app.voice.channel.and_then(|(_, c)| app.channel(c)) {
        let title = channel.title();
        let green = Rect::from_min_size(status.min, vec2(status.width(), 18.0));
        paint_truncated(ui, green, t!("Voice Connected"), theme::strong(13.0), p.green);
        paint_truncated(ui, Rect::from_min_size(status.min + vec2(0.0, 20.0), vec2(status.width(), 18.0)), &title, FontId::proportional(12.0), p.muted);
    } else {
        paint_truncated(ui, Rect::from_min_size(status.min, vec2(status.width(), 18.0)), t!("No Voice Channel"), FontId::proportional(13.0), p.muted);
    }
}

// ---- notifications -------------------------------------------------------------------

fn notifications(app: &mut App, ui: &mut egui::Ui, locked: bool) {
    let p = theme::p();
    let area = ui.max_rect();
    let mut y = area.top() + 4.0;
    let card = |y: f32| Rect::from_min_size(pos2(area.left() + 4.0, y), vec2(area.width() - 8.0, 64.0));
    if let Some(channel) = app.incoming_call() {
        let rect = card(y);
        ui.painter().rect_filled(rect, 8.0, PANEL);
        let title = app.channel(channel).map(crate::discord::model::Channel::title).unwrap_or_default();
        paint_truncated(ui, Rect::from_min_max(rect.min + vec2(10.0, 8.0), pos2(rect.right() - 80.0, rect.top() + 28.0)), &title, theme::strong(14.0), p.strong);
        paint_truncated(ui, Rect::from_min_max(rect.min + vec2(10.0, 32.0), pos2(rect.right() - 80.0, rect.top() + 52.0)), t!("Incoming Call"), FontId::proportional(13.0), p.muted);
        if !locked {
            let accept = Rect::from_center_size(pos2(rect.right() - 54.0, rect.center().y), Vec2::splat(30.0));
            ui.painter().circle_filled(accept.center(), 15.0, p.green);
            if icon_button(ui, accept, Icon::PhoneCall, true, t!("Join Call"), "overlay-accept") {
                app.actions.push(Action::AcceptCall(channel));
            }
            let decline = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(30.0));
            ui.painter().circle_filled(decline.center(), 15.0, p.red);
            if icon_button(ui, decline, Icon::Close, true, t!("Decline"), "overlay-decline") {
                app.actions.push(Action::DeclineCall(channel));
            }
        }
        y += 68.0;
    }
    let notes: Vec<(String, String, Id, Option<String>)> = app.overlay.notes.iter().map(|note| (note.title.clone(), note.body.clone(), note.channel, note.avatar.clone())).collect();
    for (index, (title, body, channel, avatar)) in notes.into_iter().enumerate() {
        let rect = card(y);
        let response = if locked { None } else { Some(ui.interact(rect, ui.id().with(("overlay-note", index)), Sense::click())) };
        let hovered = response.as_ref().is_some_and(egui::Response::hovered);
        ui.painter().rect_filled(rect, 8.0, if hovered { Color32::from_rgb(38, 40, 44) } else { PANEL });
        let picture = Rect::from_min_size(rect.min + vec2(10.0, 12.0), Vec2::splat(40.0));
        app.images.circle(ui, picture, avatar.as_deref(), &initials(&title), 20.0);
        let x = picture.right() + 10.0;
        paint_truncated(ui, Rect::from_min_max(pos2(x, rect.top() + 10.0), pos2(rect.right() - 8.0, rect.top() + 30.0)), &title, theme::strong(14.0), p.strong);
        paint_truncated(ui, Rect::from_min_max(pos2(x, rect.top() + 32.0), pos2(rect.right() - 8.0, rect.top() + 54.0)), &body, FontId::proportional(13.0), Color32::from_gray(210));
        if response.is_some_and(|response| response.clicked()) {
            app.actions.push(Action::SelectChannel(channel));
            app.settings.overlay_hidden.retain(|id| id != Widget::Chat.id());
        }
        y += 68.0;
    }
    if !locked && y == area.top() + 4.0 {
        ui.painter().text(area.center(), Align2::CENTER_CENTER, t!("No new notifications"), FontId::proportional(13.0), p.muted);
    }
}

// ---- friends and what they play ------------------------------------------------------

/// The friends who are online, the ones in a game first.
fn online(app: &App) -> Vec<(Id, String, String, String, Option<String>)> {
    let mut list: Vec<_> = app
        .relationships
        .values()
        .filter(|relationship| relationship.kind == 1)
        .filter_map(|relationship| {
            let user = &relationship.user;
            let presence = app.presences.get(&user.id)?;
            if presence.status == "offline" || presence.status == "invisible" {
                return None;
            }
            let game = presence.activities.iter().find(|activity| activity.kind == 0).map(|activity| activity.name.clone());
            Some((user.id, user.display_name().to_owned(), user.avatar_url(), presence.status.clone(), game))
        })
        .collect();
    list.sort_by_key(|(_, name, _, _, game)| (game.is_none(), name.to_lowercase()));
    list
}

fn playing(app: &App) -> Vec<(Id, String, String, String, Option<String>)> {
    online(app).into_iter().filter(|(.., game)| game.is_some()).collect()
}

fn friends(app: &mut App, ui: &mut egui::Ui) {
    let rows = online(app);
    person_list(app, ui, rows, t!("No one's around right now."));
}

fn activity(app: &mut App, ui: &mut egui::Ui) {
    let rows = playing(app);
    person_list(app, ui, rows, t!("None of your friends are playing right now."));
}

fn person_list(app: &mut App, ui: &mut egui::Ui, rows: Vec<(Id, String, String, String, Option<String>)>, empty: &str) {
    let p = theme::p();
    let area = ui.max_rect();
    if rows.is_empty() {
        ui.painter().text(area.center(), Align2::CENTER_CENTER, empty, FontId::proportional(13.0), p.muted);
        return;
    }
    egui::ScrollArea::vertical().id_salt("overlay-people").auto_shrink([false, false]).show(ui, |ui| {
        for (id, name, avatar, status, game) in rows {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 6.0, Color32::from_white_alpha(14));
            }
            let picture = Rect::from_min_size(rect.min + vec2(8.0, 8.0), Vec2::splat(32.0));
            app.images.circle(ui, picture, Some(&avatar), &initials(&name), 16.0);
            icons::paint_status(ui, Rect::from_min_size(picture.max - vec2(11.0, 11.0), Vec2::splat(12.0)), &status);
            let x = picture.right() + 10.0;
            let top = if game.is_some() { rect.top() + 6.0 } else { rect.center().y - 10.0 };
            paint_truncated(ui, Rect::from_min_max(pos2(x, top), pos2(rect.right() - 6.0, top + 20.0)), &name, theme::strong(14.0), p.strong);
            if let Some(game) = &game {
                paint_truncated(ui, Rect::from_min_max(pos2(x, top + 20.0), pos2(rect.right() - 6.0, top + 38.0)), game, FontId::proportional(12.0), p.muted);
            }
            if response.clicked() {
                app.actions.push(Action::MessageUser(id));
            }
        }
    });
}

// ---- the bar, the Widgets panel ------------------------------------------------------

/// A pill of the bar: the bundle's `overlay-secondary` and `overlay-primary`.
fn bar_button(ui: &mut egui::Ui, rect: Rect, icon: Icon, text: &str, primary: bool, id: &str) -> bool {
    let p = theme::p();
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    let fill = if primary { p.accent } else if response.hovered() { Color32::from_rgb(58, 60, 66) } else { Color32::from_rgb(40, 42, 47) };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(16.0)), icon, p.strong);
    ui.painter().text(pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(13.0), p.strong);
    response.clicked()
}

fn bar_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(13.0), Color32::WHITE).size().x + 44.0
}

fn bar(app: &mut App, ui: &mut egui::Ui, screen: Rect) {
    let p = theme::p();
    let labels = [t!("Widgets"), t!("Hide All"), t!("Reset Default Layout")];
    let widths: Vec<f32> = labels.iter().map(|label| bar_width(ui, label)).collect();
    let close = 34.0;
    let total = widths.iter().sum::<f32>() + 8.0 * 3.0 + close;
    // Top right: the band over the Friends and Text Chat widgets is free there,
    // while Quick Actions starts at a third of the way along the top.
    let mut x = screen.right() - total - 12.0;
    let y = screen.top() + 12.0;
    let open = app.overlay.panel_open;
    if bar_button(ui, Rect::from_min_size(pos2(x, y), vec2(widths[0], 34.0)), Icon::Overlay, labels[0], open, "overlay-browse") {
        app.overlay.panel_open = !open;
    }
    x += widths[0] + 8.0;
    if bar_button(ui, Rect::from_min_size(pos2(x, y), vec2(widths[1], 34.0)), Icon::EyeSlash, labels[1], false, "overlay-hide-all") {
        app.settings.overlay_hidden = ALL.iter().map(|widget| widget.id().to_owned()).collect();
        app.actions.push(Action::SettingsChanged);
    }
    x += widths[1] + 8.0;
    if bar_button(ui, Rect::from_min_size(pos2(x, y), vec2(widths[2], 34.0)), Icon::Refresh, labels[2], false, "overlay-reset") {
        reset(app);
    }
    x += widths[2] + 8.0;
    let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(close));
    let shortcut = crate::keybinds::bind(&app.settings, crate::keybinds::Shortcut::ToggleOverlayLock).map_or_else(String::new, crate::keybinds::Bind::label);
    let response = ui.interact(rect, ui.id().with("overlay-close"), Sense::click());
    ui.painter().rect_filled(rect, 8.0, if response.hovered() { Color32::from_rgb(58, 60, 66) } else { Color32::from_rgb(40, 42, 47) });
    icons::paint(ui, rect.shrink(9.0), Icon::Close, p.strong);
    if response.on_hover_text(shortcut).clicked() {
        set_locked(app, &ui.ctx().clone(), true);
    }
}

fn reset(app: &mut App) {
    app.settings.overlay_widgets.clear();
    app.settings.overlay_hidden = crate::settings::Settings::default().overlay_hidden;
    app.actions.push(Action::SettingsChanged);
}

fn widgets_panel(app: &mut App, ui: &mut egui::Ui, screen: Rect) {
    let p = theme::p();
    let rect = Rect::from_min_size(pos2(screen.center().x - 190.0, screen.top() + 54.0), vec2(380.0, 68.0 + ALL.len() as f32 * 54.0));
    ui.painter().rect_filled(rect, 10.0, Color32::from_rgb(24, 25, 28));
    ui.painter().rect_stroke(rect, 10.0, Stroke::new(1.0, Color32::from_white_alpha(24)), egui::StrokeKind::Inside);
    // Over the widgets under it, whose clicks it would otherwise leave through.
    let _ = ui.interact(rect, ui.id().with("overlay-panel"), Sense::click());
    ui.painter().text(rect.min + vec2(16.0, 14.0), Align2::LEFT_TOP, t!("Widgets"), theme::strong(16.0), p.strong);
    ui.painter().text(rect.min + vec2(16.0, 38.0), Align2::LEFT_TOP, t!("Choose what shows up in your overlay."), FontId::proportional(13.0), p.muted);
    let mut y = rect.top() + 68.0;
    for widget in ALL {
        let row = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 50.0));
        let on = !app.settings.overlay_hidden.iter().any(|id| id == widget.id());
        let response = ui.interact(row, ui.id().with(("overlay-toggle", widget.id())), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(row, 6.0, Color32::from_white_alpha(14));
        }
        icons::paint(ui, Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), Vec2::splat(20.0)), widget.icon(), p.strong);
        let right = row.right() - 54.0;
        paint_truncated(ui, Rect::from_min_max(pos2(row.left() + 40.0, row.top() + 6.0), pos2(right, row.top() + 26.0)), widget.label(), theme::strong(14.0), p.strong);
        paint_truncated(ui, Rect::from_min_max(pos2(row.left() + 40.0, row.top() + 26.0), pos2(right, row.top() + 44.0)), widget.description(), FontId::proportional(12.0), p.muted);
        let track = Rect::from_center_size(pos2(row.right() - 26.0, row.center().y), vec2(36.0, 20.0));
        ui.painter().rect_filled(track, 10.0, if on { p.accent } else { Color32::from_gray(80) });
        ui.painter().circle_filled(pos2(if on { track.right() - 10.0 } else { track.left() + 10.0 }, track.center().y), 8.0, Color32::WHITE);
        if response.clicked() {
            set_hidden(app, widget, on);
        }
        y += 54.0;
    }
}

fn set_hidden(app: &mut App, widget: Widget, hide: bool) {
    app.settings.overlay_hidden.retain(|id| id != widget.id());
    if hide {
        app.settings.overlay_hidden.push(widget.id().to_owned());
    }
    app.actions.push(Action::SettingsChanged);
}

fn empty_state(app: &mut App, ui: &mut egui::Ui, screen: Rect) {
    let p = theme::p();
    let center = screen.center();
    ui.painter().text(center - vec2(0.0, 32.0), Align2::CENTER_CENTER, t!("No widgets on screen"), theme::strong(20.0), p.strong);
    ui.painter().text(center, Align2::CENTER_CENTER, t!("Open Widgets to choose what shows up."), FontId::proportional(14.0), p.muted);
    let width = bar_width(ui, t!("Widgets"));
    if bar_button(ui, Rect::from_center_size(center + vec2(0.0, 40.0), vec2(width, 34.0)), Icon::Overlay, t!("Widgets"), true, "overlay-empty-browse") {
        app.overlay.panel_open = true;
    }
}

// ---- Settings â†’ Game Overlay ---------------------------------------------------------

pub fn settings_page(app: &mut App, ui: &mut egui::Ui) {
    if !cfg!(windows) {
        form::unavailable(ui, t!("Overlay is not available on this system."));
        return;
    }
    form::category(ui, t!("Overview"), "", |ui| {
        form::switch_row(ui, &mut app.settings.overlay_enabled, t!("Enable Overlay"), t!("Use Discord overlay in-game"));
        ui.add_space(8.0);
        let front = detect::front();
        match (&front, app.settings.overlay_enabled) {
            (_, false) => form::note(ui, t!("Overlay is disabled for all games.")),
            (Some(front), true) => {
                form::note(ui, &front.name);
                form::note(ui, t!("Overlay: On"));
                if front.exclusive {
                    form::note(ui, t!("You must run this game in borderless windowed mode to use the new Overlay."));
                }
            }
            (None, true) => form::note(ui, t!("It appears over a game you play in borderless or windowed mode, and never touches the game itself.")),
        }
    });
    form::category(ui, t!("Toggle Overlay Lock"), t!("Toggle overlay interactivity lock."), |ui| {
        let shortcut = crate::keybinds::bind(&app.settings, crate::keybinds::Shortcut::ToggleOverlayLock).map_or_else(|| t!("None").to_owned(), crate::keybinds::Bind::label);
        form::label(ui, t!("Keybind"));
        form::note(ui, &shortcut);
    });
    form::category(ui, t!("Widgets"), t!("Choose what shows up in your overlay."), |ui| {
        for widget in ALL {
            let mut on = !app.settings.overlay_hidden.iter().any(|id| id == widget.id());
            if form::switch_row(ui, &mut on, widget.label(), widget.description()) {
                set_hidden(app, widget, !on);
            }
        }
        ui.add_space(8.0);
        if form::button(ui, t!("Reset Default Layout"), form::Kind::Secondary).clicked() {
            reset(app);
        }
    });
    form::category(ui, t!("Voice Widget"), "", |ui| {
        form::label(ui, t!("Avatar Size"));
        let sizes = [t!("Large").to_owned(), t!("Small").to_owned()];
        let shown = if app.settings.overlay_avatar_large { sizes[0].clone() } else { sizes[1].clone() };
        if let Some(picked) = form::select(ui, "overlay-avatar", &shown, &sizes, 240.0) {
            app.settings.overlay_avatar_large = picked == 0;
        }
        ui.add_space(12.0);
        form::label(ui, t!("Display Names"));
        let names = [t!("Always").to_owned(), t!("Only While Speaking").to_owned(), t!("Never").to_owned()];
        let shown = names[usize::from(app.settings.overlay_names.min(2))].clone();
        if let Some(picked) = form::select(ui, "overlay-names", &shown, &names, 240.0) {
            app.settings.overlay_names = picked as u8;
        }
        ui.add_space(12.0);
        form::label(ui, t!("Display Users"));
        let users = [t!("Always").to_owned(), t!("Only While Speaking").to_owned()];
        let shown = users[usize::from(app.settings.overlay_users.min(1))].clone();
        if let Some(picked) = form::select(ui, "overlay-users", &shown, &users, 240.0) {
            app.settings.overlay_users = picked as u8;
        }
        ui.add_space(12.0);
        form::label(ui, t!("Max Users Displayed"));
        let mut max = i32::from(app.settings.overlay_max_users);
        ui.spacing_mut().slider_width = 240.0;
        if ui.add(egui::Slider::new(&mut max, 0..=25).custom_formatter(|value, _| if value < 1.0 { t!("Off").to_owned() } else { format!("{value:.0}") })).changed() {
            app.settings.overlay_max_users = max as u8;
        }
    });
    form::category(ui, t!("Notifications"), "", |ui| {
        form::switch_row(ui, &mut app.settings.overlay_notify_messages, t!("Messages"), t!("For each text message notification, shows a notice that you can reply to."));
    });
}
