//! Discord's Message Requests page (`MESSAGE_REQUESTS` in the bundle): people
//! who are no friends and wrote first wait here until accepted. The header's
//! envelope, title and the Requests and Spam pills; "Pending requests – N"
//! with "Clear All"; 73-point rows with the person, when they wrote, the
//! servers in common (the message itself while hovered) and Ignore and
//! Accept DM; Wumpus when there are none.
//!
//! Discord opens a request beside the list; here a click opens it as a
//! conversation, where the composer offers the same two choices. Its Report
//! Spam opens Discord's in-app report flow, which CordFast does not have yet.

use egui::{Align2, Color32, FontId, Frame, Margin, Rect, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::discord::model::{Channel, Id};
use crate::icons::{self, Icon};
use crate::theme;

/// Discord's row (`LIST_ROW_HEIGHT`).
const ROW: f32 = 73.0;
/// The empty state's Wumpus, dark and light (the bundle's modules 317017 and 826223).
const WUMPUS_DARK: &str = "https://discord.com/assets/4e11cf8e80a816c6.svg";
const WUMPUS_LIGHT: &str = "https://discord.com/assets/a09cfd7678b20f8c.svg";

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let spam = app.requests_spam;
    let requests = app.message_requests(false);
    let spams = app.message_requests(true);
    header(app, ui, spams.len());
    let line = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.border));
    let list = if spam { spams } else { requests };
    let ids: Vec<Id> = list.iter().map(|c| c.id).collect();
    app.ask_request_previews(&ids);
    if list.is_empty() {
        empty(app, ui, spam);
        return;
    }
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        Frame::new().inner_margin(Margin { left: 30, right: 20, top: 24, bottom: 24 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // "Pending requests – 3 • Clear All" (Spam – 3 on the other tab).
            let title = if spam { t!("Spam – {count}", count = list.len()) } else { t!("Pending requests – {count}", count = list.len()) };
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            let title = ui.painter().text(pos2(rect.left(), rect.top() + 8.0), Align2::LEFT_CENTER, title, theme::strong(14.0), p.muted);
            let dot = ui.painter().text(pos2(title.right() + 6.0, title.center().y), Align2::LEFT_CENTER, "•", theme::strong(14.0), p.text);
            let galley = ui.painter().layout_no_wrap(t!("Clear All").to_owned(), FontId::proportional(14.0), p.link);
            let clear = Rect::from_min_size(pos2(dot.right() + 6.0, title.center().y - galley.size().y / 2.0), galley.size());
            let response = ui.interact(clear, ui.id().with("requests-clear"), Sense::click());
            ui.painter().galley(clear.min, galley, p.link);
            if response.hovered() {
                ui.painter().hline(clear.x_range(), clear.bottom(), Stroke::new(1.0, p.link));
            }
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::IgnoreRequests(ids.clone()));
            }
            for channel in &list {
                row(app, ui, channel, spam);
            }
        });
    });
}

/// The envelope, "Message Requests", a divider and the two pills.
fn header(app: &mut App, ui: &mut egui::Ui, spam_count: usize) {
    let p = theme::p();
    Frame::new().inner_margin(Margin::symmetric(16, 0)).show(ui, |ui| {
        ui.set_height(48.0);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            icons::paint(ui, icon, Icon::Envelope, p.muted);
            ui.label(egui::RichText::new(t!("Message Requests")).font(theme::strong(16.0)).color(p.strong));
            let (divider, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
            ui.painter().vline(divider.center().x, divider.y_range(), Stroke::new(1.0, p.border));
            if super::friends::tab_button(ui, t!("Requests"), !app.requests_spam, 0).clicked() {
                app.requests_spam = false;
            }
            let spam_label = if spam_count == 0 { t!("Spam").to_owned() } else { t!("Spam ({count})", count = spam_count) };
            if super::friends::tab_button(ui, &spam_label, app.requests_spam, 0).clicked() {
                app.requests_spam = true;
            }
        });
    });
}

