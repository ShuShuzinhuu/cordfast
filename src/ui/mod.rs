//! Views. They read [`App`] and push [`Action`]s; only their own fields
//! (composer, token box, settings being edited) change in place.

pub mod activity;
mod chat;
mod commands;
mod components;
pub mod emoji;
mod expressions;
mod form;
pub mod friends;
mod guild;
pub mod markdown;
pub mod menu;
mod profile;
mod settings;
pub mod gates;
mod guild_more;
pub mod overlay;
pub mod soundboard;
pub mod splash;
mod requests;
pub mod form_modal;
mod titlebar;

use egui::{
    Align, Align2, Color32, FontId, Frame, Key, Label, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, Stroke, Vec2, pos2, vec2,
};

use crate::app::{Action, App, AuthView, Connection, SettingsTab};
use crate::icons::{self, Icon};
use crate::discord::model::{Channel, ChannelKind, Id};
use crate::plugins::Plugin;
use crate::theme;

/// Discord's rail since the title bar came: 40-point icons 12 from each side.
const RAIL_WIDTH: f32 = 64.0;
/// A server's icon in the rail.
const RAIL_ICON: f32 = 40.0;
/// The channel list's width, from Discord's own to the widest it drags.
const SIDEBAR_WIDTH: std::ops::RangeInclusive<f32> = 240.0..=480.0;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    // The account's display settings the drawing reads.
    {
        use crate::discord::preloaded as pre;
        use std::sync::atomic::Ordering;
        menu::DEVELOPER_MODE.store(app.preloaded.flag(pre::DEVELOPER_MODE), Ordering::Relaxed);
        crate::text::HOUR_CYCLE.store(app.preloaded.number(pre::HOUR_CYCLE).min(2) as u8, Ordering::Relaxed);
        crate::discord::model::ANIMATE_EMOJI.store(app.preloaded.flag(pre::ANIMATE_EMOJI), Ordering::Relaxed);
    }
    titlebar::show(app, ui);
    voice_notice(app, ui);
    if !matches!(app.auth, AuthView::Checking) {
        app.loading = None;
    }
    if matches!(app.auth, AuthView::Checking) {
        loading(app, ui);
    } else if !matches!(app.auth, AuthView::LoggedIn) {
        login(app, ui);
    } else if let Some(tab) = app.page {
        // Discord's settings are a window over the app: the app stays drawn
        // behind it, without the keyboard, which the window takes on opening.
        let below = ui.available_rect_before_wrap();
        let opened = egui::Id::new("settings-open");
        if !ui.ctx().data(|d| d.get_temp::<bool>(opened)).unwrap_or(false) {
            ui.ctx().data_mut(|d| d.insert_temp(opened, true));
            ui.ctx().memory_mut(|m| {
                if let Some(focused) = m.focused() {
                    m.surrender_focus(focused);
                }
            });
        }
        app.focus_composer = false;
        main(app, ui);
        settings::show(app, ui, tab, below);
        chat::modal(app, ui.ctx());
    } else if app.admin.is_some() {
        guild::show(app, ui);
    } else {
        main(app, ui);
        profile::show(app, ui.ctx());
        profile::tag_card(app, ui.ctx());
        chat::viewer(app, ui.ctx());
        expressions::show(app, ui.ctx());
        chat::modal(app, ui.ctx());
        incoming_call(app, ui.ctx());
        switcher(app, ui.ctx());
        form_modal::show(app, ui.ctx());
    }
    if app.page.is_none() {
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("settings-open"), false));
    }
    titlebar::inbox(app, ui.ctx());
    if let Some((notice, _)) = &app.notice {
        let notice = notice.clone();
        let p = theme::p();
        egui::Area::new("notice".into())
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -96.0))
            .order(egui::Order::Tooltip)
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(p.rail)
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| ui.label(RichText::new(notice).color(p.strong)));
            });
    }
    // The window's edges resize it, as the system frame's would.
    fastframe_titlebar::resize(ui);
}

/// Discord's red bar over the app when the call went on without us:
/// another device of the account joined voice, or the call ended. Its
/// "Reconnect" joins the same channel again.
fn voice_notice(app: &mut App, ui: &mut egui::Ui) {
    let Some(notice) = app.voice_notice else { return };
    let p = theme::p();
    egui::Panel::top("voice-notice").exact_size(32.0).show_separator_line(false).frame(Frame::new().fill(p.red.gamma_multiply(0.35))).show(ui, |ui| {
        let rect = ui.max_rect();
        let text = if notice.elsewhere { t!("Your voice has been disconnected because you connected at another location.") } else { t!("Your voice has been disconnected.") };
        let message = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(14.0), p.strong);
        let link = ui.painter().layout_no_wrap(t!("Reconnect").to_owned(), theme::strong(14.0), p.strong);
        let width = message.size().x + 16.0 + link.size().x;
        let x = rect.center().x - width / 2.0;
        ui.painter().galley(pos2(x, rect.center().y - message.size().y / 2.0), message.clone(), p.strong);
        let link_rect = Rect::from_min_size(pos2(x + message.size().x + 16.0, rect.center().y - link.size().y / 2.0), link.size());
        let reconnect = ui.interact(link_rect.expand(4.0), ui.id().with("voice-notice-reconnect"), Sense::click());
        ui.painter().galley(link_rect.min, link, p.strong);
        if reconnect.hovered() {
            ui.painter().hline(link_rect.x_range(), link_rect.bottom(), Stroke::new(1.0, p.strong));
        }
        if reconnect.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.voice_notice = None;
            app.actions.push(Action::JoinVoice { guild: notice.guild, channel: notice.channel, ring: false });
        }
        let close = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(20.0));
        let response = ui.interact(close, ui.id().with("voice-notice-close"), Sense::click());
        icons::paint(ui, close.shrink(3.0), Icon::Close, if response.hovered() { p.strong } else { p.text });
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.voice_notice = None;
        }
    });
}

pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(3)
        .collect()
}

// ---- loading -----------------------------------------------------------------

/// The loading screen's tips, one picked each time it shows: Discord's own
/// (the bundle's `_loadingText`), those this client does and its facts. The
/// second is the keys its field stands for, where Discord draws key chips.
const TIPS: [(&str, &str); 24] = [
    (n!("You can type /tableflip and /unflip to spice up your messages."), ""),
    (n!("You can type {asterisks} around your words to make them bold."), "**"),
    (n!("{quickSwitcherHook} to quickly find a previous conversation or channel."), "Ctrl + K"),
    (n!("{markServerUnreadHook} to mark an entire server as read."), "Shift + Esc"),
    (n!("Use {keyboardShortcutsHook} to bring up the list of keyboard shortcuts."), "Ctrl + /"),
    (n!("{messageNewlineHook} to make a new line without sending your message."), "Shift + Enter"),
    (n!("You can press {upHook} to quickly edit your most recent message."), "↑"),
    (n!("You can drag and drop files onto Discord to upload them."), ""),
    (n!("Change each participant's volume by right-clicking them in a call."), ""),
    (n!("Right click to pin messages in a channel or DM to save them for later."), ""),
    (n!("You can temporarily mute a server or channel by right-clicking it."), ""),
    (n!("Click your avatar in the lower-left corner to set a custom status."), ""),
    (n!("The top-most role for a user defines that user's color."), ""),
    (n!("You can type / to view bot commands and other built-in commands"), ""),
    (n!("Click a server name in the emoji picker to hide that server's emoji."), ""),
    (n!("Discord was almost called Bonfire before we picked our name. It was meant to be nice and cozy."), ""),
    (n!("Discord was almost called Wyvern before we picked our name. Not too proud of that one."), ""),
    (n!("Discord started as a game company making a mobile game called Fates Forever."), ""),
    (n!("Discord’s official birthday is May 13, 2015."), ""),
    (n!("We came up with the idea of Discord Nitro over morning breakfast potatoes."), ""),
    (n!("Our mascot, Wumpus, was originally created as a character with no friends :("), ""),
    (n!("In Discord's early days, light theme was the only theme. Dark theme didn't arrive until August 2015."), ""),
    (n!("In the ancient days, Discord started as a browser-only app."), ""),
    (n!("The character on our 404 page is a robot hamster named Nelly."), ""),
];

/// A tip in the interface's language, its key field filled.
fn tip_text(index: usize) -> String {
    let (text, keys) = TIPS[index % TIPS.len()];
    let text = crate::i18n::tr(text);
    match (text.find('{'), text.find('}')) {
        (Some(open), Some(close)) if close > open => format!("{}{keys}{}", &text[..open], &text[close + 1..]),
        _ => text.to_owned(),
    }
}

/// Discord's loading screen while a session starts, instead of the login
/// one: its logo spinning like a coin, "Did you know" and a tip, and after
/// a while where to look if the connection does not come.
fn loading(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let (since, tip) = *app.loading.get_or_insert_with(|| {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        (std::time::Instant::now(), seed % TIPS.len())
    });
    let elapsed = since.elapsed().as_secs_f32();
    egui::CentralPanel::default().frame(Frame::new().fill(p.chat)).show(ui, |ui| {
        let rect = ui.max_rect();
        let logo = Rect::from_center_size(rect.center() - vec2(0.0, 70.0), Vec2::splat(80.0));
        // A turn eased in and out every two seconds, then a rest.
        let phase = (elapsed % 2.0 / 0.8).min(1.0);
        let eased = phase * phase * (3.0 - 2.0 * phase);
        icons::paint_turned(ui, logo, Icon::Discord, p.strong, (eased * std::f32::consts::TAU).cos());
        let title = t!("Did you know").to_uppercase();
        ui.painter().text(pos2(rect.center().x, logo.bottom() + 36.0), Align2::CENTER_TOP, title, theme::strong(12.0), p.muted);
        // Centred line by line, as Discord's tip is.
        let mut job = egui::text::LayoutJob::simple(tip_text(tip), FontId::proportional(16.0), p.text, 380.0);
        job.halign = Align::Center;
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        ui.painter().galley(pos2(rect.center().x, logo.bottom() + 58.0), galley, p.text);
        if elapsed > 10.0 {
            let y = rect.bottom() - 70.0;
            ui.painter().text(pos2(rect.center().x, y), Align2::CENTER_CENTER, t!("Connection problems? Let us know!"), FontId::proportional(14.0), p.muted);
            let links = [(t!("Tweet Us"), "https://x.com/discord"), (t!("Server Status"), "https://discordstatus.com")];
            let widths: Vec<f32> = links.iter().map(|(label, _)| ui.painter().layout_no_wrap((*label).to_owned(), theme::strong(14.0), p.link).size().x).collect();
            let total = widths.iter().sum::<f32>() + 32.0;
            let mut x = rect.center().x - total / 2.0;
            for ((label, url), width) in links.iter().zip(widths) {
                let area = Rect::from_min_size(pos2(x, y + 16.0), vec2(width, 20.0));
                let response = ui.interact(area, ui.id().with(("loading-link", *url)), Sense::click());
                ui.painter().text(area.left_center(), Align2::LEFT_CENTER, *label, theme::strong(14.0), p.link);
                if response.hovered() {
                    ui.painter().hline(area.x_range(), area.bottom() - 2.0, Stroke::new(1.0, p.link));
                }
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    let _ = open::that_detached(url);
                }
                x += width + 32.0;
            }
        }
    });
    // Only this screen animates; it goes once the session is ready. At 30
    // frames a second: egui repaints at once when asked for less than a frame's
    // time away, and with the window still appearing nothing held it to the
    // screen's rate, which made over a thousand frames a second at startup.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
}

// ---- login -------------------------------------------------------------------

fn login(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p.rail))
        .show(ui, |ui| {
            // Discord's login card: the form on the left, the QR code on the right.
            let card = vec2(784.0, 420.0);
            let rect = Rect::from_center_size(ui.max_rect().center(), card);
            // The card stands off the background in every theme (AMOLED too).
            ui.painter().rect(rect, 8.0, p.sidebar, Stroke::new(1.0, p.selected), egui::StrokeKind::Inside);
            let form_rect = Rect::from_min_size(rect.min + vec2(32.0, 32.0), vec2(440.0, card.y - 64.0));
            let qr_rect = Rect::from_min_size(rect.min + vec2(520.0, 32.0), vec2(232.0, card.y - 64.0));
            let mut left = ui.new_child(egui::UiBuilder::new().max_rect(form_rect).layout(Layout::top_down(Align::Center)));
            login_form(app, &mut left);
            let x = rect.left() + 496.0;
            ui.painter().vline(x, egui::Rangef::new(rect.top() + 48.0, rect.bottom() - 48.0), Stroke::new(1.0, p.selected));
            let mut right = ui.new_child(egui::UiBuilder::new().max_rect(qr_rect).layout(Layout::top_down(Align::Center)));
            qr_side(app, &mut right);
        });
}

fn login_form(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let field = |ui: &mut egui::Ui, label: &str, value: &mut String, password: bool| -> egui::Response {
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.label(RichText::new(label).font(theme::strong(12.0)).color(p.muted));
            ui.add(
                egui::TextEdit::singleline(value)
                    .password(password)
                    .margin(Margin::symmetric(10, 9))
                    .desired_width(f32::INFINITY),
            )
        })
        .inner
    };
    let busy = app.login_form.busy;
    if let Some((totp, sms, backup)) = app.login_form.mfa {
        ui.label(RichText::new(t!("Multi-Factor Authentication")).font(theme::strong(22.0)).color(p.strong));
        let hint = match app.login_form.mfa_kind {
            "sms" => match &app.login_form.sms_phone {
                Some(phone) => t!("Enter the code sent by SMS to {phone}.", phone = phone),
                None => t!("Sending the code by SMS…").into(),
            },
            "backup" => t!("Enter one of your 8-digit backup codes.").into(),
            _ => t!("Enter the 6-digit code from your authenticator app.").into(),
        };
        ui.label(RichText::new(hint).color(p.muted));
        ui.add_space(14.0);
        let response = field(ui, &t!("Code").to_uppercase(), &mut app.login_form.code, false);
        // Read before asking for the focus back: asking first hides that
        // Enter just took it away.
        let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        response.request_focus();
        ui.add_space(12.0);
        let button = egui::Button::new(RichText::new(if busy { t!("Verifying…") } else { t!("Confirm") }).color(Color32::WHITE))
            .fill(p.accent)
            .min_size(vec2(ui.available_width(), 40.0));
        if (ui.add_enabled(!busy, button).clicked() || enter) && !busy {
            app.actions.push(Action::MfaCode);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (available, kind, label) in [(totp, "totp", t!("Use your authenticator app")), (sms, "sms", t!("Receive the code by SMS")), (backup, "backup", t!("Use a backup code"))] {
                if available && app.login_form.mfa_kind != kind && ui.link(RichText::new(label).size(13.0)).clicked() {
                    app.actions.push(Action::MfaKind(kind));
                }
            }
        });
        if ui.link(RichText::new(t!("Go Back")).size(13.0)).clicked() {
            app.login_form.mfa = None;
        }
    } else {
        ui.label(RichText::new(t!("Welcome back!")).font(theme::strong(24.0)).color(p.strong));
        ui.label(RichText::new(t!("We're so excited to see you again!")).color(p.muted));
        ui.add_space(18.0);
        let login = field(ui, &t!("Email or Phone Number").to_uppercase(), &mut app.login_form.login, false);
        ui.add_space(12.0);
        let password = field(ui, &t!("Password").to_uppercase(), &mut app.login_form.password, true);
        let enter = (login.lost_focus() || password.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            if ui.link(RichText::new(t!("Forgot your password?")).size(13.0)).clicked() {
                let _ = open::that_detached("https://discord.com/login");
            }
        });
        ui.add_space(12.0);
        let button = egui::Button::new(RichText::new(if busy { t!("Logging In…") } else { t!("Log In") }).color(Color32::WHITE))
            .fill(p.accent)
            .min_size(vec2(ui.available_width(), 40.0));
        if (ui.add_enabled(!busy, button).clicked() || enter) && !busy {
            app.actions.push(Action::PasswordLogin);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut app.token_input)
                .password(true)
                .hint_text(t!("or paste a token"))
                .desired_width(ui.available_width() - 70.0);
            let response = ui.add(field);
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button(t!("Log In")).clicked() || enter {
                app.actions.push(Action::TokenLogin);
            }
        });
    }
    if let Some(error) = &app.login_form.error {
        ui.add_space(8.0);
        ui.label(RichText::new(error).color(p.red));
    }
}

