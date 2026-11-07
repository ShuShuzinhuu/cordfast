//! Home: Discord's friends page. The header with "Friends" and its tabs
//! (Online, All, Pending with its count, Add Friend) and the new
//! group button; a search box; the list, each friend with what they are
//! doing under the name (the game after a controller, the song, the custom
//! status) and the message and "⋮" buttons; and "Active Now" beside it, a
//! card for each friend playing or listening, with the game under them.
//! Laid out after the owner's screenshot of Discord.

use egui::{Color32, FontId, Frame, Key, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::discord::gateway::Activity;
use crate::discord::model::{Id, Relationship, User};
use crate::icons::{self, Icon};
use crate::theme;

/// The tabs' indices in `App::friends_tab`; "Add Friend" is 4 (3 was
/// "Blocked", whose people now close the "All" list).
const ONLINE: u8 = 0;
const ALL: u8 = 1;
const PENDING: u8 = 2;
const ADD: u8 = 4;

pub fn status_name(status: &str) -> &'static str {
    match status {
        "online" => t!("Online"),
        "idle" => t!("Idle"),
        "dnd" => t!("Do Not Disturb"),
        "invisible" => t!("Invisible"),
        _ => t!("Offline"),
    }
}

/// Discord's status badge over an avatar's lower right corner: a ring of
/// the background colour cut into the avatar, and the status shape in it.
pub fn dot(ui: &egui::Ui, avatar: egui::Rect, status: &str, ring: Color32) {
    let size = (avatar.width() * 0.3).max(10.0);
    let center = avatar.right_bottom() - vec2(size * 0.4, size * 0.4);
    ui.painter().circle_filled(center, size * 0.5 + size * 0.2, ring);
    icons::paint_status(ui, egui::Rect::from_center_size(center, Vec2::splat(size)), status);
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    if app.friends_tab == 3 {
        app.friends_tab = ALL;
    }
    header(app, ui);
    let line = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.border));

    // Discord's "Active Now": what friends are doing, beside the list.
    if ui.available_width() > 900.0 {
        let panel = egui::Panel::right("active-now")
            .exact_size(360.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.chat).inner_margin(Margin { left: 16, right: 16, top: 16, bottom: 0 }))
            .show(ui, |ui| active_now(app, ui));
        ui.painter().vline(panel.response.rect.left() + 0.5, panel.response.rect.y_range(), Stroke::new(1.0, p.border));
    }
    if app.friends_tab == ADD {
        add_friend(app, ui);
        return;
    }
    let filter_id = egui::Id::new("friends-filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    Frame::new().inner_margin(Margin { left: 24, right: 20, top: 16, bottom: 8 }).show(ui, |ui| {
        search(ui, &mut filter);
    });
    let needle = filter.trim().to_lowercase();
    ui.data_mut(|d| d.insert_temp(filter_id, filter));
    let matches = |r: &Relationship| needle.is_empty() || r.user.display_name().to_lowercase().contains(&needle) || r.user.username.to_lowercase().contains(&needle);
    let mut list: Vec<Relationship> = app
        .relationships
        .values()
        .filter(|r| match app.friends_tab {
            ONLINE => r.kind == 1 && app.presences.get(&r.user.id).is_some_and(|p| p.status != "offline"),
            PENDING => r.kind == 3 || r.kind == 4,
            _ => r.kind == 1,
        })
        .filter(|r| matches(r))
        .cloned()
        .collect();
    list.sort_by_key(|r| r.user.display_name().to_lowercase());
    let mut blocked: Vec<Relationship> = if app.friends_tab == ALL { app.relationships.values().filter(|r| r.kind == 2).filter(|r| matches(r)).cloned().collect() } else { Vec::new() };
    blocked.sort_by_key(|r| r.user.display_name().to_lowercase());

    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        Frame::new().inner_margin(Margin { left: 20, right: 20, top: 4, bottom: 12 }).show(ui, |ui| {
            let title = match app.friends_tab {
                ONLINE => t!("Online — {count}", count = list.len()),
                PENDING => t!("Pending — {count}", count = list.len()),
                _ => t!("All Friends — {count}", count = list.len()),
            };
            count_line(ui, &title);
            if list.is_empty() && blocked.is_empty() {
                ui.add_space(40.0);
                let text = if needle.is_empty() { t!("No one's around.") } else { t!("No one by that name.") };
                ui.vertical_centered(|ui| ui.label(RichText::new(text).color(p.muted)));
            }
            for relationship in list {
                friend_row(app, ui, &relationship);
            }
            if !blocked.is_empty() {
                ui.add_space(16.0);
                count_line(ui, &t!("Blocked — {count}", count = blocked.len()));
                for relationship in blocked {
                    friend_row(app, ui, &relationship);
                }
            }
        });
    });
}