/// One request: the person, when they wrote, servers in common or the
/// message, and its two buttons.
fn row(app: &mut App, ui: &mut egui::Ui, channel: &Channel, spam: bool) {
    let p = theme::p();
    let Some(user) = channel.recipients.first().cloned() else { return };
    if !app.profiles.contains_key(&user.id) && app.requested_profiles.insert(user.id) {
        app.commands.send(crate::backend::Command::Profile { user: user.id, guild: None });
    }
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let hovered = response.hovered() || ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect.expand2(vec2(8.0, 0.0)), 8.0, p.hover);
    } else {
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.selected));
    }
    // The buttons first take their place at the right.
    let accept = t!("Accept DM").to_owned();
    let (first, second) = if spam { (accept.clone(), None) } else { (t!("Ignore").to_owned(), Some(accept)) };
    let font = theme::strong(14.0);
    let width = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x + 24.0;
    let second_rect = second.as_ref().map(|text| Rect::from_min_size(pos2(rect.right() - width(text), rect.center().y - 16.0), vec2(width(text), 32.0)));
    let first_right = second_rect.map_or(rect.right(), |r| r.left() - 8.0);
    let first_rect = Rect::from_min_size(pos2(first_right - width(&first), rect.center().y - 16.0), vec2(width(&first), 32.0));

    let avatar = Rect::from_min_size(pos2(rect.left(), rect.center().y - 20.0), Vec2::splat(40.0));
    app.images.circle(ui, avatar, Some(&user.avatar_url()), &super::initials(user.display_name()), 20.0);
    let x = avatar.right() + 12.0;
    let text_right = first_rect.left() - 16.0;
    // The name, the username beside it (Discord's account identifier), and
    // when they last wrote at the line's end.
    let when = app.latest.get(&channel.id).copied().or(channel.last_message_id).map(|id| crate::text::time(crate::discord::model::snowflake_ms(id))).unwrap_or_default();
    let when_galley = ui.painter().layout_no_wrap(when, FontId::proportional(12.0), p.muted);
    let when_x = text_right - when_galley.size().x;
    let top = rect.center().y - 10.0;
    let name = ui.painter().text(pos2(x, top), Align2::LEFT_CENTER, user.display_name(), theme::strong(16.0), p.strong);
    if user.display_name() != user.username && name.right() + 6.0 < when_x - 8.0 {
        super::paint_truncated(ui, Rect::from_min_max(pos2(name.right() + 6.0, top - 9.0), pos2(when_x - 8.0, top + 9.0)), &user.username, FontId::proportional(14.0), p.muted);
    }
    ui.painter().galley(pos2(when_x, top - when_galley.size().y / 2.0), when_galley, p.muted);
    // The second line: servers in common, or the message while hovered.
    let line = Rect::from_min_max(pos2(x, rect.center().y + 2.0), pos2(text_right, rect.center().y + 20.0));
    if hovered {
        let text = match app.request_previews.get(&channel.id) {
            Some(Some(message)) if !message.content.is_empty() => message.content.clone(),
            Some(Some(_)) => t!("Click to preview").to_owned(),
            Some(None) if channel.last_message_id.is_none() => t!("The last message was deleted.").to_owned(),
            Some(None) => t!("Error loading message preview.").to_owned(),
            None => String::new(),
        };
        super::paint_truncated(ui, line, &text.replace('\n', " "), FontId::proportional(14.0), p.muted);
    } else {
        mutual(app, ui, line, user.id);
    }
    // The buttons: Ignore (secondary) and Accept DM (primary); on Spam,
    // Accept DM alone as a secondary one.
    let pressed = |ui: &mut egui::Ui, rect: Rect, text: &str, primary: bool, id: &str| {
        let response = ui.interact(rect, ui.id().with((id, channel.id)), Sense::click());
        let fill = match (primary, response.hovered()) {
            (true, false) => p.accent,
            (true, true) => p.accent.gamma_multiply(0.85),
            (false, false) => p.selected,
            (false, true) => p.hover.gamma_multiply(1.6),
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::strong(14.0), if primary { p.on_accent } else { p.strong });
        response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    };
    if pressed(ui, first_rect, &first, false, "request-first") {
        app.actions.push(if spam { Action::AcceptRequest(channel.id) } else { Action::IgnoreRequests(vec![channel.id]) });
    } else if let (Some(rect), Some(text)) = (second_rect, &second)
        && pressed(ui, rect, text, true, "request-accept")
    {
        app.actions.push(Action::AcceptRequest(channel.id));
    } else if response.clicked() {
        app.actions.push(Action::SelectChannel(channel.id));
    }
}

/// Up to three server icons and "2 Mutual Servers", or "No Mutual Servers".
fn mutual(app: &mut App, ui: &egui::Ui, line: Rect, user: Id) {
    let p = theme::p();
    let guilds: Vec<(Option<String>, String)> = app
        .profiles
        .get(&user)
        .map(|profile| profile.mutual_guilds.iter().filter_map(|id| app.guild(*id)).map(|g| (g.icon_url(), g.name.clone())).collect())
        .unwrap_or_default();
    if guilds.is_empty() {
        if app.profiles.contains_key(&user) {
            ui.painter().text(line.left_center(), Align2::LEFT_CENTER, t!("No Mutual Servers"), FontId::proportional(14.0), p.muted);
        }
        return;
    }
    let mut x = line.left();
    for (icon, name) in guilds.iter().take(3) {
        let rect = Rect::from_min_size(pos2(x, line.center().y - 8.0), Vec2::splat(16.0));
        ui.painter().circle_filled(rect.center(), 9.0, p.chat);
        app.images.circle(ui, rect, icon.as_deref(), &super::initials(name), 8.0);
        x += 12.0;
    }
    let text = tn!(guilds.len() as u64, "{count} Mutual Server", "{count} Mutual Servers");
    ui.painter().text(pos2(x + 10.0, line.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), p.muted);
}

/// Wumpus and "There are no pending message requests".
fn empty(app: &mut App, ui: &mut egui::Ui, spam: bool) {
    let p = theme::p();
    let note = if spam { t!("There are no spam requests. Here’s Wumpus for now.") } else { t!("There are no pending message requests. Here’s Wumpus for now.") };
    let area = ui.available_rect_before_wrap();
    let image = Rect::from_center_size(area.center() - vec2(0.0, 40.0), vec2(415.0, 200.0));
    if let Some((texture, size)) = app.images.get(if p.dark { WUMPUS_DARK } else { WUMPUS_LIGHT }, 830) {
        let scale = (image.width() / size.x).min(image.height() / size.y);
        egui::Image::from_texture((texture, size * scale)).paint_at(ui, Rect::from_center_size(image.center(), size * scale));
    }
    ui.painter().text(pos2(area.center().x, image.bottom() + 40.0), Align2::CENTER_CENTER, note, FontId::proportional(16.0), p.muted);
}