fn qr_side(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    match &app.auth {
        AuthView::Checking | AuthView::Generating => {
            ui.add_space(70.0);
            ui.spinner();
        }
        AuthView::Code { modules, width } => {
            qr(ui, modules, *width, 176.0);
        }
        AuthView::Scanned(name) => {
            ui.add_space(40.0);
            ui.label(RichText::new(t!("Check your phone!")).font(theme::strong(18.0)).color(p.strong));
            if !name.is_empty() {
                ui.label(RichText::new(t!("Logging in as {name}", name = name)).color(p.muted));
            }
            ui.spinner();
        }
        AuthView::Failed(error) => {
            ui.add_space(30.0);
            ui.label(RichText::new(error).color(p.red));
            if ui.button(t!("Generate a New Code")).clicked() {
                app.actions.push(Action::NewQr);
            }
        }
        AuthView::LoggedIn => {}
    }
    ui.add_space(16.0);
    ui.label(RichText::new(t!("Log in with QR Code")).font(theme::strong(18.0)).color(p.strong));
    ui.label(RichText::new(t!("Scan this with the Discord mobile app to log in instantly.")).color(p.muted));
}

fn qr(ui: &mut egui::Ui, modules: &[bool], width: usize, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, Color32::WHITE);
    let quiet = 2.0;
    let cell = size / (width as f32 + quiet * 2.0);
    for (i, dark) in modules.iter().enumerate() {
        if *dark {
            let x = (i % width) as f32 + quiet;
            let y = (i / width) as f32 + quiet;
            let min = rect.min + vec2(x * cell, y * cell);
            painter.rect_filled(Rect::from_min_size(min, Vec2::splat(cell + 0.3)), 0.0, Color32::BLACK);
        }
    }
}

// ---- main window ---------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum Side {
    Left,
    Right,
}

/// A 1 px line on one inner edge of a panel, drawn only when the panel and
/// its neighbour share a colour: with every background black (AMOLED) it is
/// what parts the areas, as the shades do in the other themes.
pub fn seam(ui: &egui::Ui, rect: Rect, side: Side, inside: Color32, outside: Color32) {
    if inside != outside {
        return;
    }
    let (a, b) = match side {
        Side::Left => (rect.left_top() + vec2(0.5, 0.0), rect.left_bottom() + vec2(0.5, 0.0)),
        Side::Right => (rect.right_top() - vec2(0.5, 0.0), rect.right_bottom() - vec2(0.5, 0.0)),
    };
    ui.painter().line_segment([a, b], Stroke::new(1.0, theme::p().border));
}

fn main(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    if app.page.is_none() && crate::keybinds::pressed(ui, &app.settings, crate::keybinds::Shortcut::QuickSwitcher) {
        app.switcher = Some(String::new());
    }
    if app.page.is_none() {
        shortcuts(app, ui);
    }
    // Discord's account panel: a card over the bottom of the rail and the
    // channel list together, so the name has room; both columns end above it.
    let card = account_card_height(app);
    let rail_panel = egui::Panel::left("rail")
        .resizable(false)
        .exact_size(RAIL_WIDTH)
        .show_separator_line(false)
        .frame(Frame::new().fill(p.rail).inner_margin(Margin { left: 12, right: 12, top: 0, bottom: 0 }))
        .show(ui, |ui| {
            egui::Panel::bottom("rail-under-card").exact_size(card + 16.0).show_separator_line(false).frame(Frame::NONE).show(ui, |_| {});
            rail(app, ui);
        });
    let width = app.settings.sidebar_width.clamp(*SIDEBAR_WIDTH.start(), *SIDEBAR_WIDTH.end());
    let sidebar_panel = egui::Panel::left("sidebar")
        .resizable(false)
        .exact_size(width)
        .show_separator_line(false)
        .frame(Frame::new().fill(p.sidebar))
        .show(ui, |ui| sidebar(app, ui));
    seam(ui, sidebar_panel.response.rect, Side::Right, p.sidebar, p.chat);
    egui::CentralPanel::default()
        .frame(Frame::new().fill(p.chat))
        .show(ui, |ui| chat::show(app, ui));
    sidebar_edge(app, ui, sidebar_panel.response.rect, width);
    let bottom = sidebar_panel.response.rect.bottom();
    content_edge(ui, sidebar_panel.response.rect.left_top(), bottom - card - 16.0, app.page.is_some());
    let area = Rect::from_min_max(pos2(rail_panel.response.rect.left() + 8.0, bottom - card - 8.0), pos2(sidebar_panel.response.rect.right() - 8.0, bottom - 8.0));
    account_card(app, ui.ctx(), area);
}

/// The channel list's edge, as Discord's: dragged, it makes the list wider
/// or narrower; a line marks it while pointed at. The width is saved when
/// the drag ends.
fn sidebar_edge(app: &mut App, ui: &mut egui::Ui, sidebar: Rect, width: f32) {
    let p = theme::p();
    let edge = Rect::from_x_y_ranges(sidebar.right() - 2.0..=sidebar.right() + 4.0, sidebar.y_range());
    let handle = ui.interact(edge, egui::Id::new("sidebar-edge"), Sense::drag());
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ui.painter().vline(sidebar.right(), sidebar.y_range(), Stroke::new(2.0, p.muted));
    }
    if handle.dragged() {
        app.settings.sidebar_width = (width + handle.drag_delta().x).clamp(*SIDEBAR_WIDTH.start(), *SIDEBAR_WIDTH.end());
    }
    if handle.drag_stopped() {
        app.actions.push(Action::SettingsChanged);
    }
}

/// The account card's height: the account row, and the voice panel over it
/// while in a call.
fn account_card_height(app: &App) -> f32 {
    if app.voice.channel.is_some() { 112.0 } else { 52.0 }
}

/// The card itself: rounded, a line around it, the voice panel and the
/// account row inside, as Discord's.
fn account_card(app: &mut App, ctx: &egui::Context, area: Rect) {
    let p = theme::p();
    let in_voice = app.voice.channel.is_some();
    egui::Area::new(egui::Id::new("account-card"))
        .fixed_pos(area.min)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            ui.data_mut(|d| d.insert_temp(egui::Id::new("account-card-rect"), area));
            Frame::new()
                .fill(p.account)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(area.width() - 16.0);
                    ui.set_height(area.height() - 14.0);
                    if in_voice {
                        voice_panel(app, ui);
                    }
                    ui.allocate_ui_with_layout(vec2(ui.available_width(), 40.0), Layout::left_to_right(Align::Center), |ui| account(app, ui));
                });
        });
}

/// Where the content meets the title bar and the rail, as Discord's: a
/// rounded corner and a 1-point line along the top and the left, in the
/// line colour (the rail's in the dark theme, a grey on AMOLED black).
fn content_edge(ui: &egui::Ui, corner: egui::Pos2, left_end: f32, covered: bool) {
    let p = theme::p();
    const RADIUS: f32 = 8.0;
    // A layer egui hasn't ordered paints over every window of its order, so
    // under the settings window the edge stays on the panels' own layer.
    let layer = if covered { ui.layer_id() } else { egui::LayerId::new(egui::Order::Middle, egui::Id::new("content-edge")) };
    let painter = ui.ctx().layer_painter(layer);
    let screen = ui.ctx().content_rect();
    // The rail's colour outside the curve, the sidebar's inside it.
    painter.rect_filled(Rect::from_min_size(corner, Vec2::splat(RADIUS)), 0.0, p.rail);
    painter.circle_filled(corner + Vec2::splat(RADIUS), RADIUS, p.sidebar);
    let x = corner.x + 0.5;
    let y = corner.y + 0.5;
    let _ = screen;
    let mut points = vec![pos2(x, left_end)];
    for step in 0..=8 {
        let angle = std::f32::consts::PI * (1.0 + step as f32 / 16.0);
        points.push(pos2(x + RADIUS + RADIUS * angle.cos(), y + RADIUS + RADIUS * angle.sin()));
    }
    points.push(pos2(ui.ctx().content_rect().right(), y));
    painter.add(egui::Shape::line(points, Stroke::new(1.0, p.border)));
}

/// A rail button, at the size Discord draws it now (40 points, 8 apart): a
/// circle that rounds into a square when selected or hovered, with
/// Discord's pill for unread and selected.
#[allow(clippy::too_many_arguments)]
fn rail_item(
    app: &mut App,
    ui: &mut egui::Ui,
    selected: bool,
    unread: bool,
    mentions: u32,
    tooltip: &str,
    paint: impl FnOnce(&mut App, &egui::Ui, Rect, f32),
) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(RAIL_ICON, RAIL_ICON), Sense::click());
    let hovered = response.hovered();
    let rounding = if selected || hovered { 13.0 } else { RAIL_ICON / 2.0 };
    paint(app, ui, rect, rounding);
    let pill_height = if selected {
        40.0
    } else if hovered {
        20.0
    } else if unread {
        8.0
    } else {
        0.0
    };
    if pill_height > 0.0 {
        let pill = Rect::from_center_size(pos2(rect.left() - 12.0, rect.center().y), vec2(8.0, pill_height));
        ui.painter().rect_filled(pill, 4.0, p.strong);
    }
    if mentions > 0 {
        badge(ui, pos2(rect.right() - 6.0, rect.bottom() - 6.0), mentions, p.rail);
    }
    response.on_hover_text(tooltip)
}

pub fn badge(ui: &egui::Ui, center: egui::Pos2, count: u32, ring: Color32) {
    let p = theme::p();
    let label = if count > 99 { "99+".to_owned() } else { count.to_string() };
    let width = 16.0f32.max(8.0 + 7.0 * label.len() as f32);
    let rect = Rect::from_center_size(center, vec2(width, 16.0));
    ui.painter().rect(rect.expand(3.0), 11.0, ring, Stroke::NONE, egui::StrokeKind::Inside);
    ui.painter().rect_filled(rect, 8.0, p.red);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::strong(11.0), Color32::WHITE);
}

enum RailEntry {
    Guild(Id),
    Folder { id: i64, name: Option<String>, color: Option<u32>, guilds: Vec<Id> },
}

/// The server list in the user's order: new servers first, then the
/// folders and loose servers from the settings protobuf.
fn rail_entries(app: &App) -> Vec<RailEntry> {
    let known: std::collections::HashSet<Id> = app.guilds.iter().map(|g| g.id).collect();
    let mut placed = std::collections::HashSet::new();
    let mut entries = Vec::new();
    if let Some(folders) = &app.folders {
        for folder in &folders.folders {
            let guilds: Vec<Id> = folder.guild_ids.iter().copied().filter(|g| known.contains(g)).collect();
            placed.extend(guilds.iter().copied());
            match folder.id {
                Some(id) if !guilds.is_empty() && (guilds.len() > 1 || folder.name.is_some()) => {
                    entries.push(RailEntry::Folder { id, name: folder.name.clone(), color: folder.color, guilds });
                }
                _ => entries.extend(guilds.into_iter().map(RailEntry::Guild)),
            }
        }
    }
    let mut new: Vec<RailEntry> = app
        .guilds
        .iter()
        .filter(|g| !placed.contains(&g.id))
        .map(|g| RailEntry::Guild(g.id))
        .collect();
    new.append(&mut entries);
    new
}

fn guild_button(app: &mut App, ui: &mut egui::Ui, id: Id) {
    let Some(guild) = app.guild(id) else { return };
    let (name, icon) = (guild.name.clone(), guild.icon_url());
    let (unread, mentions) = app.guild_unread(guild);
    let selected = app.selected_guild == Some(id);
    let response = rail_item(app, ui, selected, unread, mentions, &name, |app, ui, rect, r| {
        app.images.circle(ui, rect, icon.as_deref(), &initials(&name), r);
    });
    voice_badge(app, ui, response.rect, &[id]);
    if response.clicked() {
        app.actions.push(Action::SelectGuild(Some(id)));
    }
    guild_menu(app, &response, id);
}

/// Discord's upper badge on a server (or closed folder): a speaker while
/// someone is in one of its voice channels, on green when we are.
fn voice_badge(app: &App, ui: &egui::Ui, icon: Rect, guilds: &[Id]) {
    let p = theme::p();
    let ours = app.voice.channel.is_some_and(|(guild, _)| guild.is_some_and(|g| guilds.contains(&g)));
    if !ours && !guilds.iter().filter_map(|g| app.guild(*g)).any(|g| !g.voice_states.is_empty()) {
        return;
    }
    let center = pos2(icon.right() - 4.0, icon.top() + 4.0);
    ui.painter().circle_filled(center, 10.0, p.rail);
    if ours {
        ui.painter().circle_filled(center, 7.5, p.green);
    }
    icons::paint(ui, Rect::from_center_size(center, Vec2::splat(11.0)), Icon::Voice, if ours { Color32::WHITE } else { p.strong });
}

/// Asks Discord for an invite to `channel`; the answer goes to the
/// clipboard (see the "invite" answer in app.rs).
fn create_invite(app: &mut App, channel: Id) {
    // Streamer Mode: links stay off the screen and the clipboard's way.
    if app.streamer_on() && app.settings.streamer_hide_invites {
        app.notify(t!("Hide Invite Links"));
        return;
    }
    app.commands.send(crate::backend::Command::Api {
        key: "invite".into(),
        method: "POST",
        path: format!("/channels/{channel}/invites"),
        body: Some(serde_json::json!({ "max_age": 604800, "max_uses": 0 })),
    });
}

/// The channel an invite to a server leads to: the open one, or the first
/// text channel.
fn invite_channel(app: &App, guild: Id) -> Option<Id> {
    app.selected_channel
        .filter(|c| app.channel(*c).is_some_and(|c| c.guild_id == Some(guild)))
        .or_else(|| app.guild(guild)?.channels.iter().find(|c| c.kind == ChannelKind::Text).map(|c| c.id))
}

/// "Hide Muted Channels", a check per server.
fn hide_muted_item(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    let key = guild.to_string();
    let hidden = app.settings.hide_muted.contains(&key);
    if menu::item(ui, t!("Hide Muted Channels"), menu::Tail::Check(hidden)).clicked() {
        if hidden {
            app.settings.hide_muted.retain(|g| *g != key);
        } else {
            app.settings.hide_muted.push(key);
        }
        app.actions.push(Action::SettingsChanged);
    }
}

const CREATE_INVITE: u64 = 1;
const MANAGE_CHANNELS: u64 = 1 << 4;
/// Setting the status of the voice channel you are in.
const SET_VOICE_CHANNEL_STATUS: u64 = 1 << 48;
const MANAGE_GUILD: u64 = 1 << 5;
const MANAGE_ROLES: u64 = 1 << 28;