/// "Friends • Online All Pending [Add Friend]", and the new group
/// button at the right end.
fn header(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    Frame::new().inner_margin(Margin::symmetric(16, 0)).show(ui, |ui| {
        ui.set_height(48.0);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            icons::paint(ui, icon, Icon::Friends, p.muted);
            ui.label(RichText::new(t!("Friends")).font(theme::strong(16.0)).color(p.strong));
            let (dot, _) = ui.allocate_exact_size(vec2(4.0, 20.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 2.0, p.faint);
            let pending = app.relationships.values().filter(|r| r.kind == 3).count();
            for (tab, label) in [(ONLINE, t!("Online")), (ALL, t!("All")), (PENDING, t!("Pending"))] {
                let count = if tab == PENDING { pending } else { 0 };
                if tab_button(ui, label, app.friends_tab == tab, count).clicked() {
                    app.friends_tab = tab;
                }
            }
            // "Add Friend": filled until chosen, then its text alone.
            let chosen = app.friends_tab == ADD;
            let galley = ui.painter().layout_no_wrap(t!("Add Friend").to_owned(), theme::strong(14.0), Color32::WHITE);
            let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 30.0), Sense::click());
            let text = if chosen {
                if p.dark { Color32::from_rgb(p.accent.r() / 2 + 128, p.accent.g() / 2 + 128, p.accent.b() / 2 + 128) } else { p.accent }
            } else {
                ui.painter().rect_filled(rect, 8.0, if response.hovered() { p.accent.gamma_multiply(0.85) } else { p.accent });
                p.on_accent
            };
            ui.painter().galley(pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, text);
            if response.clicked() {
                app.friends_tab = ADD;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                let color = if response.hovered() { p.strong } else { p.muted };
                new_group_icon(ui, rect.center(), color);
                if response.on_hover_text(t!("New Group DM")).clicked() {
                    app.modal = Some(crate::app::Modal::NewDm { picked: Vec::new(), filter: String::new() });
                }
            });
        });
    });
}

/// A header tab: a pill while chosen or hovered, with Pendentes' count in
/// Discord's red badge.
pub(super) fn tab_button(ui: &mut egui::Ui, label: &str, chosen: bool, count: usize) -> egui::Response {
    let p = theme::p();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(15.0), p.text);
    let badge = (count > 0).then(|| ui.painter().layout_no_wrap(count.to_string(), theme::strong(12.0), Color32::WHITE));
    let badge_width = badge.as_ref().map_or(0.0, |b| b.size().x.max(8.0) + 16.0);
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 20.0 + badge_width, 30.0), Sense::click());
    if chosen || response.hovered() {
        ui.painter().rect_filled(rect, 8.0, if chosen { p.selected } else { p.hover });
    }
    let color = if chosen || response.hovered() { p.strong } else { p.muted };
    ui.painter().galley(pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley.clone(), color);
    if let Some(badge) = badge {
        let pill = Rect::from_center_size(pos2(rect.left() + 10.0 + galley.size().x + 6.0 + badge_width / 2.0 - 3.0, rect.center().y), vec2(badge.size().x.max(8.0) + 8.0, 16.0));
        ui.painter().rect_filled(pill, 8.0, p.red);
        ui.painter().galley(pill.center() - badge.size() / 2.0, badge, Color32::WHITE);
    }
    response
}

/// Discord's new group button: a chat bubble with a plus at its corner.
fn new_group_icon(ui: &egui::Ui, center: Pos2, color: Color32) {
    let p = theme::p();
    icons::paint(ui, Rect::from_center_size(center + vec2(-1.0, 1.0), Vec2::splat(20.0)), Icon::Chat, color);
    let corner = center + vec2(7.0, -7.0);
    ui.painter().circle_filled(corner, 6.5, p.chat);
    icons::paint(ui, Rect::from_center_size(corner, Vec2::splat(10.0)), Icon::Plus, color);
}

