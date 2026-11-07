//! Discord's notice bar over the app (the bundle's `Notice`, `NoticeStore`):
//! one bar at a time, the first of its priority list that applies, 36 points
//! tall, its text centred with an outlined button after it and a close
//! button at the right end. Colours are the theme's `--notice-*` tokens.

use egui::{Color32, FontId, Frame, Rect, Sense, Stroke, Vec2, pos2, vec2};
use serde_json::Value;

use crate::app::{Action, App, SettingsTab};
use crate::icons::{self, Icon};
use crate::theme;

/// The notices this client raises, in Discord's priority order (`cq`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Not Discord's list: the call went on without us (another device
    /// took it, or it ended), with "Reconnect".
    VoiceEnded,
    PttNoKeybind,
    VoiceConnectedLastSession,
    StreamerMode,
    UnverifiedAccount,
    PremiumPastDueInvalidPayment,
    PremiumPastDueMissingPayment,
}

/// The bar's colour scheme (`colorDefault`, `colorDanger`…).
#[derive(Clone, Copy)]
enum Color {
    Default,
    Danger,
    Warning,
    StreamerMode,
}

impl Color {
    /// (background, text): dark theme `--notice-background-*` and
    /// `--notice-text-*` (green-new-77 and 3, red-new-75 and 1, yellow-new-84
    /// and 1), the light theme's the other way round; Twitch's purple.
    fn colors(self, dark: bool) -> (Color32, Color32) {
        let rgb = |c: u32| Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
        match (self, dark) {
            (Color::Default, true) => (rgb(0x00431e), rgb(0xc1fcd1)),
            (Color::Default, false) => (rgb(0xc8ffd8), rgb(0x00461f)),
            (Color::Danger, true) => (rgb(0x730e1b), rgb(0xffdedd)),
            (Color::Danger, false) => (rgb(0xffdedd), rgb(0x690917)),
            (Color::Warning, true) => (rgb(0x622a00), rgb(0xffe7cf)),
            (Color::Warning, false) => (rgb(0xffe7cf), rgb(0x5e2700)),
            (Color::StreamerMode, _) => (rgb(0x9147ff), Color32::WHITE),
        }
    }
}

struct Notice {
    kind: Kind,
    color: Color,
    text: String,
    button: Option<String>,
    /// Discord's close button (`NoticeCloseButton`); some bars have none.
    close: bool,
}

/// Nitro's subscription when payment is past due: (its payment source,
/// paid through Apple or Google).
fn past_due(app: &App) -> Option<(Option<String>, bool)> {
    let Some(Some(Ok(list))) = app.settings_data.get("settings:subscriptions") else { return None };
    let sub = list.as_array()?.iter().find(|s| s.get("type").and_then(Value::as_u64) == Some(1))?;
    // SubscriptionStatus.PAST_DUE, and the period not over yet.
    if sub.get("status").and_then(Value::as_u64) != Some(2) {
        return None;
    }
    let end = sub.get("current_period_end").and_then(Value::as_str).and_then(|t| t.parse::<jiff::Timestamp>().ok())?;
    if end < jiff::Timestamp::now() {
        return None;
    }
    let external = matches!(sub.get("payment_gateway").and_then(Value::as_u64), Some(3 | 4));
    Some((sub.get("payment_source_id").and_then(Value::as_str).map(str::to_owned), external))
}

fn source_invalid(app: &App, id: &str) -> bool {
    let Some(Some(Ok(list))) = app.settings_data.get("settings:payment-sources") else { return false };
    list.as_array().into_iter().flatten().any(|s| s.get("id").and_then(Value::as_str) == Some(id) && s.get("invalid").and_then(Value::as_bool) == Some(true))
}

/// The first bar that applies, as `NoticeStore` picks it.
fn pick(app: &App) -> Option<Notice> {
    let open = |kind: Kind| !app.dismissed_notices.contains(&kind);
    let notice = |kind, color, text: &str, button: Option<&str>, close| Notice { kind, color, text: text.to_owned(), button: button.map(str::to_owned), close };
    if let Some(ended) = app.voice_notice {
        let text = if ended.elsewhere { t!("Your voice has been disconnected because you connected at another location.") } else { t!("Your voice has been disconnected.") };
        return Some(notice(Kind::VoiceEnded, Color::Danger, text, Some(t!("Reconnect")), true));
    }
    if app.settings.push_to_talk && app.settings.push_to_talk_key == 0 && open(Kind::PttNoKeybind) {
        return Some(notice(Kind::PttNoKeybind, Color::Danger, t!("Push-to-talk is enabled, but no keybind is set."), Some(t!("Set Keybind")), false));
    }
    if app.voice.channel.is_none() && app.last_session_voice.is_some_and(|(_, c)| app.channel(c).is_some()) && open(Kind::VoiceConnectedLastSession) {
        return Some(notice(Kind::VoiceConnectedLastSession, Color::Default, t!("You were in a voice channel last time you left Discord."), Some(t!("Reconnect")), true));
    }
    if app.streamer_on() && open(Kind::StreamerMode) {
        return Some(notice(Kind::StreamerMode, Color::StreamerMode, t!("Streamer Mode is enabled. Usernames have been truncated."), Some(t!("Disable")), true));
    }
    if app.unverified {
        return Some(notice(Kind::UnverifiedAccount, Color::Default, t!("Please check your email to verify your account and keep your current username."), Some(t!("Resend Email")), false));
    }
    if let Some((source, _)) = past_due(app).filter(|(_, external)| !*external) {
        match source {
            Some(id) if source_invalid(app, &id) && open(Kind::PremiumPastDueInvalidPayment) => {
                return Some(notice(Kind::PremiumPastDueInvalidPayment, Color::Danger, t!("Your Nitro subscription payment failed, please update your payment details."), Some(t!("Update Now")), true));
            }
            None if open(Kind::PremiumPastDueMissingPayment) => {
                return Some(notice(Kind::PremiumPastDueMissingPayment, Color::Warning, t!("Your Nitro sub is about to run out. Add a payment method to keep using profile customisation and more."), Some(t!("Add Payment")), true));
            }
            _ => {}
        }
    }
    None
}