/// The menu under the server's name, as in Discord.
fn server_menu(app: &mut App, response: &egui::Response, id: Id) {
    let Some(guild) = app.guild(id) else { return };
    let me = app.me_id();
    let perms = guild.my_permissions(me);
    let owner = guild.owner_id == Some(me);
    let invite = invite_channel(app, id);
    egui::Popup::menu(response).show(|ui| {
        menu::begin(ui);
        ui.set_min_width(220.0);
        if perms & CREATE_INVITE != 0
            && let Some(channel) = invite
            && menu::brand(ui, t!("Invite People"), menu::Tail::Icon(Icon::UserPlus)).clicked()
        {
            create_invite(app, channel);
            ui.close();
        }
        if perms & (MANAGE_GUILD | MANAGE_ROLES | MANAGE_CHANNELS) != 0 && menu::item(ui, t!("Server Settings"), menu::Tail::Icon(Icon::Settings)).clicked() {
            app.actions.push(Action::OpenGuildSettings(id));
            ui.close();
        }
        if perms & MANAGE_CHANNELS != 0 {
            for label in [t!("Create Channel"), t!("Create Category")] {
                if menu::item(ui, label, menu::Tail::Icon(Icon::CirclePlus)).clicked() {
                    app.actions.push(Action::OpenGuildSettings(id));
                    app.actions.push(Action::GuildTab(crate::admin::Tab::Channels));
                    ui.close();
                }
            }
        }
        if menu::item(ui, t!("Events"), menu::Tail::Icon(Icon::Calendar)).clicked() {
            gates::open(app, gates::Gate::Events { guild: id, draft: None });
            ui.close();
        }
        if menu::item(ui, t!("Channels & Roles"), menu::Tail::None).clicked() {
            gates::open(app, gates::Gate::Onboarding { guild: id, picked: None, page: 0 });
            ui.close();
        }
        menu::separator(ui);
        menu::notifications(app, ui, id, None);
        hide_muted_item(app, ui, id);
        if !owner {
            menu::separator(ui);
            if menu::danger(ui, t!("Leave Server"), menu::Tail::Icon(Icon::DoorExit)).clicked() {
                app.actions.push(Action::LeaveGuild(id));
                ui.close();
            }
        }
    });
}

/// Right click on a server, as in Discord.
fn guild_menu(app: &mut App, response: &egui::Response, id: Id) {
    response.context_menu(|ui| {
        let Some(guild) = app.guild(id) else { return };
        let me = app.me_id();
        let perms = guild.my_permissions(me);
        let owner = guild.owner_id == Some(me);
        let channels: Vec<Id> = guild.channels.iter().filter(|c| c.kind.is_text()).map(|c| c.id).collect();
        let invite = invite_channel(app, id);
        menu::begin(ui);
        if menu::item(ui, t!("Mark As Read"), menu::Tail::None).clicked() {
            app.actions.push(Action::MarkRead(channels));
            ui.close();
        }
        if perms & CREATE_INVITE != 0
            && let Some(channel) = invite
        {
            menu::separator(ui);
            if menu::brand(ui, t!("Invite People"), menu::Tail::None).clicked() {
                create_invite(app, channel);
                ui.close();
            }
        }
        menu::separator(ui);
        menu::mute(app, ui, menu::Muted::Server, Some(id), None);
        menu::notifications(app, ui, id, None);
        hide_muted_item(app, ui, id);
        if perms & (MANAGE_GUILD | MANAGE_ROLES | MANAGE_CHANNELS) != 0 {
            menu::separator(ui);
            if menu::item(ui, t!("Server Settings"), menu::Tail::None).clicked() {
                app.actions.push(Action::OpenGuildSettings(id));
                ui.close();
            }
        }
        if perms & MANAGE_CHANNELS != 0 {
            for label in [t!("Create Channel"), t!("Create Category")] {
                if menu::item(ui, label, menu::Tail::None).clicked() {
                    app.actions.push(Action::OpenGuildSettings(id));
                    app.actions.push(Action::GuildTab(crate::admin::Tab::Channels));
                    ui.close();
                }
            }
        }
        if !owner {
            menu::separator(ui);
            if menu::danger(ui, t!("Leave Server"), menu::Tail::None).clicked() {
                app.actions.push(Action::LeaveGuild(id));
                ui.close();
            }
        }
        menu::separator(ui);
        menu::copy_id(ui, t!("Copy Server ID"), id);
    });
}

fn rail(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    // Discord's: 6 points between icons, the first right under the title bar.
    ui.spacing_mut().item_spacing.y = 6.0;
    let dm_mentions = app.dm_mentions();
    let home = app.selected_guild.is_none();
    if rail_item(app, ui, home, false, dm_mentions, t!("Direct Messages"), |_, ui, rect, r| {
        let fill = if home { p.accent } else { p.selected };
        ui.painter().rect_filled(rect, r, fill);
        let color = if home { Color32::WHITE } else { p.strong };
        icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)), Icon::Discord, color);
    })
    .clicked()
    {
        app.actions.push(Action::SelectGuild(None));
    }
    update_button(app, ui);
    let (sep, _) = ui.allocate_exact_size(vec2(RAIL_ICON, 1.0), Sense::hover());
    ui.painter().hline(sep.shrink2(vec2(4.0, 0.0)).x_range(), sep.center().y, Stroke::new(1.0, p.selected));

    let entries = rail_entries(app);
    ScrollArea::vertical()
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            for entry in entries {
                match entry {
                    RailEntry::Guild(id) => guild_button(app, ui, id),
                    RailEntry::Folder { id, name, color, guilds } => {
                        folder(app, ui, id, name, color, &guilds);
                    }
                }
            }
            if rail_item(app, ui, false, false, 0, t!("Add a Server"), |_, ui, rect, r| {
                ui.painter().rect_filled(rect, r, p.selected);
                icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), Icon::Plus, p.green);
            })
            .clicked()
            {
                app.modal = Some(crate::app::Modal::AddServer { invite: String::new(), name: String::new() });
            }
            if rail_item(app, ui, false, false, 0, t!("Discover"), |_, ui, rect, r| {
                ui.painter().rect_filled(rect, r, p.selected);
                icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), Icon::Compass, p.green);
            })
            .clicked()
            {
                gates::open(app, gates::Gate::Discover { query: String::new(), asked: String::new() });
            }
        });
}

fn folder(app: &mut App, ui: &mut egui::Ui, id: i64, name: Option<String>, color: Option<u32>, guilds: &[Id]) {
    let p = theme::p();
    let open = app.open_folders.contains(&id);
    let tint = color
        .and_then(theme::role_color)
        .unwrap_or(p.accent);
    let (unread, mentions) = guilds
        .iter()
        .filter_map(|g| app.guild(*g))
        .map(|g| app.guild_unread(g))
        .fold((false, 0), |(u, m), (u2, m2)| (u || u2, m + m2));
    let label = name.clone().unwrap_or_else(|| t!("Folder").into());
    let top = ui.cursor().top();
    let icons: Vec<(Option<String>, String)> = guilds
        .iter()
        .take(4)
        .filter_map(|g| app.guild(*g))
        .map(|g| (g.icon_url(), initials(&g.name)))
        .collect();
    let selected_inside = guilds.iter().any(|g| app.selected_guild == Some(*g));
    let response = rail_item(app, ui, selected_inside && !open, unread, if open { 0 } else { mentions }, &label, |app, ui, rect, r| {
        ui.painter().rect_filled(rect, r, tint.linear_multiply(if open { 0.15 } else { 0.35 }));
        if open {
            icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)), Icon::Folder, tint);
        } else {
            for (i, (url, initials)) in icons.iter().enumerate() {
                let cell = Rect::from_min_size(
                    rect.min + vec2(5.0 + (i % 2) as f32 * 16.0, 5.0 + (i / 2) as f32 * 16.0),
                    Vec2::splat(14.0),
                );
                app.images.circle(ui, cell, url.as_deref(), initials, 7.0);
            }
        }
    });
    if !open {
        voice_badge(app, ui, response.rect, guilds);
    }
    let folder_channels: Vec<Id> = guilds
        .iter()
        .filter_map(|g| app.guild(*g))
        .flat_map(|g| g.channels.iter().filter(|c| c.kind.is_text()).map(|c| c.id))
        .collect();
    response.context_menu(|ui| {
        menu::begin(ui);
        if menu::item(ui, t!("Mark Folder As Read"), menu::Tail::None).clicked() {
            app.actions.push(Action::MarkRead(folder_channels.clone()));
            ui.close();
        }
    });
    if response.clicked() {
        if open {
            app.open_folders.remove(&id);
        } else {
            if app.plugin(Plugin::BetterFolders) {
                app.open_folders.clear();
            }
            app.open_folders.insert(id);
        }
    }
    if open {
        for guild in guilds {
            guild_button(app, ui, *guild);
        }
        let bottom = ui.cursor().top();
        let back = Rect::from_min_max(pos2(ui.max_rect().left(), top), pos2(ui.max_rect().left() + RAIL_ICON, bottom - 4.0));
        ui.painter().add(egui::Shape::rect_filled(back, 13.0, tint.linear_multiply(0.12)));
    }
}

// ---- channel list -----------------------------------------------------------------

/// A guild's channels in Discord's order: loose channels, then each
/// category with its text channels before its voice channels, threads
/// under their parents.
fn ordered_channels(app: &App, guild: Id) -> Vec<(Option<(Id, String)>, Vec<(Channel, bool)>)> {
    let Some(guild) = app.guild(guild) else { return Vec::new() };
    let me = app.me_id();
    let hidden = app.plugin(Plugin::ShowHiddenChannels);
    let rank = |c: &Channel| (matches!(c.kind, ChannelKind::Voice | ChannelKind::Stage), c.position, c.id);
    let shown = |c: &Channel| -> Option<bool> {
        if c.kind == ChannelKind::Category || c.kind == ChannelKind::Thread {
            return None;
        }
        let visible = guild.can_view(me, c);
        (visible || hidden).then_some(visible)
    };
    let with_threads = |list: Vec<(Channel, bool)>| -> Vec<(Channel, bool)> {
        let mut out = Vec::new();
        for (channel, visible) in list {
            let id = channel.id;
            out.push((channel, visible));
            if visible {
                let mut threads: Vec<&Channel> = guild
                    .channels
                    .iter()
                    .filter(|t| t.kind == ChannelKind::Thread && t.parent_id == Some(id) && t.joined && !t.archived)
                    .collect();
                threads.sort_by_key(|t| std::cmp::Reverse(t.last_message_id.unwrap_or(t.id)));
                out.extend(threads.into_iter().map(|t| (t.clone(), true)));
            }
        }
        out
    };
    let mut loose: Vec<(Channel, bool)> = guild
        .channels
        .iter()
        .filter(|c| c.parent_id.is_none())
        .filter_map(|c| shown(c).map(|v| (c.clone(), v)))
        .collect();
    loose.sort_by_key(|(c, _)| rank(c));
    let mut groups = vec![(None, with_threads(loose))];
    let mut categories: Vec<&Channel> = guild.channels.iter().filter(|c| c.kind == ChannelKind::Category).collect();
    categories.sort_by_key(|c| (c.position, c.id));
    for category in categories {
        let mut children: Vec<(Channel, bool)> = guild
            .channels
            .iter()
            .filter(|c| c.parent_id == Some(category.id))
            .filter_map(|c| shown(c).map(|v| (c.clone(), v)))
            .collect();
        if children.is_empty() {
            continue;
        }
        children.sort_by_key(|(c, _)| rank(c));
        groups.push((Some((category.id, category.name.to_uppercase())), with_threads(children)));
    }
    groups
}

fn sidebar(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let title = match app.selected_guild {
        Some(id) => app.guild(id).map(|g| g.name.clone()).unwrap_or_default(),
        None => t!("Direct Messages").into(),
    };
    // Discord's DM sidebar opens with "Find or start a conversation", a
    // secondary button that opens the quick switcher, where a server shows
    // its name.
    let margin = if app.selected_guild.is_none() { Margin::symmetric(10, 0) } else { Margin::symmetric(16, 0) };
    let header = Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.set_height(48.0);
        ui.set_width(ui.available_width());
        if app.selected_guild.is_none() {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                ui.painter().rect_filled(rect, 8.0, if response.hovered() { p.selected } else { p.hover });
                // A small secondary button: 14-point medium text, centred.
                let text = t!("Find or start a conversation");
                let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(14.0), p.text);
                let inner = rect.shrink2(vec2(8.0, 0.0));
                if galley.size().x <= inner.width() {
                    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, if response.hovered() { p.strong } else { p.text });
                } else {
                    paint_truncated(ui, inner, text, FontId::proportional(14.0), if response.hovered() { p.strong } else { p.text });
                }
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.switcher = Some(String::new());
                }
            });
            return;
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            // A long name stops short of the arrow.
            let room = ui.available_width() - if app.selected_guild.is_some() { 26.0 } else { 0.0 };
            ui.scope(|ui| {
                ui.set_max_width(room);
                ui.add(Label::new(RichText::new(title).font(theme::strong(16.0)).color(p.strong)).truncate());
            });
            if app.selected_guild.is_some() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                    icons::paint(ui, rect, Icon::ChevronDown, p.strong);
                });
            }
        });
    });
    if let Some(guild) = app.selected_guild {
        let response = ui.interact(header.response.rect, ui.id().with("server-menu"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(header.response.rect, 0.0, p.hover.linear_multiply(0.5));
        }
        server_menu(app, &response, guild);
    }
    let line = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.border));

    // Room for the account card, which covers this and the rail's bottom.
    egui::Panel::bottom("under-card").exact_size(account_card_height(app) + 16.0).show_separator_line(false).frame(Frame::NONE).show(ui, |_| {});

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // Rows follow each other as in Discord: 32 points and a point
        // above and below, no gap.
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(8.0);
        match app.selected_guild {
            Some(guild) => {
                // With "Manage Channels", channels and categories drag to
                // new places, as in Discord; the rows seen are where they
                // can land.
                let (rect, response) = row(ui, 32.0, false);
                icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(18.0)), Icon::Calendar, p.muted);
                ui.painter().text(pos2(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, t!("Events"), FontId::proportional(15.0), if response.hovered() { p.strong } else { p.muted });
                if response.clicked() {
                    gates::open(app, gates::Gate::Events { guild, draft: None });
                }
                let draggable = app.guild(guild).is_some_and(|g| g.my_permissions(app.me_id()) & MANAGE_CHANNELS != 0);
                let mut targets: Vec<(Rect, DropTarget)> = Vec::new();
                for (category, channels) in ordered_channels(app, guild) {
                    let mut collapsed = false;
                    let parent = category.as_ref().map(|(id, _)| *id);
                    if let Some((id, name)) = category {
                        collapsed = app.collapsed.contains(&id);
                        ui.add_space(12.0);
                        let sense = if draggable { Sense::click_and_drag() } else { Sense::click() };
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), sense);
                        targets.push((rect, DropTarget::Category(id)));
                        if draggable && response.drag_started() {
                            ui.data_mut(|d| d.insert_temp(egui::Id::new("channel-drag"), Dragging { id, category: true, label: name.clone() }));
                        }
                        let color = if response.hovered() { p.strong } else { p.muted };
                        let arrow = Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(12.0));
                        // The chevron points right while collapsed.
                        if collapsed {
                            icons::paint_chevron_right(ui, arrow, color);
                        } else {
                            icons::paint(ui, arrow, Icon::ChevronDown, color);
                        }
                        ui.painter().text(pos2(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, &name, theme::strong(12.0), color);
                        if response.clicked() {
                            if collapsed {
                                app.collapsed.remove(&id);
                            } else {
                                app.collapsed.insert(id);
                            }
                        }
                        category_menu(app, &response, guild, id, collapsed, &channels);
                    }
                    let hide_muted = app.settings.hide_muted.contains(&guild.to_string());
                    for (channel, visible) in channels {
                        // A collapsed category still shows the open channel
                        // and those with mentions, as Discord does; so do
                        // hidden muted channels.
                        let hidden = collapsed || (hide_muted && app.is_muted(&channel));
                        if hidden
                            && app.selected_channel != Some(channel.id)
                            && app.mentions(channel.id) == 0
                            && app.voice.channel.map(|(_, c)| c) != Some(channel.id)
                        {
                            continue;
                        }
                        let rect = channel_row(app, ui, &channel, visible, draggable);
                        if channel.kind != ChannelKind::Thread {
                            targets.push((rect, DropTarget::Channel { id: channel.id, parent }));
                        }
                    }
                }
                drag_channels(app, ui, guild, &targets);
            }
            None => {
                // Discord's rows above the conversations, in its order:
                // Friends, Message Requests (while there are any), Nitro
                // Home, Shop and Quests, then a divider.
                let home = app.selected_channel.is_none().then_some(app.home);
                let pending = app.relationships.values().filter(|r| r.kind == 3).count() as u32;
                if nav_row(ui, Icon::Friends, t!("Friends"), home == Some(crate::app::Home::Friends), pending).clicked() {
                    app.actions.push(Action::OpenFriends);
                }
                let requests = app.dms.iter().filter(|c| c.message_request && !c.spam).count() as u32;
                if app.dms.iter().any(|c| c.message_request) && nav_row(ui, Icon::Envelope, t!("Message Requests"), home == Some(crate::app::Home::MessageRequests), requests).clicked() {
                    app.actions.push(Action::OpenMessageRequests);
                }
                // Discord's store pages; here they open on discord.com.
                for (icon, label, path) in [(Icon::Nitro, t!("Nitro Home"), "store"), (Icon::Shop, t!("Shop"), "shop"), (Icon::Quests, t!("Quests"), "quest-home")] {
                    if nav_row(ui, icon, label, false, 0).clicked() {
                        let _ = open::that_detached(format!("https://discord.com/{path}"));
                    }
                }
                ui.add_space(8.0);
                let y = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range().shrink(16.0), y, Stroke::new(1.0, p.border));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new(t!("Direct Messages").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(12.0);
                        let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                        icons::paint(ui, rect, Icon::Plus, if response.hovered() { p.strong } else { p.muted });
                        if response.on_hover_text(t!("Create DM")).clicked() {
                            app.modal = Some(crate::app::Modal::NewDm { picked: Vec::new(), filter: String::new() });
                        }
                    });
                });
                // Requests wait on their own page until accepted.
                let mut dms: Vec<Channel> = app.dms.iter().filter(|c| !c.message_request).cloned().collect();
                dms.sort_by_key(|c| std::cmp::Reverse(app.latest.get(&c.id).copied().or(c.last_message_id).unwrap_or(c.id)));
                for channel in dms {
                    dm_row(app, ui, &channel);
                }
            }
        }
        ui.add_space(8.0);
    });
}