/// The search box over the list: the magnifier, then what is typed.
fn search(ui: &mut egui::Ui, filter: &mut String) {
    let p = theme::p();
    Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin { left: 10, right: 10, top: 8, bottom: 8 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            icons::paint(ui, icon, Icon::Search, p.muted);
            ui.add(egui::TextEdit::singleline(filter).frame(Frame::NONE).hint_text(RichText::new(t!("Search")).color(p.faint)).desired_width(f32::INFINITY));
        });
    });
}

/// "Online — 3" over the rows, as Discord now writes it.
pub(super) fn count_line(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).font(theme::strong(14.0)).color(theme::p().muted));
    ui.add_space(8.0);
}

fn add_friend(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
        ui.label(RichText::new(t!("Add Friend")).font(theme::strong(20.0)).color(p.strong));
        ui.label(RichText::new(t!("You can add friends with their Discord username.")).color(p.muted));
        ui.add_space(14.0);
        Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut app.friend_input)
                        .hint_text(t!("You can add friends with their Discord username."))
                        .frame(Frame::NONE)
                        .desired_width(ui.available_width() - 220.0),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let ready = !app.friend_input.trim().is_empty();
                let send = egui::Button::new(RichText::new(t!("Send Friend Request")).color(Color32::WHITE)).fill(p.accent).min_size(vec2(0.0, 32.0));
                if (ui.add_enabled(ready, send).clicked() || enter) && ready {
                    app.actions.push(Action::AddFriend);
                }
            });
        });
    });
}