fn act(app: &mut App, kind: Kind) {
    match kind {
        Kind::VoiceEnded => {
            if let Some(ended) = app.voice_notice.take() {
                app.actions.push(Action::JoinVoice { guild: ended.guild, channel: ended.channel, ring: false });
            }
        }
        Kind::PttNoKeybind => app.actions.push(Action::OpenSettings(SettingsTab::Voice)),
        Kind::VoiceConnectedLastSession => {
            if let Some((guild, channel)) = app.last_session_voice.take() {
                app.actions.push(Action::JoinVoice { guild, channel, ring: false });
            }
        }
        Kind::StreamerMode => {
            // `setEnabled(false)`: off, the automatic one too until the
            // streaming app is seen starting again.
            app.settings.streamer_mode = false;
            app.streaming_app = false;
            app.actions.push(Action::SettingsChanged);
        }
        Kind::UnverifiedAccount => {
            app.commands.send(crate::backend::Command::Api { key: "verify-resend".into(), method: "POST", path: "/auth/verify/resend".into(), body: Some(serde_json::json!({})) });
            let email = app.email.clone().unwrap_or_default();
            app.notify(&t!("We have sent you a new verification email to {email}, please check both your inbox and spam folder.", email = email));
        }
        Kind::PremiumPastDueInvalidPayment | Kind::PremiumPastDueMissingPayment => {
            app.actions.push(Action::OpenSettings(SettingsTab::Subscriptions));
        }
    }
}

fn close(app: &mut App, kind: Kind) {
    match kind {
        Kind::VoiceEnded => app.voice_notice = None,
        Kind::VoiceConnectedLastSession => {
            app.last_session_voice = None;
            app.settings.last_voice = None;
            app.actions.push(Action::SettingsChanged);
        }
        _ => {}
    }
    app.dismissed_notices.push(kind);
}

/// The bar, above everything under the title bar.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    app.notice_bar = false;
    if !matches!(app.auth, crate::app::AuthView::LoggedIn) {
        return;
    }
    let Some(notice) = pick(app) else { return };
    app.notice_bar = true;
    let p = theme::p();
    let (fill, text) = notice.color.colors(p.dark);
    egui::Panel::top("notice-bar").exact_size(36.0).show_separator_line(false).frame(Frame::new().fill(p.rail)).show(ui, |ui| {
        // Right of the server rail (the layout's `notice` area, drawn after
        // the rail), its top-left corner rounded 8 with the app frame's
        // border along the top and left.
        let rect = ui.max_rect();
        let corner = egui::CornerRadius { nw: 8, ..Default::default() };
        ui.painter().rect_filled(rect, corner, fill);
        let (x, y) = (rect.left() + 0.5, rect.top() + 0.5);
        let mut edge = vec![pos2(x, rect.bottom())];
        for step in 0..=8 {
            let angle = std::f32::consts::PI * (1.0 + step as f32 / 16.0);
            edge.push(pos2(x + 8.0 + 8.0 * angle.cos(), y + 8.0 + 8.0 * angle.sin()));
        }
        edge.push(pos2(rect.right(), y));
        ui.painter().add(egui::Shape::line(edge, Stroke::new(1.0, p.border)));
        // 14-point medium text; the button 24 tall, 10 to each side of its
        // label, 10 after the text, a 1-point border radius 3.
        let message = ui.painter().layout_no_wrap(notice.text.clone(), FontId::proportional(14.0), text);
        let label = notice.button.as_ref().map(|b| ui.painter().layout_no_wrap(b.clone(), FontId::proportional(14.0), text));
        let width = message.size().x + label.as_ref().map_or(0.0, |l| 10.0 + l.size().x + 20.0);
        let x = (rect.center().x - width / 2.0).max(rect.left() + 4.0);
        ui.painter().galley(pos2(x, rect.center().y - message.size().y / 2.0), message.clone(), text);
        if let Some(label) = label {
            let at = Rect::from_min_size(pos2(x + message.size().x + 10.0, rect.center().y - 12.0), vec2(label.size().x + 20.0, 24.0));
            let button = ui.interact(at, ui.id().with(("notice-button", notice.kind as u8)), Sense::click());
            // Hovered, `--background-mod-subtle` behind the label.
            if button.hovered() {
                ui.painter().rect_filled(at, 3.0, text.gamma_multiply(0.12));
            }
            ui.painter().rect_stroke(at, 3.0, Stroke::new(1.0, text), egui::StrokeKind::Inside);
            ui.painter().galley(at.center() - label.size() / 2.0, label, text);
            if button.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                act(app, notice.kind);
            }
        }
        if notice.close {
            let at = Rect::from_min_size(pos2(rect.right() - 36.0, rect.top()), Vec2::splat(36.0));
            let button = ui.interact(at, ui.id().with(("notice-close", notice.kind as u8)), Sense::click());
            icons::paint(ui, Rect::from_center_size(at.center(), Vec2::splat(18.0)), Icon::Close, if button.hovered() { text } else { text.gamma_multiply(0.8) });
            if button.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                close(app, notice.kind);
            }
        }
    });
}