/// A channel or category being dragged.
#[derive(Clone)]
struct Dragging {
    id: Id,
    category: bool,
    label: String,
}

/// A row a dragged channel can be dropped on.
enum DropTarget {
    Category(Id),
    Channel { id: Id, parent: Option<Id> },
}

/// While a channel or category is dragged: its name under the pointer, the
/// line where it would land, and on release the move (Esc gives up).
fn drag_channels(app: &mut App, ui: &mut egui::Ui, guild: Id, targets: &[(Rect, DropTarget)]) {
    let p = theme::p();
    let key = egui::Id::new("channel-drag");
    let Some(dragging) = ui.data(|d| d.get_temp::<Dragging>(key)) else { return };
    let (pointer, released, cancel) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_released() || !i.pointer.any_down(), i.key_pressed(Key::Escape)));
    let Some(pointer) = pointer else { return };
    if cancel {
        ui.data_mut(|d| d.remove::<Dragging>(key));
        return;
    }
    // Where it would land, and the line that shows it.
    let mut landing: Option<(f32, Action)> = None;
    if dragging.category {
        let next = targets.iter().find_map(|(rect, t)| match t {
            DropTarget::Category(id) if rect.center().y > pointer.y => Some((rect.top() - 6.0, *id)),
            _ => None,
        });
        let line = next.map_or_else(|| targets.last().map_or(pointer.y, |(r, _)| r.bottom() + 2.0), |(y, _)| y);
        let before = next.map(|(_, id)| id);
        if before != Some(dragging.id) {
            landing = Some((line, Action::MoveCategory { guild, category: dragging.id, before }));
        }
    } else {
        let hit = targets
            .iter()
            .min_by(|(a, _), (b, _)| (a.center().y - pointer.y).abs().total_cmp(&(b.center().y - pointer.y).abs()));
        if let Some((rect, target)) = hit {
            let (line, parent, anchor) = match target {
                DropTarget::Category(id) => (rect.bottom() + 2.0, Some(*id), crate::app::Anchor::Start),
                DropTarget::Channel { id, parent } if pointer.y < rect.center().y => (rect.top(), *parent, crate::app::Anchor::Before(*id)),
                DropTarget::Channel { id, parent } => (rect.bottom(), *parent, crate::app::Anchor::After(*id)),
            };
            let onto_itself = matches!(anchor, crate::app::Anchor::Before(id) | crate::app::Anchor::After(id) if id == dragging.id);
            if !onto_itself {
                landing = Some((line, Action::MoveChannel { guild, channel: dragging.id, parent, anchor }));
            }
        }
    }
    if let Some((y, _)) = &landing {
        let x = ui.max_rect().x_range().shrink(8.0);
        ui.painter().hline(x, *y, Stroke::new(2.0, p.accent));
        ui.painter().circle_filled(pos2(x.min, *y), 3.5, p.accent);
    }
    // The name follows the pointer, as the row Discord drags.
    egui::Area::new(egui::Id::new("channel-drag-ghost"))
        .fixed_pos(pointer + vec2(12.0, -14.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            Frame::new().fill(p.selected).corner_radius(4).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                let text = if dragging.category { dragging.label.clone() } else { format!("# {}", dragging.label) };
                ui.label(RichText::new(text).font(theme::strong(14.0)).color(p.strong));
            });
        });
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    if released {
        ui.data_mut(|d| d.remove::<Dragging>(key));
        if let Some((_, action)) = landing {
            app.actions.push(action);
        }
    }
}

/// One of the DM sidebar's page rows: Discord's 42-point link with a
/// 24-point icon, its name and a count.
fn nav_row(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool, count: u32) -> egui::Response {
    let p = theme::p();
    let (rect, response) = row(ui, 42.0, selected);
    let color = if selected || response.hovered() { p.strong } else { p.muted };
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), Vec2::splat(24.0)), icon, color);
    // A long name ("Solicitações de mensagens") stops short of the count.
    let right = if count > 0 { rect.right() - 34.0 } else { rect.right() - 8.0 };
    paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 52.0, rect.top()), pos2(right, rect.bottom())), label, theme::strong(15.0), color);
    if count > 0 {
        badge(ui, pos2(rect.right() - 16.0, rect.center().y), count, Color32::TRANSPARENT);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn row(ui: &mut egui::Ui, height: f32, selected: bool) -> (Rect, egui::Response) {
    row_sense(ui, height, selected, Sense::click())
}

fn row_sense(ui: &mut egui::Ui, height: f32, selected: bool, sense: Sense) -> (Rect, egui::Response) {
    let p = theme::p();
    let width = ui.available_width();
    let (outer, response) = ui.allocate_exact_size(vec2(width, height + 2.0), sense);
    let rect = Rect::from_min_max(pos2(outer.left() + 8.0, outer.top() + 1.0), pos2(outer.right() - 8.0, outer.bottom() - 1.0));
    let fill = if selected {
        Some(p.selected)
    } else if response.hovered() {
        Some(p.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    (rect, response)
}

/// A channel in the list; returns its row, where a dragged one can land.
fn channel_row(app: &mut App, ui: &mut egui::Ui, channel: &Channel, visible: bool, draggable: bool) -> Rect {
    let p = theme::p();
    let draggable = draggable && channel.kind != ChannelKind::Thread;
    let thread = channel.kind == ChannelKind::Thread;
    let selected = app.selected_channel == Some(channel.id);
    let voice = matches!(channel.kind, ChannelKind::Voice | ChannelKind::Stage);
    let muted = app.is_muted(channel);
    let unread = app.is_unread(channel.id) && !muted;
    let mentions = app.mentions(channel.id);
    let in_this_voice = app.voice.channel.map(|(_, c)| c) == Some(channel.id);
    // A voice channel's status shows under its name; in the one you are in,
    // with the permission, "Set a channel status" stands in for it.
    let me = app.me_id();
    let can_set_status = voice
        && in_this_voice
        && channel.guild_id.and_then(|g| app.guild(g)).is_some_and(|g| g.channel_permissions(me, channel) & (SET_VOICE_CHANNEL_STATUS | MANAGE_CHANNELS) != 0);
    let status_line = channel.status.clone().or_else(|| can_set_status.then(|| t!("Set a channel status").to_owned()));
    let sense = if draggable { Sense::click_and_drag() } else { Sense::click() };
    let height = if thread { 28.0 } else if status_line.is_some() { 44.0 } else { 32.0 };
    let (rect, response) = row_sense(ui, height, selected || in_this_voice, sense);
    // The name keeps the top 32 points of a two-line row.
    let rect = if status_line.is_some() { Rect::from_min_size(rect.min, vec2(rect.width(), 30.0)) } else { rect };
    if draggable && response.drag_started() {
        ui.data_mut(|d| d.insert_temp(egui::Id::new("channel-drag"), Dragging { id: channel.id, category: false, label: channel.name.clone() }));
    }
    let icon = match channel.kind {
        _ if !visible => Icon::Lock,
        ChannelKind::Voice => Icon::Voice,
        ChannelKind::Stage => Icon::Stage,
        ChannelKind::Announcement => Icon::Announcements,
        ChannelKind::Forum => Icon::Forum,
        ChannelKind::Thread => Icon::Thread,
        _ => Icon::Text,
    };
    let color = if !visible || muted {
        p.faint
    } else if selected || response.hovered() || (unread && !voice) {
        p.strong
    } else {
        p.muted
    };
    if unread && !selected && !voice {
        let pill = Rect::from_center_size(pos2(rect.left() - 8.0, rect.center().y), vec2(8.0, 8.0));
        ui.painter().rect_filled(pill, 4.0, p.strong);
    }
    let indent = if thread { 22.0 } else { 0.0 };
    let icon_color = if selected || response.hovered() { p.muted } else { p.faint };
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 18.0 + indent, rect.center().y), Vec2::splat(18.0)), icon, icon_color);
    let font = if unread { theme::strong(15.0) } else { FontId::proportional(15.0) };
    let mut name_right = rect.right() - 30.0;
    if voice {
        let mut x = rect.right() - 6.0;
        // A Stage's limit (10000) is the API's default, not one anyone set.
        if channel.user_limit > 0 && channel.kind != ChannelKind::Stage {
            let count = app.voice_members(channel).len();
            x = count_pill(ui, pos2(x, rect.center().y), count as u64, channel.user_limit);
            x -= 6.0;
        }
        if in_this_voice
            && let Some(since) = app.voice.since
        {
            let secs = since.elapsed().as_secs();
            let text = if secs >= 3600 {
                format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
            } else {
                format!("{}:{:02}", secs / 60, secs % 60)
            };
            let galley = ui.painter().layout_no_wrap(text, FontId::monospace(12.0), p.green);
            x -= galley.size().x;
            ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.green);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            x -= 6.0;
        }
        name_right = x;
    }
    let name_rect = Rect::from_min_max(pos2(rect.left() + 32.0 + indent, rect.top()), pos2(name_right, rect.bottom()));
    paint_truncated(ui, name_rect, &channel.name, font, color);
    if let Some(line) = &status_line {
        let line_rect = Rect::from_min_max(pos2(rect.left() + 32.0, rect.bottom() - 4.0), pos2(rect.right() - 6.0, rect.bottom() + 12.0));
        let line_response = can_set_status.then(|| ui.interact(line_rect, ui.id().with(("voice-status", channel.id)), Sense::click()));
        let hovered = line_response.as_ref().is_some_and(egui::Response::hovered);
        let galley = ui.painter().layout(line.clone(), FontId::proportional(12.0), if hovered { p.text } else { p.muted }, line_rect.width() - 18.0);
        let galley = if galley.rows.len() > 1 {
            ui.painter().layout_job(egui::text::LayoutJob { wrap: egui::text::TextWrapping::truncate_at_width(line_rect.width() - 18.0), ..egui::text::LayoutJob::simple_singleline(line.clone(), FontId::proportional(12.0), p.muted) })
        } else {
            galley
        };
        let text_width = galley.size().x;
        ui.painter().galley(pos2(line_rect.left(), line_rect.center().y - galley.size().y / 2.0), galley, p.muted);
        if can_set_status {
            let pencil = Rect::from_center_size(pos2(line_rect.left() + text_width + 10.0, line_rect.center().y), Vec2::splat(12.0));
            icons::paint(ui, pencil, Icon::Pencil, if hovered { p.text } else { p.muted });
        }
        if let Some(line_response) = line_response
            && line_response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        {
            app.modal = Some(crate::app::Modal::VoiceStatus { channel: channel.id, status: channel.status.clone().unwrap_or_default() });
        }
    }
    if mentions > 0 {
        badge(ui, pos2(rect.right() - 16.0, rect.center().y), mentions, Color32::TRANSPARENT);
    } else if app.plugin(Plugin::TypingIndicator)
        && app.typing.get(&channel.id).is_some_and(|t| !t.is_empty())
    {
        ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, "•••", FontId::proportional(13.0), p.muted);
    }
    channel_menu(app, &response, channel);
    if response.clicked() {
        if !visible {
            app.notify(t!("You do not have permission to view this channel."));
        } else if voice && in_this_voice {
            // Already there: a click opens it, as Discord's list.
            app.actions.push(Action::OpenVoiceChannel { guild: channel.guild_id, channel: channel.id });
        } else if voice {
            app.actions.push(Action::JoinVoice { guild: channel.guild_id, channel: channel.id, ring: false });
        } else if channel.kind == ChannelKind::Forum {
            app.actions.push(Action::OpenForum(channel.id));
        } else {
            app.actions.push(Action::SelectChannel(channel.id));
        }
    }
    // The row being dragged stays dimmed in its old place.
    if ui.data(|d| d.get_temp::<Dragging>(egui::Id::new("channel-drag"))).is_some_and(|d| d.id == channel.id) {
        ui.painter().rect_filled(rect, 4.0, p.sidebar.gamma_multiply(0.7));
    }
    if voice {
        let members = app.voice_members(channel);
        for user in members {
            voice_member(app, ui, channel.guild_id, user);
        }
        // Discord's last row in the channel you are in.
        let can_invite = channel.guild_id.and_then(|g| app.guild(g)).is_some_and(|g| g.channel_permissions(me, channel) & CREATE_INVITE != 0);
        if in_this_voice && can_invite {
            let (row_rect, row_response) = row(ui, 26.0, false);
            let hovered = row_response.hovered();
            let tint = if hovered { p.text } else { p.muted };
            icons::paint(ui, Rect::from_center_size(pos2(row_rect.left() + 44.0, row_rect.center().y), Vec2::splat(16.0)), Icon::UserPlus, tint);
            ui.painter().text(pos2(row_rect.left() + 62.0, row_rect.center().y), Align2::LEFT_CENTER, t!("Invite to Voice"), FontId::proportional(14.0), tint);
            icons::paint_chevron_right(ui, Rect::from_center_size(pos2(row_rect.right() - 14.0, row_rect.center().y), Vec2::splat(14.0)), tint);
            if row_response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                create_invite(app, channel.id);
            }
        }
    }
    rect
}