/// Discord's activity status (the bundle's `ActivityStatus`), the line
/// under a name in the friend, DM and member lists and the account panel:
/// the first activity with its green icon (a controller, music, a screen)
/// or "In voice" with the speaker, "+1" for each more, then "•" and the
/// custom status with its emoji. Small lists (`small`, 12 points) keep only
/// the activity's icon when a custom status has words. Paints nothing and
/// returns false when there is nothing to say.
pub fn status_line(app: &App, ui: &egui::Ui, rect: Rect, user: Id, small: bool) -> bool {
    let p = theme::p();
    let presence = app.presences.get(&user);
    let custom = presence.and_then(|presence| presence.custom.clone()).filter(|c| !c.trim().is_empty());
    // Activities other than the custom status (4) and a hang status (6),
    // one per application or name.
    let mut activities: Vec<&Activity> = Vec::new();
    for activity in presence.map(|p| p.activities.as_slice()).unwrap_or_default() {
        if activity.kind == 4 || activity.kind == 6 {
            continue;
        }
        if !activities.iter().any(|a| (a.application.is_some() && a.application == activity.application) || a.name == activity.name) {
            activities.push(activity);
        }
    }
    let voice = app.user_voice_channel(user).map(|channel| match channel.kind {
        crate::discord::model::ChannelKind::Dm | crate::discord::model::ChannelKind::GroupDm => t!("In a call"),
        crate::discord::model::ChannelKind::Stage => t!("In a stage"),
        _ => t!("In voice"),
    });
    let count = activities.len() + usize::from(voice.is_some());
    if count == 0 && custom.is_none() {
        return false;
    }
    let size = if small { 12.0 } else { 14.0 };
    let font = egui::FontId::proportional(size);
    let green = p.green;
    // A custom status with words hides the activity's own words in small lists.
    let custom_words = custom.as_ref().is_some_and(|c| c.chars().any(char::is_alphanumeric));
    let icon_only = small && custom_words;
    let mut x = rect.left();
    let y = rect.center().y;
    let text = |ui: &egui::Ui, x: &mut f32, words: &str, color: Color32| {
        let room = (rect.right() - *x).max(0.0);
        let mut job = crate::text::script_job(words, font.clone(), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let width = galley.size().x;
        ui.painter().galley(pos2(*x, y - galley.size().y / 2.0), galley, color);
        *x += width;
    };
    if count > 0 {
        let (icon, words) = match activities.first() {
            Some(activity) => {
                let icon = match activity.kind {
                    0 | 5 => Some(Icon::Controller),
                    2 => Some(Icon::Music),
                    1 | 3 => Some(Icon::Tv),
                    _ => None,
                };
                (icon, activity.short())
            }
            None => (Some(Icon::Voice), voice.clone().unwrap_or_default().to_owned()),
        };
        if let Some(icon) = icon {
            icons::paint(ui, Rect::from_center_size(pos2(x + size / 2.0, y), Vec2::splat(size)), icon, green);
            x += size + 4.0;
        }
        if !icon_only || icon.is_none() {
            text(ui, &mut x, &words, p.muted);
        }
        if count > 1 {
            x += 4.0;
            text(ui, &mut x, &format!("+{}", count - 1), if custom_words { green } else { p.muted });
        }
    }
    if let Some(custom) = custom {
        if count > 0 {
            text(ui, &mut x, " • ", p.muted);
        }
        text(ui, &mut x, &custom, p.muted);
    }
    true
}

/// Whether `status_line` has something to say for someone.
pub fn has_status_line(app: &App, user: Id) -> bool {
    let presence = app.presences.get(&user);
    presence.is_some_and(|p| p.custom.as_ref().is_some_and(|c| !c.trim().is_empty()) || p.activities.iter().any(|a| a.kind != 4 && a.kind != 6)) || app.user_voice_channel(user).is_some()
}

/// Whether someone's line says they are in voice: Discord's list names a
/// game or another activity first.
pub fn shows_voice(app: &App, user: Id) -> bool {
    app.presences.get(&user).is_none_or(|presence| presence.activities.is_empty()) && app.user_voice_channel(user).is_some()
}

fn friend_row(app: &mut App, ui: &mut egui::Ui, r: &Relationship) {
    let p = theme::p();
    let user = &r.user;
    let status = app.presences.get(&user.id).map_or("offline", |p| p.status.as_str()).to_owned();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
    let hovered = response.hovered() || response.context_menu_opened();
    if hovered {
        ui.painter().rect_filled(rect, 8.0, p.hover);
    } else {
        ui.painter().hline(rect.x_range().shrink(10.0), rect.top(), Stroke::new(1.0, p.selected));
    }
    let avatar = egui::Rect::from_min_size(rect.left_center() + vec2(10.0, -16.0), Vec2::splat(32.0));
    app.images.circle(ui, avatar, Some(&user.avatar_url()), &super::initials(user.display_name()), 16.0);
    if r.kind == 1 {
        dot(ui, avatar, &status, if hovered { p.hover } else { p.chat });
    }
    let x = avatar.right() + 12.0;
    // The name with the server tag it wears; the username beside them while
    // hovered, as Discord's.
    let name_room = Rect::from_min_max(pos2(x, rect.center().y - 19.0), pos2(rect.right() - 110.0, rect.center().y + 1.0));
    super::name_with_tag(app, ui, name_room, user, user.display_name(), theme::strong(16.0), p.strong);
    if hovered {
        let name = ui.painter().layout_no_wrap(user.display_name().to_owned(), theme::strong(16.0), p.strong).size().x;
        let tag = user.tag.as_ref().map_or(0.0, |(_, tag, _)| super::profile::tag_size(ui, user, tag, 16.0).x + 6.0);
        let at = x + name + tag + 6.0;
        if at < name_room.right() {
            super::paint_truncated(ui, Rect::from_min_max(pos2(at, name_room.top()), name_room.max), &user.username, FontId::proportional(14.0), p.muted);
        }
    }
    // A friend's line is Discord's activity status, or else their status.
    let line_rect = Rect::from_min_max(pos2(x, rect.center().y + 2.0), pos2(rect.right() - 100.0, rect.center().y + 20.0));
    if !(r.kind == 1 && status_line(app, ui, line_rect, user.id, false)) {
        let line = match r.kind {
            3 => t!("Incoming Friend Request").to_owned(),
            4 => t!("Outgoing Friend Request").to_owned(),
            2 => t!("Blocked").to_owned(),
            _ => status_name(&status).to_owned(),
        };
        super::paint_truncated(ui, line_rect, &line, FontId::proportional(14.0), p.muted);
    }

    // Round buttons on the right, as in Discord.
    let round = |ui: &mut egui::Ui, at: Pos2, id: &str| -> egui::Response {
        let rect = Rect::from_center_size(at, Vec2::splat(36.0));
        let response = ui.interact(rect, ui.id().with((id, user.id)), Sense::click());
        ui.painter().circle_filled(rect.center(), 18.0, if response.hovered() { p.rail } else { p.sidebar });
        response
    };
    let first = pos2(rect.right() - 28.0, rect.center().y);
    let second = first - vec2(46.0, 0.0);
    match r.kind {
        1 => {
            let more = round(ui, first, "more");
            vertical_dots(ui, more.rect.center(), if more.hovered() { p.strong } else { p.muted });
            egui::Popup::menu(&more.on_hover_text(t!("More"))).show(|ui| {
                super::menu::begin(ui);
                if super::menu::item(ui, t!("Start Voice Call"), super::menu::Tail::None).clicked() {
                    app.actions.push(Action::CallUser(user.id));
                    ui.close();
                }
                if super::menu::danger(ui, t!("Remove Friend"), super::menu::Tail::None).clicked() {
                    app.modal = Some(super::menu::remove_friend(user));
                    ui.close();
                }
            });
            let message = round(ui, second, "message");
            icons::paint(ui, Rect::from_center_size(message.rect.center(), Vec2::splat(20.0)), Icon::Chat, if message.hovered() { p.strong } else { p.muted });
            if message.on_hover_text(t!("Message")).clicked() {
                app.actions.push(Action::MessageUser(user.id));
            }
        }
        3 => {
            let decline = round(ui, first, "decline");
            icons::paint(ui, Rect::from_center_size(decline.rect.center(), Vec2::splat(20.0)), Icon::Close, if decline.hovered() { p.red } else { p.muted });
            if decline.on_hover_text(t!("Ignore")).clicked() {
                app.actions.push(Action::Relationship { user: user.id, kind: None });
            }
            let accept = round(ui, second, "accept");
            icons::paint(ui, Rect::from_center_size(accept.rect.center(), Vec2::splat(20.0)), Icon::Check, if accept.hovered() { p.green } else { p.muted });
            if accept.on_hover_text(t!("Accept")).clicked() {
                app.actions.push(Action::Relationship { user: user.id, kind: Some(1) });
            }
        }
        _ => {
            let cancel = round(ui, first, "cancel");
            icons::paint(ui, Rect::from_center_size(cancel.rect.center(), Vec2::splat(20.0)), Icon::Close, if cancel.hovered() { p.red } else { p.muted });
            let tip = if r.kind == 2 { t!("Unblock") } else { t!("Cancel") };
            if cancel.on_hover_text(tip).clicked() {
                app.actions.push(Action::Relationship { user: user.id, kind: None });
            }
        }
    }
    if response.clicked() && r.kind == 1 {
        app.actions.push(Action::MessageUser(user.id));
    }
    let user = user.clone();
    response.context_menu(|ui| {
        super::menu::begin(ui);
        super::menu::user(app, ui, &user, None);
    });
}

/// Discord's "⋮": three dots one over another.
fn vertical_dots(ui: &egui::Ui, center: Pos2, color: Color32) {
    for dy in [-6.0, 0.0, 6.0] {
        ui.painter().circle_filled(center + vec2(0.0, dy), 2.0, color);
    }
}

/// How long ago something started, as the cards write it: "just now",
/// "15m", "2h", "3d".
fn ago(start: Option<u64>) -> Option<String> {
    let elapsed = crate::app::now_ms().saturating_sub(start?) / 1000;
    Some(match elapsed {
        0..60 => t!("just now").into(),
        60..3600 => t!("{n}m", n = elapsed / 60),
        3600..86_400 => t!("{n}h", n = elapsed / 3600),
        _ => t!("{n}d", n = elapsed / 86_400),
    })
}

fn active_now(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    ui.label(RichText::new(t!("Active Now")).font(theme::strong(20.0)).color(p.strong));
    ui.add_space(14.0);
    let mut active: Vec<(User, Activity)> = app
        .relationships
        .values()
        .filter(|r| r.kind == 1)
        .filter_map(|r| {
            let presence = app.presences.get(&r.user.id)?;
            let activity = presence.activities.iter().find(|a| matches!(a.kind, 0 | 1 | 2 | 3 | 5))?.clone();
            Some((r.user.clone(), activity))
        })
        .collect();
    // Who started most recently first, as Discord orders them.
    active.sort_by_key(|(_, a)| std::cmp::Reverse(a.start.unwrap_or(0)));
    if active.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("It's quiet for now...")).font(theme::strong(15.0)).color(p.strong));
            ui.label(RichText::new(t!("When a friend starts an activity—like playing a game or hanging out on voice—we'll show it here!")).size(13.0).color(p.muted));
        });
        return;
    }
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (user, activity) in active {
            active_card(app, ui, &user, &activity);
            ui.add_space(12.0);
        }
        ui.add_space(4.0);
    });
}