/// Right click on a category, as in Discord.
fn category_menu(app: &mut App, response: &egui::Response, guild: Id, id: Id, collapsed: bool, channels: &[(Channel, bool)]) {
    response.context_menu(|ui| {
        menu::begin(ui);
        let text: Vec<Id> = channels.iter().filter(|(c, v)| *v && c.kind.is_text()).map(|(c, _)| c.id).collect();
        if menu::item(ui, t!("Mark As Read"), menu::Tail::None).clicked() {
            app.actions.push(Action::MarkRead(text));
            ui.close();
        }
        menu::separator(ui);
        if menu::item(ui, t!("Collapse Category"), menu::Tail::Check(collapsed)).clicked() {
            if collapsed {
                app.collapsed.remove(&id);
            } else {
                app.collapsed.insert(id);
            }
        }
        if menu::item(ui, t!("Collapse All Categories"), menu::Tail::None).clicked() {
            let all: Vec<Id> = app.guild(guild).map(|g| g.channels.iter().filter(|c| c.kind == ChannelKind::Category).map(|c| c.id).collect()).unwrap_or_default();
            app.collapsed.extend(all);
            ui.close();
        }
        menu::separator(ui);
        menu::mute(app, ui, menu::Muted::Category, Some(guild), Some(id));
        if let Some(category) = app.channel(id).cloned() {
            menu::notifications(app, ui, guild, Some(&category));
        }
        let perms = app.guild(guild).map_or(0, |g| g.my_permissions(app.me_id()));
        if perms & MANAGE_CHANNELS != 0 {
            menu::separator(ui);
            if menu::item(ui, t!("Edit Category"), menu::Tail::None).clicked() {
                app.actions.push(Action::OpenGuildSettings(guild));
                app.actions.push(Action::GuildTab(crate::admin::Tab::Channels));
                ui.close();
            }
        }
        menu::separator(ui);
        menu::copy_id(ui, t!("Copy Category ID"), id);
    });
}

/// Right click on a channel, as in Discord.
fn channel_menu(app: &mut App, response: &egui::Response, channel: &Channel) {
    response.context_menu(|ui| {
        menu::begin(ui);
        let voice = matches!(channel.kind, ChannelKind::Voice | ChannelKind::Stage);
        let (guild, id) = (channel.guild_id, channel.id);
        let perms = guild.and_then(|g| app.guild(g)).map_or(0, |g| g.my_permissions(app.me_id()));
        if voice {
            if menu::item(ui, t!("Join Voice Channel"), menu::Tail::None).clicked() {
                app.actions.push(Action::JoinVoice { guild, channel: id, ring: false });
                ui.close();
            }
            if channel.kind == ChannelKind::Stage && perms & MANAGE_CHANNELS != 0 {
                if menu::item(ui, t!("Start Stage"), menu::Tail::None).clicked() {
                    gates::open(app, gates::Gate::Stage { channel: id, topic: String::new() });
                    ui.close();
                }
                if menu::item(ui, t!("End Stage"), menu::Tail::None).clicked() {
                    app.commands.send(crate::backend::Command::Api { key: "pin".into(), method: "DELETE", path: format!("/stage-instances/{id}"), body: None });
                    ui.close();
                }
            }
        } else if menu::item(ui, t!("Mark As Read"), menu::Tail::None).clicked() {
            app.actions.push(Action::MarkRead(vec![id]));
            ui.close();
        }
        menu::separator(ui);
        if perms & CREATE_INVITE != 0 && menu::brand(ui, t!("Invite People"), menu::Tail::None).clicked() {
            create_invite(app, id);
            ui.close();
        }
        if menu::item(ui, t!("Copy Link"), menu::Tail::None).clicked() {
            let guild = guild.map_or_else(|| "@me".into(), |g| g.to_string());
            ui.ctx().copy_text(format!("https://discord.com/channels/{guild}/{id}"));
            ui.close();
        }
        if perms & (1 << 28) != 0 && !matches!(channel.kind, ChannelKind::Thread) && menu::item(ui, t!("Permissions"), menu::Tail::None).clicked() {
            gates::open(app, gates::Gate::Overwrites { channel: id, picked: None });
            ui.close();
        }
        if channel.kind == ChannelKind::Thread {
            let path = format!("/channels/{id}");
            let api = |method: &'static str, path: String, body: Option<serde_json::Value>| crate::backend::Command::Api { key: "pin".into(), method, path, body };
            let me = app.me_id();
            let mine = channel.owner == Some(me);
            // Manage Threads.
            let manage = perms & (1 << 34) != 0;
            menu::separator(ui);
            let (label, method) = if channel.joined { (t!("Leave Thread"), "DELETE") } else { (t!("Join Thread"), "PUT") };
            if menu::item(ui, label, menu::Tail::None).clicked() {
                app.commands.send(api(method, format!("{path}/thread-members/@me?location=Context%20Menu"), None));
                ui.close();
            }
            if (mine || manage) && !channel.archived && menu::item(ui, t!("Close Thread"), menu::Tail::None).clicked() {
                app.commands.send(api("PATCH", path.clone(), Some(serde_json::json!({ "archived": true }))));
                ui.close();
            }
            if manage && menu::item(ui, if channel.locked { t!("Unlock Thread") } else { t!("Lock Thread") }, menu::Tail::None).clicked() {
                app.commands.send(api("PATCH", path.clone(), Some(serde_json::json!({ "locked": !channel.locked }))));
                ui.close();
            }
        }
        menu::separator(ui);
        menu::mute(app, ui, menu::Muted::Channel, guild, Some(id));
        if let Some(guild) = guild {
            menu::notifications(app, ui, guild, Some(channel));
        }
        if perms & MANAGE_CHANNELS != 0
            && let Some(guild) = guild
        {
            menu::separator(ui);
            if menu::item(ui, t!("Edit Channel"), menu::Tail::None).clicked() {
                app.actions.push(Action::OpenGuildSettings(guild));
                app.actions.push(Action::GuildTab(crate::admin::Tab::Channels));
                ui.close();
            }
        }
        menu::separator(ui);
        menu::copy_id(ui, t!("Copy Channel ID"), id);
    });
}

fn voice_member(app: &mut App, ui: &mut egui::Ui, guild: Option<Id>, user_id: Id) {
    let p = theme::p();
    let user = app.users.get(&user_id).cloned().unwrap_or(crate::discord::model::User {
        id: user_id,
        ..Default::default()
    });
    let (mut name, _) = app.display(guild, &user);
    if name.trim().is_empty() {
        // Not known yet: the member request in `display` brings the name.
        if let Some(guild) = guild {
            app.request_member(guild, user_id);
        }
        name = t!("Loading…").into();
    }
    // The row stays lit while its activity card is open beside it.
    let open = activity::voice_card_open(ui, user_id);
    let (rect, response) = row(ui, 26.0, open);
    let avatar = Rect::from_min_size(pos2(rect.left() + 34.0, rect.center().y - 10.0), Vec2::splat(20.0));
    app.images.circle(ui, avatar, Some(&user.avatar_url()), &initials(&name), 10.0);
    let speaking = app.voice.speaking.contains(&user_id);
    if speaking {
        ui.painter().circle_stroke(avatar.center(), 11.5, Stroke::new(2.0, p.green));
    }
    let (muted, deaf) = app.voice_flags.get(&user_id).copied().unwrap_or_default();
    let mut right = rect.right() - 6.0;
    // An emoji effect or a sound they just sent, beside them for a moment.
    if let Some((id, emoji, _)) = app.voice_effects.get(&user_id).cloned() {
        let r = Rect::from_center_size(pos2(right - 9.0, rect.center().y), Vec2::splat(18.0));
        match (id, emoji) {
            (Some(id), _) => {
                if let Some((texture, _)) = app.images.get(&crate::discord::model::emoji_url(id, true, 48), 48) {
                    egui::Image::from_texture((texture, r.size())).paint_at(ui, r);
                }
            }
            (None, Some(emoji)) => markdown::paint_twemoji(app, ui, &emoji, r),
            _ => {}
        }
        right -= 22.0;
    }
    for (on, icon) in [(deaf, Icon::HeadphonesSlash), (muted, Icon::MicrophoneSlash)] {
        if on {
            let r = Rect::from_center_size(pos2(right - 8.0, rect.center().y), Vec2::splat(15.0));
            icons::paint(ui, r, icon, p.muted);
            right -= 20.0;
        }
    }
    // The game they are playing, its icon at the row's end, as Discord's.
    let game = app.presences.get(&user_id).and_then(|presence| presence.activities.iter().find(|a| a.kind != 2)).and_then(|a| a.application);
    if let Some(application) = game {
        let r = Rect::from_center_size(pos2(right - 8.0, rect.center().y), Vec2::splat(16.0));
        match app.app_icon(application).and_then(|url| app.images.get(&url, 32)) {
            Some((texture, _)) => {
                egui::Image::from_texture((texture, r.size())).corner_radius(4.0).paint_at(ui, r);
            }
            None => icons::paint(ui, r, Icon::Controller, p.muted),
        }
        right -= 22.0;
    }
    let name_rect = Rect::from_min_max(pos2(avatar.right() + 8.0, rect.top()), pos2(right - 4.0, rect.bottom()));
    // Names in a call take no role colour in Discord: grey, white while
    // speaking or pointed at.
    name_with_tag(app, ui, name_rect, &user, &name, FontId::proportional(14.0), if speaking || open || response.hovered() { p.strong } else { p.muted });
    let me = app.me_id();
    let perms = guild.and_then(|g| app.guild(g)).map_or(0, |g| g.my_permissions(me));
    let (muted, deaf) = app.voice_flags.get(&user_id).copied().unwrap_or_default();
    response.context_menu(|ui| {
        menu::begin(ui);
        if user_id != me {
            // As Discord: muting someone only for you, and their volume
            // for you (0% is the same mute).
            let key = user_id.to_string();
            let volume = app.settings.user_volumes.get(&key).copied().unwrap_or(1.0);
            let locally_muted = volume == 0.0;
            if menu::item(ui, t!("Mute"), menu::Tail::Check(locally_muted)).clicked() {
                app.settings.user_volumes.insert(key.clone(), if locally_muted { 1.0 } else { 0.0 });
                app.actions.push(Action::SettingsChanged);
            }
            let quiet = app.settings.soundboard_muted.contains(&key);
            if menu::item(ui, t!("Mute Soundboard"), menu::Tail::Check(quiet)).clicked() {
                if quiet {
                    app.settings.soundboard_muted.retain(|user| *user != key);
                } else {
                    app.settings.soundboard_muted.push(key.clone());
                }
                app.actions.push(Action::SettingsChanged);
            }
            ui.add_space(4.0);
            ui.label(RichText::new(t!("User Volume")).size(12.0).color(p.muted));
            let max = if app.plugin(crate::plugins::Plugin::VolumeBooster) { 4.0 } else { 2.0 };
            let mut level = volume;
            ui.spacing_mut().slider_width = 180.0;
            ui.visuals_mut().handle_shape = egui::style::HandleShape::Circle;
            ui.visuals_mut().selection.bg_fill = p.accent;
            if ui.add(egui::Slider::new(&mut level, 0.0..=max).trailing_fill(true).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() {
                app.settings.user_volumes.insert(key, level);
                app.actions.push(Action::SettingsChanged);
            }
            menu::separator(ui);
        }
        if let Some(guild) = guild {
            let member = |body: serde_json::Value| crate::backend::Command::Api {
                key: "pin".into(),
                method: "PATCH",
                path: format!("/guilds/{guild}/members/{user_id}"),
                body: Some(body),
            };
            if perms & (1 << 22) != 0 && menu::item(ui, t!("Server Mute"), menu::Tail::Check(muted)).clicked() {
                app.commands.send(member(serde_json::json!({ "mute": !muted })));
            }
            if perms & (1 << 23) != 0 && menu::item(ui, t!("Server Deafen"), menu::Tail::Check(deaf)).clicked() {
                app.commands.send(member(serde_json::json!({ "deaf": !deaf })));
            }
            if perms & (1 << 24) != 0 && menu::danger(ui, t!("Disconnect"), menu::Tail::None).clicked() {
                app.commands.send(member(serde_json::json!({ "channel_id": null })));
                ui.close();
            }
            // On a Stage, with Mute Members: bring someone up, or back down.
            if perms & (1 << 22) != 0
                && let Some((_, channel)) = app.voice.channel.filter(|(_, c)| app.channel(*c).is_some_and(|c| c.kind == ChannelKind::Stage))
            {
                let suppressed = app.voice_stage.get(&user_id).is_none_or(|(suppress, _)| *suppress);
                let (label, body) = if suppressed {
                    (t!("Invite to Speak"), serde_json::json!({ "channel_id": channel.to_string(), "suppress": false, "request_to_speak_timestamp": jiff::Timestamp::now().to_string() }))
                } else {
                    (t!("Move to Audience"), serde_json::json!({ "channel_id": channel.to_string(), "suppress": true }))
                };
                if menu::item(ui, label, menu::Tail::None).clicked() {
                    app.commands.send(crate::backend::Command::Api { key: "pin".into(), method: "PATCH", path: format!("/guilds/{guild}/voice-states/{user_id}"), body: Some(body) });
                    ui.close();
                }
            }
            if perms & (1 << 24) != 0 {
                let here = app.user_voice_channel(user_id).map(|channel| channel.id);
                let targets: Vec<(Id, String)> = app.guild(guild).map_or_else(Vec::new, |g| g.channels.iter().filter(|c| c.kind == crate::discord::model::ChannelKind::Voice && Some(c.id) != here && g.can_view(me, c)).map(|c| (c.id, c.title())).collect());
                if !targets.is_empty() {
                    menu::submenu(ui, t!("Move to"), |ui| {
                        for (channel, name) in &targets {
                            if menu::item(ui, name, menu::Tail::None).clicked() {
                                app.commands.send(member(serde_json::json!({ "channel_id": channel.to_string() })));
                                ui.close();
                            }
                        }
                    });
                }
            }
            if perms & ((1 << 22) | (1 << 23) | (1 << 24)) != 0 {
                menu::separator(ui);
            }
        }
        menu::user(app, ui, &user, guild);
    });
    activity::voice_card(app, ui, &response, user_id);
    if response.clicked() {
        let at = ui.ctx().pointer_latest_pos().unwrap_or_default();
        app.actions.push(Action::OpenProfile { user, guild, at });
    }
}

fn dm_row(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let p = theme::p();
    let selected = app.selected_channel == Some(channel.id);
    let unread = app.is_unread(channel.id);
    let mentions = app.mentions(channel.id);
    let (rect, response) = row(ui, 42.0, selected);
    let title = channel.title();
    let avatar = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 16.0), Vec2::splat(32.0));
    let url = (channel.kind == ChannelKind::Dm)
        .then(|| channel.recipients.first().map(|u| u.avatar_url()))
        .flatten();
    app.images.circle(ui, avatar, url.as_deref(), &initials(&title), 16.0);
    if channel.kind == ChannelKind::Dm
        && let Some(user) = channel.recipients.first()
        && let Some(presence) = app.presences.get(&user.id)
        && presence.status != "offline"
    {
        friends::dot(ui, avatar, &presence.status, if selected { p.selected } else { p.sidebar });
    }
    if app.calls.contains_key(&channel.id) {
        ui.painter().circle(avatar.right_bottom() - vec2(3.0, 3.0), 6.0, p.green, Stroke::new(2.0, p.sidebar));
    }
    let color = if selected || response.hovered() || unread { p.strong } else { p.muted };
    let font = if unread { theme::strong(15.0) } else { FontId::proportional(15.0) };
    let x = avatar.right() + 12.0;
    let right = rect.right() - 30.0;
    // Under the name, as Discord's list: its activity status for a person
    // (the game, "In voice", "+1", the custom status), a group's size.
    let top = Rect::from_min_max(pos2(x, rect.center().y - 17.0), pos2(right, rect.center().y + 1.0));
    let bottom = Rect::from_min_max(pos2(x, rect.center().y + 1.0), pos2(right, rect.center().y + 17.0));
    let person = (channel.kind == ChannelKind::Dm).then(|| channel.recipients.first().cloned()).flatten();
    let has_line = match &person {
        Some(user) => friends::has_status_line(app, user.id),
        None => true,
    };
    let name_rect = if has_line { top } else { Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom())) };
    match &person {
        Some(user) => name_with_tag(app, ui, name_rect, user, &title, font, color),
        None => paint_truncated(ui, name_rect, &title, font, color),
    }
    match &person {
        Some(user) => {
            friends::status_line(app, ui, bottom, user.id, true);
        }
        None => paint_truncated(ui, bottom, &tn!(channel.recipients.len() + 1, "{count} Member", "{count} Members"), FontId::proportional(12.0), p.muted),
    }
    if mentions > 0 {
        badge(ui, pos2(rect.right() - 16.0, rect.center().y), mentions, Color32::TRANSPARENT);
    }
    if response.clicked() {
        app.actions.push(Action::SelectChannel(channel.id));
    }
    let id = channel.id;
    let user = (channel.kind == ChannelKind::Dm).then(|| channel.recipients.first().cloned()).flatten();
    let group = channel.kind == ChannelKind::GroupDm;
    let group_name = channel.name.clone();
    response.context_menu(|ui| {
        menu::begin(ui);
        if menu::item(ui, t!("Mark As Read"), menu::Tail::None).clicked() {
            app.actions.push(Action::MarkRead(vec![id]));
            ui.close();
        }
        menu::separator(ui);
        if let Some(user) = &user {
            menu::user(app, ui, user, None);
            menu::separator(ui);
        }
        menu::mute(app, ui, menu::Muted::Conversation, None, Some(id));
        if group {
            if menu::item(ui, t!("Edit Group"), menu::Tail::None).clicked() {
                app.modal = Some(crate::app::Modal::GroupName { channel: id, name: group_name.clone() });
                ui.close();
            }
            if menu::item(ui, t!("Add Friends to DM"), menu::Tail::None).clicked() {
                app.modal = Some(crate::app::Modal::GroupAdd { channel: id, picked: Vec::new(), filter: String::new() });
                ui.close();
            }
            if menu::danger(ui, t!("Leave Group"), menu::Tail::None).clicked() {
                app.modal = Some(crate::app::Modal::Confirm {
                    title: t!("Leave Group").into(),
                    body: t!("Are you sure you want to leave this group?").into(),
                    button: t!("Leave Group").into(),
                    then: crate::app::Confirmed::LeaveGroup { channel: id },
                });
                ui.close();
            }
        } else if menu::item(ui, t!("Close DM"), menu::Tail::None).clicked() {
            app.actions.push(Action::CloseDm(id));
            ui.close();
        }
        menu::separator(ui);
        menu::copy_id(ui, t!("Copy Channel ID"), id);
    });
}

/// Appends a translated template to a layout job: its text in `plain`, each
/// `{field}` named in `fields` in `bold`, so every language puts the names
/// where its grammar wants them ("{user} banned {target}"). A brace with no
/// field stays as written.
pub fn rich_template(job: &mut egui::text::LayoutJob, template: &str, plain: &egui::TextFormat, bold: &egui::TextFormat, fields: &[(&str, &str)]) {
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(length) = rest[start..].find('}') else { break };
        let field = fields.iter().find(|(name, _)| *name == &rest[start + 1..start + length]);
        job.append(&rest[..start], 0.0, plain.clone());
        match field {
            Some((_, value)) => job.append(value, 0.0, bold.clone()),
            None => job.append(&rest[start..=start + length], 0.0, plain.clone()),
        }
        rest = &rest[start + length + 1..];
    }
    job.append(rest, 0.0, plain.clone());
}

/// A name in `rect` with the server tag it wears after it, the name cut
/// short so the tag keeps its room, as Discord's lists show them.
pub(super) fn name_with_tag(app: &mut App, ui: &mut egui::Ui, rect: Rect, user: &crate::discord::model::User, name: &str, font: FontId, color: Color32) {
    let tag = user.tag.as_ref().map(|(_, tag, _)| tag.as_str());
    let chip = tag.map(|tag| profile::tag_size(ui, user, tag, 16.0));
    let room = (rect.width() - chip.map_or(0.0, |c| c.x + 6.0)).max(0.0);
    paint_truncated(ui, Rect::from_min_size(rect.min, vec2(room, rect.height())), name, font.clone(), color);
    if let (Some(tag), Some(chip)) = (tag, chip) {
        let width = ui.painter().layout_no_wrap(name.to_owned(), font, color).size().x.min(room);
        let at = Rect::from_min_size(pos2(rect.left() + width + 6.0, rect.center().y - chip.y / 2.0), chip);
        profile::paint_tag(app, ui, at, user, tag);
    }
}

pub fn paint_truncated(ui: &egui::Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    let mut job = crate::text::script_job(text, font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let pos = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}

/// A 32-point button with one of Discord's icons; `active` paints it red
/// (a muted microphone, deafened headphones).
fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, active: bool) -> bool {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let color = if active {
        p.red
    } else if response.hovered() {
        p.strong
    } else {
        p.muted
    };
    icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), icon, color);
    response.on_hover_text(tooltip).clicked()
}

/// Discord's split button in the account panel: the icon toggles, the
/// arrow beside it opens the device menu.
fn split_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, menu: &str, active: bool) -> (egui::Response, egui::Response) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(50.0, 32.0), Sense::hover());
    let main = Rect::from_min_size(rect.min, Vec2::splat(32.0));
    let side = Rect::from_min_max(pos2(main.right(), rect.top()), rect.max);
    let button = ui.interact(main, ui.id().with(("split", menu, "button")), Sense::click());
    let arrow = ui.interact(side, ui.id().with(("split", menu, "arrow")), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&arrow));
    if button.hovered() || arrow.hovered() || open {
        ui.painter().rect_filled(rect, 6.0, p.hover.linear_multiply(0.6));
    }
    for (part, response) in [(main, &button), (side, &arrow)] {
        if response.hovered() {
            ui.painter().rect_filled(part, 6.0, p.hover);
        }
    }
    let color = if active {
        p.red
    } else if button.hovered() {
        p.strong
    } else {
        p.muted
    };
    icons::paint(ui, Rect::from_center_size(main.center(), Vec2::splat(20.0)), icon, color);
    let arrow_color = if arrow.hovered() || open { p.strong } else { p.muted };
    icons::paint(ui, Rect::from_center_size(side.center(), Vec2::splat(14.0)), Icon::ChevronDown, arrow_color);
    (button.on_hover_text(tooltip), arrow.on_hover_text(menu))
}

/// The system's own device, as the device lists name it.
pub fn default_device() -> &'static str {
    if cfg!(windows) { t!("Windows Default") } else { t!("System Default") }
}

/// A menu row with a second line under its label (the device in use, the
/// profile chosen) and a chevron, as Discord's device menus have.
fn detail_row(ui: &mut egui::Ui, label: &str, detail: &str) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let text = rect.shrink2(vec2(8.0, 4.0));
    paint_truncated(ui, Rect::from_min_max(text.min, pos2(text.right() - 22.0, text.center().y)), label, FontId::proportional(14.0), p.text);
    paint_truncated(ui, Rect::from_min_max(pos2(text.left(), text.center().y), pos2(text.right() - 22.0, text.bottom())), detail, FontId::proportional(12.0), p.muted);
    icons::paint_chevron_right(ui, Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(16.0)), p.muted);
    response
}