/// One "Active Now" card: the friend with what they do and since when, and
/// under them the game (or song) with its picture and what it reports.
fn active_card(app: &mut App, ui: &mut egui::Ui, user: &User, activity: &Activity) {
    let p = theme::p();
    let icon = activity.application.and_then(|a| app.app_icon(a));
    let response = Frame::new().fill(p.sidebar).stroke(Stroke::new(1.0, p.border)).corner_radius(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // The friend.
        let (top, top_response) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::click());
        let avatar = Rect::from_min_size(top.left_top() + vec2(12.0, 14.0), Vec2::splat(32.0));
        app.images.circle(ui, avatar, Some(&user.avatar_url()), &super::initials(user.display_name()), 16.0);
        if let Some(status) = app.presences.get(&user.id).map(|pr| pr.status.clone()) {
            dot(ui, avatar, &status, p.sidebar);
        }
        let x = avatar.right() + 10.0;
        let right = top.right() - if icon.is_some() { 48.0 } else { 12.0 };
        super::paint_truncated(ui, Rect::from_min_max(pos2(x, top.top() + 11.0), pos2(right, top.top() + 30.0)), user.display_name(), theme::strong(15.0), p.strong);
        let what = match activity.kind {
            0 => activity.name.clone(),
            _ => activity.line(),
        };
        let line = match ago(activity.start) {
            Some(when) => format!("{what} – {when}"),
            None => what,
        };
        super::paint_truncated(ui, Rect::from_min_max(pos2(x, top.top() + 30.0), pos2(right, top.top() + 48.0)), &line, FontId::proportional(13.0), p.muted);
        if let Some(url) = &icon
            && let Some((texture, _)) = app.images.get(url, 64)
        {
            let at = Rect::from_center_size(pos2(top.right() - 28.0, top.center().y), Vec2::splat(28.0));
            egui::Image::from_texture((texture, at.size())).corner_radius(6.0).paint_at(ui, at);
        }
        if top_response.clicked() {
            app.actions.push(Action::OpenProfile { user: user.clone(), guild: None, at: top.left_top() - vec2(312.0, 0.0) });
        }
        // The game, under a line.
        ui.painter().hline(top.x_range(), top.bottom(), Stroke::new(1.0, p.border));
        Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (art, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                let picture = activity.large_image.clone().or_else(|| icon.clone());
                match picture.as_deref().and_then(|url| app.images.get(url, 96)) {
                    Some((texture, _)) => {
                        egui::Image::from_texture((texture, art.size())).corner_radius(8.0).paint_at(ui, art);
                    }
                    None => {
                        ui.painter().rect_filled(art, 8.0, p.selected);
                        let glyph = if activity.kind == 2 { Icon::Headphones } else { Icon::Controller };
                        icons::paint(ui, art.shrink(10.0), glyph, p.muted);
                    }
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let title = if activity.kind == 2 { activity.details.clone().unwrap_or_else(|| activity.name.clone()) } else { activity.name.clone() };
                    ui.add(egui::Label::new(RichText::new(title).font(theme::strong(14.0)).color(p.strong)).truncate());
                    let detail = match activity.kind {
                        2 => activity.state.clone().map(|s| t!("by {artist}", artist = s)),
                        _ => activity.details.clone().or_else(|| activity.state.clone()),
                    };
                    if let Some(detail) = detail {
                        ui.add(egui::Label::new(RichText::new(detail).size(13.0).color(p.muted)).truncate());
                    }
                });
            });
            // A song: its progress, as Discord's card shows it.
            if let (Some(start), Some(end)) = (activity.start, activity.end)
                && end > start
            {
                ui.add_space(8.0);
                let now = crate::app::now_ms();
                let done = now.saturating_sub(start).min(end - start) as f32 / (end - start) as f32;
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                ui.painter().rect_filled(bar, 2.0, p.selected);
                ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * done, bar.height())), 2.0, p.strong);
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        });
    });
    let _ = response;
}