/// A choice in a submenu, with Discord's radio on the right.
fn radio_row(ui: &mut egui::Ui, label: &str, detail: &str, selected: bool) -> egui::Response {
    let p = theme::p();
    let height = if detail.is_empty() { 32.0 } else { 46.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let text = rect.shrink2(vec2(8.0, 6.0));
    let right = text.right() - 26.0;
    if detail.is_empty() {
        paint_truncated(ui, Rect::from_min_max(text.min, pos2(right, text.bottom())), label, FontId::proportional(14.0), p.text);
    } else {
        paint_truncated(ui, Rect::from_min_max(text.min, pos2(right, text.top() + 16.0)), label, FontId::proportional(14.0), p.text);
        let galley = ui.painter().layout(detail.to_owned(), FontId::proportional(12.0), p.muted, right - text.left());
        ui.painter().galley(pos2(text.left(), text.top() + 18.0), galley, p.muted);
    }
    let center = pos2(rect.right() - 18.0, rect.center().y);
    ui.painter().circle_stroke(center, 8.0, Stroke::new(2.0, if selected { p.accent } else { p.muted }));
    if selected {
        ui.painter().circle_filled(center, 4.0, p.accent);
    }
    response
}

/// The menu behind the arrow beside the microphone or the headphones, as
/// Discord's: the device (its list in a submenu), the input profile, the
/// volume, and the way to the voice settings.
fn device_menu(app: &mut App, arrow: &egui::Response, input: bool) {
    let p = theme::p();
    if arrow.clicked() {
        // Plugged in or out since the last look.
        app.devices = crate::voice::devices();
    }
    let changed = egui::Popup::menu(arrow)
        .align(egui::RectAlign::TOP_START)
        .gap(6.0)
        .show(|ui| {
            ui.set_width(250.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            let s = &app.settings;
            let before = (s.input_device.clone(), s.output_device.clone(), s.input_volume, s.output_volume, s.noise_suppression, s.automatic_sensitivity);
            let (inputs, outputs) = app.devices.clone();
            let devices = if input { inputs } else { outputs };
            let chosen = if input { app.settings.input_device.clone() } else { app.settings.output_device.clone() };
            let label = if input { t!("Input Device") } else { t!("Output Device") };
            let response = detail_row(ui, label, chosen.as_deref().unwrap_or(default_device()));
            egui::containers::menu::SubMenu::new().show(ui, &response, |ui| {
                ui.set_width(280.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                let choices = std::iter::once((default_device().to_owned(), None)).chain(devices.iter().map(|d| (d.clone(), Some(d.clone()))));
                for (name, value) in choices {
                    if radio_row(ui, &name, "", chosen == value).clicked() {
                        if input {
                            app.settings.input_device = value;
                        } else {
                            app.settings.output_device = value;
                        }
                        ui.close();
                    }
                }
            });
            if input {
                let current = InputProfile::of(&app.settings);
                let response = detail_row(ui, t!("Input Profile"), current.name());
                egui::containers::menu::SubMenu::new().show(ui, &response, |ui| {
                    ui.set_width(360.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for profile in [InputProfile::VoiceIsolation, InputProfile::Studio, InputProfile::Custom] {
                        if radio_row(ui, profile.name(), profile.detail(), current == profile).clicked() {
                            profile.apply(app);
                            ui.close();
                        }
                    }
                });
            }
            ui.add_space(4.0);
            ui.label(RichText::new(if input { t!("Input Volume") } else { t!("Output Volume") }).size(12.0).color(p.muted));
            let volume = if input { &mut app.settings.input_volume } else { &mut app.settings.output_volume };
            ui.spacing_mut().slider_width = 234.0;
            ui.visuals_mut().handle_shape = egui::style::HandleShape::Circle;
            ui.visuals_mut().selection.bg_fill = p.accent;
            ui.add(egui::Slider::new(volume, 0.0..=2.0).show_value(false).trailing_fill(true)).on_hover_text(format!("{:.0}%", *volume * 100.0));
            ui.add_space(4.0);
            let line = ui.cursor().top();
            ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.hover));
            ui.add_space(4.0);
            if menu_row(ui, t!("Voice Settings"), Some(Icon::Settings), None, false, p.text).clicked() {
                app.actions.push(Action::OpenSettings(SettingsTab::Voice));
                ui.close();
            }
            let s = &app.settings;
            before != (s.input_device.clone(), s.output_device.clone(), s.input_volume, s.output_volume, s.noise_suppression, s.automatic_sensitivity)
        })
        .is_some_and(|r| r.inner);
    if changed {
        app.actions.push(Action::SettingsChanged);
    }
}

/// Discord's input profiles, over CordFast's settings: voice isolation is
/// the noise suppression with the automatic threshold, studio is the
/// microphone untouched, anything else is custom.
#[derive(Clone, Copy, PartialEq)]
pub enum InputProfile {
    VoiceIsolation,
    Studio,
    Custom,
}

impl InputProfile {
    pub fn of(settings: &crate::settings::Settings) -> Self {
        match (settings.noise_suppression, settings.automatic_sensitivity) {
            (true, true) => Self::VoiceIsolation,
            (false, _) => Self::Studio,
            (true, false) => Self::Custom,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::VoiceIsolation => t!("Voice Isolation"),
            Self::Studio => t!("Studio"),
            Self::Custom => t!("Custom"),
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::VoiceIsolation => t!("Just your beautiful voice: let Discord cut through the noise"),
            Self::Studio => t!("Pure audio: open mic with no processing"),
            Self::Custom => t!("Advanced mode: give me all the buttons and dials!"),
        }
    }

    pub fn apply(self, app: &mut App) {
        match self {
            Self::VoiceIsolation => {
                app.settings.noise_suppression = true;
                app.settings.automatic_sensitivity = true;
            }
            Self::Studio => app.settings.noise_suppression = false,
            Self::Custom => app.actions.push(Action::OpenSettings(SettingsTab::Voice)),
        }
    }
}

/// The call at the top of the account card, as Discord's: the connection's
/// bars, "Voice Connected" and where, noise suppression and hang up; the ping
/// and region on hover.
fn voice_panel(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let Some((guild, channel)) = app.voice.channel else { return };
    let channel_name = app.channel(channel).map(Channel::title).unwrap_or_default();
    let mut place = match guild.and_then(|g| app.guild(g)) {
        Some(g) => format!("{channel_name} / {}", g.name),
        None => channel_name,
    };
    if app.plugin(Plugin::CallTimer)
        && let Some(since) = app.voice.since
    {
        let secs = since.elapsed().as_secs();
        place = format!("{place} · {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    }
    let (status, color) = if app.voice.connected {
        (t!("Voice Connected"), p.green)
    } else if let Some(error) = &app.voice.error {
        (error.as_str(), p.red)
    } else {
        (t!("Connecting…"), p.warning)
    };
    let status = status.to_owned();
    let connected = app.voice.connected;
    let region = app.voice.regions.first().cloned();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    // The connection's bars in a rounded square, the ping on hover.
    let bars = Rect::from_min_size(pos2(row.left(), row.center().y - 16.0), Vec2::splat(32.0));
    let bars_response = ui.interact(bars, ui.id().with("voice-ping"), Sense::hover());
    ui.painter().rect_filled(bars, 8.0, p.hover);
    for (i, height) in [5.0, 9.0, 13.0].into_iter().enumerate() {
        let x = bars.center().x - 6.0 + i as f32 * 5.0;
        ui.painter().rect_filled(Rect::from_min_max(pos2(x, bars.center().y + 6.0 - height), pos2(x + 3.0, bars.center().y + 6.0)), 1.0, color);
    }
    bars_response.on_hover_ui(|ui| {
        ui.set_min_width(220.0);
        match &region {
            Some((name, ms)) => {
                let quality = if *ms < 60 { (t!("Excellent"), p.green) } else if *ms < 140 { (t!("Good"), p.warning) } else { (t!("Bad"), p.red) };
                ui.label(RichText::new(t!("Average ping: {ms} ms", ms = ms)).font(theme::strong(13.5)).color(quality.1));
                ui.label(RichText::new(t!("Connection: {quality}", quality = quality.0)).size(12.5).color(p.text));
                ui.label(RichText::new(t!("Region: {region}", region = name)).size(12.5).color(p.muted));
            }
            None => {
                ui.label(RichText::new(t!("Measuring the ping…")).size(12.5).color(p.muted));
            }
        }
        ui.label(RichText::new(if connected { t!("End-to-end encrypted") } else { t!("Connecting…") }).size(12.0).color(p.muted));
    });
    let text_right = row.right() - 76.0;
    paint_truncated(ui, Rect::from_min_max(pos2(bars.right() + 8.0, row.top() + 4.0), pos2(text_right, row.center().y + 1.0)), &status, theme::strong(14.0), color);
    // Where we are, underlined on hover: it opens the channel.
    let place_rect = Rect::from_min_max(pos2(bars.right() + 8.0, row.center().y + 1.0), pos2(text_right, row.bottom() - 4.0));
    paint_truncated(ui, place_rect, &place, FontId::proportional(12.0), p.muted);
    let place_width = ui.painter().layout_no_wrap(place.clone(), FontId::proportional(12.0), p.muted).size().x.min(place_rect.width());
    let place_response = ui.interact(Rect::from_min_size(place_rect.min, vec2(place_width, place_rect.height())), ui.id().with("voice-place"), Sense::click());
    if place_response.hovered() {
        ui.painter().hline(place_rect.left()..=place_rect.left() + place_width, place_rect.center().y + 7.0, Stroke::new(1.0, p.muted));
    }
    if place_response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.actions.push(Action::OpenVoiceChannel { guild, channel });
    }
    // Noise suppression, then hang up, at the right end.
    let hang = Rect::from_center_size(pos2(row.right() - 18.0, row.center().y), Vec2::splat(32.0));
    let response = ui.interact(hang, ui.id().with("voice-leave"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(hang, 6.0, p.hover);
    }
    icons::paint(ui, Rect::from_center_size(hang.center(), Vec2::splat(20.0)), Icon::PhoneHangUp, if response.hovered() { p.red } else { p.muted });
    if response.on_hover_text(t!("Disconnect")).clicked() {
        app.actions.push(Action::LeaveVoice);
    }
    let noise = hang.translate(vec2(-36.0, 0.0));
    let response = ui.interact(noise, ui.id().with("voice-noise"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(noise, 6.0, p.hover);
    }
    let on = app.settings.noise_suppression;
    let wave = if on { p.strong } else { p.muted };
    for (i, height) in [6.0, 12.0, 16.0, 10.0, 6.0].into_iter().enumerate() {
        let x = noise.center().x - 8.0 + i as f32 * 4.0;
        ui.painter().line_segment([pos2(x, noise.center().y - height / 2.0), pos2(x, noise.center().y + height / 2.0)], Stroke::new(2.0, wave));
    }
    if !on {
        ui.painter().line_segment([noise.center() + vec2(-9.0, -9.0), noise.center() + vec2(9.0, 9.0)], Stroke::new(2.0, p.red));
    }
    let response = response.on_hover_text(if on { t!("Noise Suppression On") } else { t!("Noise Suppression Off") });
    noise_popout(app, &response);
    ui.add_space(2.0);
    gates::stage_row(app, ui);
    soundboard::buttons(app, ui);
    ui.add_space(6.0);
    let line = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.selected));
    ui.add_space(4.0);
}

/// Discord's noise suppression popout over the call panel: the switch with
/// what it does, and a mic test with its level, run while it is open (the
/// call neither hears us nor reaches us meanwhile).
fn noise_popout(app: &mut App, button: &egui::Response) {
    let p = theme::p();
    let id = egui::Popup::default_response_id(button);
    let shown = egui::Popup::from_toggle_button_response(button)
        .align(egui::RectAlign::TOP_START)
        .gap(8.0)
        .width(300.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(16)))
        .show(|ui| {
            ui.set_width(268.0);
            let mut on = app.settings.noise_suppression;
            // Discord's own words, less the name of its suppressor.
            if form::switch_row(ui, &mut on, t!("Noise Suppression"), t!("Make some noise—like clapping your hands—while speaking. Your friends will hear nothing but your beautiful voice.")) {
                app.settings.noise_suppression = on;
                app.actions.push(Action::SettingsChanged);
            }
            ui.add_space(12.0);
            ui.label(RichText::new(t!("Mic Test").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let label = if app.mic_test { t!("Stop") } else { t!("Test") };
                if form::button(ui, label, form::Kind::Secondary).clicked() {
                    app.mic_test = !app.mic_test;
                    app.commands.send(crate::backend::Command::Voice(crate::voice::VoiceCommand::MicTest(app.mic_test)));
                }
                // The microphone's level as the call would get it.
                let (meter, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
                ui.painter().rect_filled(meter, 4.0, p.selected);
                if app.mic_test {
                    let level = ((app.voice.mic_level + 100.0) / 100.0).clamp(0.0, 1.0);
                    ui.painter().rect_filled(Rect::from_min_size(meter.min, vec2(meter.width() * level, meter.height())), 4.0, p.green);
                }
            });
        });
    // Closing the popout ends a test it started.
    if shown.is_none() && !egui::Popup::is_id_open(&button.ctx, id) && app.mic_test && app.page.is_none() {
        app.mic_test = false;
        app.commands.send(crate::backend::Command::Voice(crate::voice::VoiceCommand::MicTest(false)));
    }
}

fn account(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let Some(me) = app.me.clone() else { return };
    ui.horizontal_centered(|ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
        app.images.circle(ui, rect, Some(&me.avatar_url()), &initials(me.display_name()), 16.0);
        match app.connection {
            Connection::Connected => friends::dot(ui, rect, &app.status, p.account),
            Connection::Connecting => friends::dot(ui, rect, "idle", p.account),
            Connection::Disconnected => friends::dot(ui, rect, "dnd", p.account),
        }
        // No speaking ring here: Discord shows it in the channel's list,
        // and over the status dot it hid it.
        if response.clicked() {
            // Over the account card, its left edge with the card's.
            let card = ui.data(|d| d.get_temp::<Rect>(egui::Id::new("account-card-rect"))).unwrap_or(ui.max_rect());
            app.actions.push(Action::OpenOwnProfile { at: card.left_top() });
        }
        ui.vertical(|ui| {
            // Room for the two split buttons and the cog.
            ui.set_width((ui.available_width() - 138.0).max(40.0));
            ui.add_space(2.0);
            ui.add(Label::new(RichText::new(me.display_name()).font(theme::strong(14.0)).color(p.strong)).truncate());
            // Discord's panel: your activity status (the green controller and
            // the game, "•" and the custom status), the username while the
            // panel is hovered or when there is nothing to show.
            let hovered = ui.ctx().pointer_hover_pos().is_some_and(|at| ui.data(|d| d.get_temp::<Rect>(egui::Id::new("account-card-rect"))).is_some_and(|card| card.contains(at)));
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
            match (app.connection, &app.activity) {
                (Connection::Connecting, _) => paint_truncated(ui, line, t!("Connecting…"), FontId::proportional(12.0), p.muted),
                (Connection::Disconnected, _) => paint_truncated(ui, line, t!("No connection"), FontId::proportional(12.0), p.muted),
                (_, game) if !hovered && (game.is_some() || app.custom_status.is_some()) => {
                    let mut x = line.left();
                    if let Some(game) = game {
                        icons::paint(ui, Rect::from_center_size(pos2(x + 6.0, line.center().y), Vec2::splat(12.0)), Icon::Controller, p.green);
                        x += 16.0;
                        // Small lists keep the icon alone beside words of a custom status.
                        if app.custom_status.is_none() {
                            paint_truncated(ui, Rect::from_min_max(pos2(x, line.top()), line.max), game, FontId::proportional(12.0), p.muted);
                        }
                    }
                    if let Some(custom) = &app.custom_status {
                        let text = if game.is_some() { format!("• {custom}") } else { custom.clone() };
                        paint_truncated(ui, Rect::from_min_max(pos2(x, line.top()), line.max), &text, FontId::proportional(12.0), p.muted);
                    }
                }
                _ => paint_truncated(ui, line, &me.username, FontId::proportional(12.0), p.muted),
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_button(ui, Icon::Settings, t!("User Settings"), false) {
                app.actions.push(Action::OpenSettings(SettingsTab::Account));
            }
            let deaf = app.voice.deaf;
            let headphones = if deaf { Icon::HeadphonesSlash } else { Icon::Headphones };
            let (button, arrow) = split_button(ui, headphones, if deaf { t!("Undeafen") } else { t!("Deafen") }, t!("Output Options"), deaf);
            if button.clicked() {
                app.actions.push(Action::ToggleDeaf);
            }
            device_menu(app, &arrow, false);
            let mute = app.voice.mute;
            let mic = if mute { Icon::MicrophoneSlash } else { Icon::Microphone };
            let (button, arrow) = split_button(ui, mic, if mute { t!("Unmute") } else { t!("Mute") }, t!("Input Options"), mute);
            if button.clicked() {
                app.actions.push(Action::ToggleMute);
            }
            device_menu(app, &arrow, true);
        });
    });
}

// ---- overlays -------------------------------------------------------------------

fn incoming_call(app: &mut App, ctx: &egui::Context) {
    let Some(channel) = app.incoming_call() else { return };
    let p = theme::p();
    let title = app.channel(channel).map(Channel::title).unwrap_or_else(|| t!("Someone").into());
    let avatar = app
        .channel(channel)
        .and_then(|c| c.recipients.first())
        .map(|u| u.avatar_url());
    egui::Area::new("incoming-call".into())
        .anchor(Align2::CENTER_TOP, vec2(0.0, 24.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            Frame::new()
                .fill(p.rail)
                .corner_radius(12)
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_width(260.0);
                    ui.vertical_centered(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
                        app.images.circle(ui, rect, avatar.as_deref(), &initials(&title), 36.0);
                        ui.painter().circle_stroke(rect.center(), 39.0, Stroke::new(3.0, p.green));
                        ui.add_space(8.0);
                        ui.label(RichText::new(&title).font(theme::strong(17.0)).color(p.strong));
                        ui.label(RichText::new(t!("is calling you…")).color(p.muted));
                        ui.add_space(10.0);
                        // Discord's two round buttons: hang up in red,
                        // answer in green (Ctrl+Enter answers too).
                        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
                        for (x, accept) in [(-34.0, false), (34.0, true)] {
                            let rect = Rect::from_center_size(row.center() + vec2(x, 0.0), Vec2::splat(48.0));
                            let response = ui.interact(rect, ui.id().with(("incoming", accept)), Sense::click());
                            let fill = if accept { p.green } else { p.red };
                            ui.painter().circle_filled(rect.center(), 24.0, if response.hovered() { fill.gamma_multiply(0.85) } else { fill });
                            let icon = if accept { Icon::PhoneCall } else { Icon::PhoneHangUp };
                            icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)), icon, Color32::WHITE);
                            if response.on_hover_text(if accept { tp!("call", "Accept") } else { t!("Decline") }).clicked() {
                                app.actions.push(if accept { Action::AcceptCall(channel) } else { Action::DeclineCall(channel) });
                            }
                        }
                    });
                });
        });
}

/// Ctrl+K: jump to any channel or DM by name.
fn switcher(app: &mut App, ctx: &egui::Context) {
    let Some(mut query) = app.switcher.take() else { return };
    let p = theme::p();
    let me = app.me_id();
    let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
    let needle = query.to_lowercase();
    let mut results: Vec<(Option<Id>, Id, String, String)> = Vec::new();
    for dm in &app.dms {
        let title = dm.title();
        if title.to_lowercase().contains(&needle) {
            results.push((None, dm.id, format!("@{title}"), String::new()));
        }
    }
    for guild in &app.guilds {
        for channel in &guild.channels {
            if matches!(channel.kind, ChannelKind::Text | ChannelKind::Announcement | ChannelKind::Voice)
                && channel.name.to_lowercase().contains(&needle)
                && guild.can_view(me, channel)
            {
                results.push((Some(guild.id), channel.id, format!("#{}", channel.name), guild.name.clone()));
            }
        }
    }
    results.truncate(12);
    let mut chosen = None;
    egui::Modal::new("switcher".into()).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new(t!("Where would you like to go?")).font(theme::strong(16.0)).color(p.strong));
        let response = ui.add(egui::TextEdit::singleline(&mut query).hint_text(t!("Channel, server or conversation")).desired_width(f32::INFINITY));
        response.request_focus();
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            chosen = results.first().map(|(g, c, _, _)| (*g, *c));
        }
        ui.add_space(6.0);
        for (guild, channel, name, place) in &results {
            let response = ui.add(egui::Button::new(
                RichText::new(format!("{name}   {place}")).color(p.text),
            ).frame(false));
            if response.clicked() {
                chosen = Some((*guild, *channel));
            }
        }
    });
    if let Some((guild, channel)) = chosen {
        app.actions.push(Action::SelectGuild(guild));
        app.actions.push(Action::SelectChannel(channel));
        close = true;
    }
    if !close {
        app.switcher = Some(query);
    }
}

/// Discord's keyboard shortcuts.
fn shortcuts(app: &mut App, ui: &mut egui::Ui) {
    use crate::keybinds::{Shortcut, pressed};
    if pressed(ui, &app.settings, Shortcut::ToggleMute) {
        app.actions.push(Action::ToggleMute);
    }
    if pressed(ui, &app.settings, Shortcut::ToggleDeaf) {
        app.actions.push(Action::ToggleDeaf);
    }
    // The mouse's side buttons go back and forward, as in Discord.
    let (back, forward) = ui.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
    if back || pressed(ui, &app.settings, Shortcut::Back) {
        app.actions.push(Action::Back);
    } else if forward || pressed(ui, &app.settings, Shortcut::Forward) {
        app.actions.push(Action::Forward);
    }
    if pressed(ui, &app.settings, Shortcut::DmsAndServer) {
        match app.selected_guild {
            Some(_) => app.actions.push(Action::SelectGuild(None)),
            None => {
                if let Some(guild) = app.back.iter().rev().find_map(|p| p.guild) {
                    app.actions.push(Action::SelectGuild(Some(guild)));
                }
            }
        }
    }
    if (pressed(ui, &app.settings, Shortcut::ReturnToVoice) | pressed(ui, &app.settings, Shortcut::JumpToCall))
        && let Some((guild, channel)) = app.voice.channel
    {
        app.actions.push(Action::SelectGuild(guild));
        app.actions.push(Action::SelectChannel(channel));
    }
    if pressed(ui, &app.settings, Shortcut::AnswerCall)
        && let Some(channel) = app.incoming_call()
    {
        app.actions.push(Action::AcceptCall(channel));
    }
    if pressed(ui, &app.settings, Shortcut::StartCall)
        && let Some(channel) = app.selected_channel.filter(|c| app.channel(*c).is_some_and(|ch| ch.guild_id.is_none()))
    {
        app.actions.push(Action::JoinVoice { guild: None, channel, ring: true });
    }
    if pressed(ui, &app.settings, Shortcut::CopyChannelLink)
        && let Some(channel) = app.selected_channel
    {
        let place = app.channel(channel).and_then(|c| c.guild_id).map_or_else(|| "@me".to_owned(), |g| g.to_string());
        ui.ctx().copy_text(format!("https://discord.com/channels/{place}/{channel}"));
        app.notify(t!("Link copied"));
    }
    // Discord's: collapse every category of the open server, or open them
    // all again when they already are.
    if pressed(ui, &app.settings, Shortcut::ToggleCategories)
        && let Some(guild) = app.selected_guild.and_then(|g| app.guild(g))
    {
        let categories: Vec<Id> = guild.channels.iter().filter(|c| c.kind == ChannelKind::Category).map(|c| c.id).collect();
        if categories.iter().any(|c| !app.collapsed.contains(c)) {
            app.collapsed.extend(categories);
        } else {
            for c in categories {
                app.collapsed.remove(&c);
            }
        }
    }
    let zoom = app.settings.zoom;
    if pressed(ui, &app.settings, Shortcut::ZoomIn) {
        app.settings.zoom = (zoom + 0.1).min(2.0);
    }
    if pressed(ui, &app.settings, Shortcut::ZoomOut) {
        app.settings.zoom = (zoom - 0.1).max(0.6);
    }
    if pressed(ui, &app.settings, Shortcut::ZoomReset) {
        app.settings.zoom = 1.0;
    }
    if app.settings.zoom != zoom {
        app.actions.push(Action::SettingsChanged);
    }
    if pressed(ui, &app.settings, Shortcut::Pins) {
        app.toggle_pins();
    }
    if pressed(ui, &app.settings, Shortcut::Inbox) {
        app.toggle_inbox();
    }
    if pressed(ui, &app.settings, Shortcut::Members) {
        app.toggle_members();
    }
    if pressed(ui, &app.settings, Shortcut::Search) {
        app.toggle_search();
    }
    if app.selected_channel.is_some() {
        let now = ui.input(|i| i.time);
        for (shortcut, tab) in [(Shortcut::Emoji, crate::app::ExpressionTab::Emoji), (Shortcut::Gif, crate::app::ExpressionTab::Gifs), (Shortcut::Sticker, crate::app::ExpressionTab::Stickers)] {
            if pressed(ui, &app.settings, shortcut) {
                app.toggle_expressions(tab, crate::app::PickerTarget::Composer, now);
            }
        }
        if pressed(ui, &app.settings, Shortcut::Upload) {
            app.actions.push(Action::AttachFiles);
        }
    }
    if pressed(ui, &app.settings, Shortcut::CreateServer) {
        app.modal = Some(crate::app::Modal::AddServer { invite: String::new(), name: String::new() });
    }
    if pressed(ui, &app.settings, Shortcut::NewGroup) {
        app.modal = Some(crate::app::Modal::NewDm { picked: Vec::new(), filter: String::new() });
    }
    if pressed(ui, &app.settings, Shortcut::Help) {
        let _ = open::that_detached(crate::i18n::current().help_url());
    }
    if pressed(ui, &app.settings, Shortcut::Shortcuts) {
        app.actions.push(Action::OpenSettings(SettingsTab::System));
    }
    // Most modifiers first: a pattern matches with extra Shift held, so
    // Ctrl+Alt+Shift (mentions) goes before Ctrl+Alt (servers), and
    // Alt+Shift (unread) before Alt (channels).
    let mention = if pressed(ui, &app.settings, Shortcut::PrevMention) {
        Some(-1)
    } else if pressed(ui, &app.settings, Shortcut::NextMention) {
        Some(1)
    } else {
        None
    };
    let server = if mention.is_some() {
        None
    } else if pressed(ui, &app.settings, Shortcut::PrevServer) {
        Some(-1)
    } else if pressed(ui, &app.settings, Shortcut::NextServer) {
        Some(1)
    } else {
        None
    };
    if let Some(delta) = server {
        let mut order: Vec<Option<Id>> = vec![None];
        for entry in rail_entries(app) {
            match entry {
                RailEntry::Guild(id) => order.push(Some(id)),
                RailEntry::Folder { guilds, .. } => order.extend(guilds.into_iter().map(Some)),
            }
        }
        let at = order.iter().position(|g| *g == app.selected_guild).unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(order.len() as i32) as usize;
        app.actions.push(Action::SelectGuild(order[next]));
        return;
    }
    // 0 every channel, 1 unread ones, 2 those that mention you.
    let (delta, only) = if let Some(delta) = mention {
        (delta, 2)
    } else if pressed(ui, &app.settings, Shortcut::PrevUnread) {
        (-1, 1)
    } else if pressed(ui, &app.settings, Shortcut::NextUnread) {
        (1, 1)
    } else if pressed(ui, &app.settings, Shortcut::PrevChannel) {
        (-1, 0)
    } else if pressed(ui, &app.settings, Shortcut::NextChannel) {
        (1, 0)
    } else {
        return;
    };
    let channels: Vec<Id> = match app.selected_guild {
        Some(guild) => ordered_channels(app, guild)
            .into_iter()
            .flat_map(|(_, list)| list)
            .filter(|(c, visible)| *visible && matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement | ChannelKind::Thread))
            .map(|(c, _)| c.id)
            .collect(),
        None => {
            let mut dms: Vec<(Id, Id)> = app
                .dms
                .iter()
                .map(|c| (app.latest.get(&c.id).copied().unwrap_or(c.id), c.id))
                .collect();
            dms.sort_unstable_by(|a, b| b.cmp(a));
            dms.into_iter().map(|(_, id)| id).collect()
        }
    };
    if channels.is_empty() {
        return;
    }
    let at = app
        .selected_channel
        .and_then(|c| channels.iter().position(|x| *x == c))
        .map_or(-1, |i| i as i32);
    let len = channels.len() as i32;
    for offset in 1..=len {
        let index = (at + delta * offset).rem_euclid(len) as usize;
        let id = channels[index];
        let wanted = match only {
            0 => true,
            1 => app.is_unread(id) || app.mentions(id) > 0,
            _ => app.mentions(id) > 0,
        };
        if wanted {
            app.actions.push(Action::SelectChannel(id));
            break;
        }
    }
}

/// Discord's green download button at the top of the server list: shown
/// while an update downloads, and a click restarts into it once ready.
fn update_button(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let (label, ready) = match &*app.updates.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        crate::updates::State::Ready { version, .. } => (t!("Update ready ({version}): restart", version = version), true),
        crate::updates::State::Downloading { version, percent } => (t!("Downloading version {version}: {percent}%", version = version, percent = percent), false),
        _ => return,
    };
    let response = rail_item(app, ui, false, false, 0, &label, |_, ui, rect, r| {
        ui.painter().rect_filled(rect, r, if ready { p.green } else { p.selected });
        // A download arrow.
        let c = rect.center();
        let stroke = Stroke::new(2.5, Color32::WHITE);
        ui.painter().line_segment([c + vec2(0.0, -10.0), c + vec2(0.0, 6.0)], stroke);
        ui.painter().line_segment([c + vec2(-7.0, -1.0), c + vec2(0.0, 6.0)], stroke);
        ui.painter().line_segment([c + vec2(7.0, -1.0), c + vec2(0.0, 6.0)], stroke);
        ui.painter().line_segment([c + vec2(-9.0, 11.0), c + vec2(9.0, 11.0)], stroke);
    });
    if ready && response.clicked() {
        app.actions.push(Action::InstallUpdate);
    }
}

/// Discord's "03 / 35" beside a voice channel with a limit, right-aligned
/// at `right`; returns its left edge.
fn count_pill(ui: &egui::Ui, right: egui::Pos2, count: u64, limit: u64) -> f32 {
    let p = theme::p();
    let left_text = format!("{count:02}");
    let right_text = format!("{limit:02}");
    let font = FontId::monospace(11.5);
    let a = ui.painter().layout_no_wrap(left_text, font.clone(), p.text);
    let b = ui.painter().layout_no_wrap(right_text, font, p.muted);
    let half = a.size().x.max(b.size().x) + 8.0;
    let rect = Rect::from_min_max(pos2(right.x - half * 2.0, right.y - 9.0), pos2(right.x, right.y + 9.0));
    ui.painter().rect_filled(rect, 9.0, p.rail);
    ui.painter().rect_filled(Rect::from_min_max(pos2(rect.center().x, rect.top()), rect.max), egui::CornerRadius { ne: 9, se: 9, nw: 0, sw: 0 }, p.selected);
    ui.painter().galley(pos2(rect.left() + 4.0, rect.center().y - a.size().y / 2.0), a, p.text);
    ui.painter().galley(pos2(rect.center().x + 4.0, rect.center().y - b.size().y / 2.0), b, p.muted);
    rect.left()
}

/// A row of a menu card, as Discord's: an icon or status, the label, and
/// a chevron when it opens a submenu.
pub fn menu_row(ui: &mut egui::Ui, label: &str, icon: Option<Icon>, status: Option<&str>, submenu: bool, color: Color32) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let lead = rect.left_center() + vec2(16.0, 0.0);
    if let Some(icon) = icon {
        icons::paint(ui, Rect::from_center_size(lead, Vec2::splat(18.0)), icon, color);
    }
    if let Some(status) = status {
        icons::paint_status(ui, Rect::from_center_size(lead, Vec2::splat(12.0)), status);
    }
    let right = if submenu { rect.right() - 30.0 } else { rect.right() - 8.0 };
    paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 34.0, rect.top()), pos2(right, rect.bottom())), label, FontId::proportional(14.0), color);
    if submenu {
        icons::paint_chevron_right(ui, Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(16.0)), p.muted);
    }
    response
}

/// The status row of your own profile's first card, as Discord's: each
/// choice and how long it lasts in submenus. The custom status is the
/// bubble beside the avatar.
pub fn me_menu(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let durations: [(&str, Option<u64>); 6] = [
        (n!("For 15 Minutes"), Some(15)),
        (n!("For 1 Hour"), Some(60)),
        (n!("For 8 Hours"), Some(480)),
        (n!("For 24 Hours"), Some(1440)),
        (n!("For 3 Days"), Some(4320)),
        (n!("Forever"), None),
    ];
    let status = app.status.clone();
    let response = menu_row(ui, friends::status_name(&status), None, Some(&status), true, p.text);
    menu::flyout(ui, &response, 300.0, |ui| {
        // Discord's status menu: each status but "online" can last a
        // while, chosen in its submenu.
        let row = |ui: &mut egui::Ui, status: &str, detail: &str| -> egui::Response {
            // The note wraps (two lines in longer languages) and the row
            // grows with it.
            let note = (!detail.is_empty()).then(|| ui.painter().layout(detail.to_owned(), FontId::proportional(11.5), Color32::PLACEHOLDER, ui.available_width() - 40.0));
            let height = note.as_ref().map_or(32.0, |note| 27.0 + note.size().y + 7.0);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4.0, p.accent);
            }
            let text = if response.hovered() { Color32::WHITE } else { p.text };
            let icon = Rect::from_center_size(pos2(rect.left() + 16.0, rect.top() + 16.0), Vec2::splat(12.0));
            icons::paint_status(ui, icon, status);
            ui.painter().text(pos2(rect.left() + 32.0, rect.top() + 16.0), Align2::LEFT_CENTER, friends::status_name(status), FontId::proportional(14.0), text);
            if let Some(note) = note {
                ui.painter().galley(pos2(rect.left() + 32.0, rect.top() + 27.0), note, if response.hovered() { Color32::WHITE } else { p.muted });
            }
            response
        };
        if row(ui, "online", "").clicked() {
            app.actions.push(Action::SetStatus("online", None));
            ui.close();
        }
        for (status, detail) in [
            ("idle", ""),
            ("dnd", t!("You will not receive desktop notifications")),
            ("invisible", t!("You will not appear online, but will have full access to Discord")),
        ] {
            let response = row(ui, status, detail);
            // Opens on hover, as Discord's duration menu does.
            egui::containers::menu::SubMenu::new().show(ui, &response, |ui| {
                ui.set_min_width(170.0);
                for (label, minutes) in durations {
                    if menu::item(ui, crate::i18n::tr(label), menu::Tail::None).clicked() {
                        app.actions.push(Action::SetStatus(status, minutes));
                        ui.close();
                    }
                }
            });
        }
    });
}

/// The second card under your own profile: switching accounts and your ID.
pub fn me_accounts(app: &mut App, ui: &mut egui::Ui, me: &crate::discord::model::User) {
    let p = theme::p();
    // Discord's account switcher.
    let accounts = app.settings.accounts.clone();
    let response = menu_row(ui, t!("Switch Accounts"), Some(Icon::Switch), None, true, p.text);
    menu::flyout(ui, &response, 260.0, |ui| {
        for account in &accounts {
            let current = account.id == me.id.to_string();
            let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 4.0, p.hover);
            }
            let avatar = Rect::from_min_size(pos2(row.left() + 6.0, row.center().y - 14.0), Vec2::splat(28.0));
            app.images.circle(ui, avatar, Some(&account.avatar), &initials(&account.name), 14.0);
            paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 8.0, row.top() + 3.0), pos2(row.right() - 30.0, row.center().y)), &account.name, theme::strong(13.5), p.strong);
            paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 8.0, row.center().y), pos2(row.right() - 30.0, row.bottom() - 3.0)), &account.username, FontId::proportional(12.0), p.muted);
            if current {
                icons::paint(ui, Rect::from_center_size(row.right_center() - vec2(14.0, 0.0), Vec2::splat(16.0)), Icon::Check, p.green);
            }
            if response.clicked() && !current {
                if let Ok(id) = account.id.parse() {
                    app.actions.push(Action::SwitchAccount(id));
                }
                ui.close();
            }
            let id = account.id.clone();
            response.context_menu(|ui| {
                menu::begin(ui);
                if menu::danger(ui, t!("Remove From This List"), menu::Tail::None).clicked() {
                    app.actions.push(Action::RemoveAccount(id.clone()));
                    ui.close();
                }
            });
        }
        menu::separator(ui);
        if menu::item(ui, t!("Add an Account"), menu::Tail::None).clicked() {
            app.actions.push(Action::AddAccount);
            ui.close();
        }
        if menu::danger(ui, t!("Log Out of This Account"), menu::Tail::None).clicked() {
            app.actions.push(Action::Logout);
            ui.close();
        }
    });
    if menu_row(ui, t!("Copy User ID"), Some(Icon::Id), None, false, p.text).clicked() {
        ui.ctx().copy_text(me.id.to_string());
        app.notify(t!("ID copied"));
    }
}
