//! Discord's context menus, for every right click and "…" button: rows 32
//! points high with the label at the left and, at the right, an icon, a
//! checkbox, a radio or the chevron of a submenu. The hovered row takes the
//! accent colour with white text; a dangerous one is red, and fills red.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::discord::model::{Channel, Id, User};
use crate::icons::{self, Icon};
use crate::theme;

/// What sits at the right of a row.
#[derive(Clone, Copy)]
pub enum Tail {
    None,
    Icon(Icon),
    /// Discord's "ID" badge, beside the copy-ID items.
    Id,
    Check(bool),
    Radio(bool),
    Submenu,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Normal,
    /// Leaving, deleting, blocking: red, and red when hovered.
    Danger,
    /// Inviting: in the accent's lighter shade.
    Brand,
}

/// A row; clicked() is the choice. Width is the menu's.
pub fn item(ui: &mut egui::Ui, label: &str, tail: Tail) -> egui::Response {
    row(ui, label, "", tail, Kind::Normal)
}

/// A row with a second, smaller line (a duration's end, the device in use).
pub fn item_with(ui: &mut egui::Ui, label: &str, detail: &str, tail: Tail) -> egui::Response {
    row(ui, label, detail, tail, Kind::Normal)
}

/// A red row: leaving, deleting, blocking.
pub fn danger(ui: &mut egui::Ui, label: &str, tail: Tail) -> egui::Response {
    row(ui, label, "", tail, Kind::Danger)
}

/// "Invite People", in the accent's colour as Discord writes it.
pub fn brand(ui: &mut egui::Ui, label: &str, tail: Tail) -> egui::Response {
    row(ui, label, "", tail, Kind::Brand)
}

fn row(ui: &mut egui::Ui, label: &str, detail: &str, tail: Tail, kind: Kind) -> egui::Response {
    let p = theme::p();
    let height = if detail.is_empty() { 32.0 } else { 44.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 4.0, if kind == Kind::Danger { p.red } else { p.accent });
    }
    let lighter = |c: Color32| Color32::from_rgb(c.r() / 2 + 128, c.g() / 2 + 128, c.b() / 2 + 128);
    let color = match (hovered, kind) {
        (true, _) => Color32::WHITE,
        (_, Kind::Danger) => p.red,
        (_, Kind::Brand) => if p.dark { lighter(p.accent) } else { p.accent },
        _ => p.text,
    };
    let right = rect.right() - if matches!(tail, Tail::None) { 8.0 } else { 34.0 };
    let text = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(right, rect.bottom()));
    if detail.is_empty() {
        super::paint_truncated(ui, text, label, FontId::proportional(14.0), color);
    } else {
        let (top, bottom) = (Rect::from_min_max(text.min, pos2(text.right(), text.center().y + 2.0)), Rect::from_min_max(pos2(text.left(), text.center().y + 2.0), text.max));
        super::paint_truncated(ui, top.translate(vec2(0.0, 2.0)), label, FontId::proportional(14.0), color);
        super::paint_truncated(ui, bottom.translate(vec2(0.0, -4.0)), detail, FontId::proportional(12.0), if hovered { Color32::WHITE } else { p.muted });
    }
    let slot = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(18.0));
    match tail {
        Tail::None => {}
        Tail::Icon(icon) => icons::paint(ui, slot, icon, color),
        Tail::Id => {
            let badge = Rect::from_center_size(slot.center(), vec2(20.0, 14.0));
            ui.painter().rect_stroke(badge, 3.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
            ui.painter().text(badge.center(), Align2::CENTER_CENTER, "ID", theme::strong(9.0), color);
        }
        Tail::Check(on) => {
            if on {
                let fill = if hovered { Color32::WHITE } else { p.accent };
                ui.painter().rect_filled(slot, 4.0, fill);
                let tick = if hovered { p.accent } else { Color32::WHITE };
                let points = [slot.left_center() + vec2(4.0, 0.0), slot.center_bottom() + vec2(-2.0, -5.0), slot.right_top() + vec2(-4.0, 5.0)];
                ui.painter().add(egui::Shape::line(points.to_vec(), Stroke::new(2.0, tick)));
            } else {
                ui.painter().rect_stroke(slot, 4.0, Stroke::new(2.0, color), egui::StrokeKind::Inside);
            }
        }
        Tail::Radio(on) => {
            ui.painter().circle_stroke(slot.center(), 8.0, Stroke::new(2.0, color));
            if on {
                ui.painter().circle_filled(slot.center(), 4.0, color);
            }
        }
        Tail::Submenu => icons::paint_chevron_right(ui, slot.shrink(1.0), color),
    }
    response
}

/// The line between groups of rows.
pub fn separator(ui: &mut egui::Ui) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range().shrink(4.0), rect.center().y, Stroke::new(1.0, p.hover));
}

/// A row that opens a submenu on hover, as Discord's do.
pub fn submenu<R>(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let response = item(ui, label, Tail::Submenu);
    egui::containers::menu::SubMenu::new()
        .show(ui, &response, |ui| {
            ui.set_width(220.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            content(ui)
        })
        .map(|r| r.inner)
}

/// Prepares a menu's Ui: Discord's width and spacing between rows.
pub fn begin(ui: &mut egui::Ui) {
    ui.set_width(240.0);
    ui.spacing_mut().item_spacing.y = 2.0;
}

/// Discord's mute durations, in seconds (-1: until unmuted).
pub const MUTE_DURATIONS: [(&str, i64); 6] = [
    (n!("For 15 Minutes"), 900),
    (n!("For 1 Hour"), 3600),
    (n!("For 3 Hours"), 10_800),
    (n!("For 8 Hours"), 28_800),
    (n!("For 24 Hours"), 86_400),
    (n!("Until I turn it back on"), -1),
];

/// What a mute row is about.
#[derive(Clone, Copy)]
pub enum Muted {
    Server,
    Category,
    Channel,
    Conversation,
}

/// "Mute …" with the durations in a submenu, or, when muted, "Unmute …"
/// with when the mute ends, as Discord's rows.
pub fn mute(app: &mut App, ui: &mut egui::Ui, what: Muted, guild: Option<Id>, channel: Option<Id>) {
    let Some(id) = channel.or(guild) else { return };
    let muted = if channel.is_some() { app.mutes.channels.contains(&id) } else { app.mutes.guilds.contains(&id) };
    let (mute_label, unmute_label) = match what {
        Muted::Server => (t!("Mute Server"), t!("Unmute Server")),
        Muted::Category => (t!("Mute Category"), t!("Unmute Category")),
        Muted::Channel => (t!("Mute Channel"), t!("Unmute Channel")),
        Muted::Conversation => (t!("Mute Conversation"), t!("Unmute Conversation")),
    };
    if muted {
        let detail = app
            .mutes
            .ends
            .get(&id)
            .map(|end| t!("Muted until {time}", time = crate::text::until(*end)))
            .unwrap_or_default();
        if item_with(ui, unmute_label, &detail, Tail::None).clicked() {
            app.actions.push(Action::Mute { guild, channel, seconds: None });
            ui.close();
        }
    } else {
        submenu(ui, mute_label, |ui| {
            for (label, seconds) in MUTE_DURATIONS {
                if item(ui, crate::i18n::tr(label), Tail::None).clicked() {
                    app.actions.push(Action::Mute { guild, channel, seconds: Some(seconds) });
                    ui.close();
                }
            }
        });
    }
}

const LEVELS: [&str; 3] = [n!("All Messages"), n!("Only @mentions"), n!("Nothing")];

/// "Notification Settings": every message, @mentions or nothing,
/// and for a channel or category the choice of following its parent,
/// which shows what that parent says.
pub fn notifications(app: &mut App, ui: &mut egui::Ui, guild: Id, channel: Option<&Channel>) {
    let own = app.mutes.levels.get(&channel.map_or(guild, |c| c.id)).copied();
    let guild_level = app.mutes.levels.get(&guild).copied().unwrap_or_else(|| app.guild(guild).map_or(1, |g| g.default_notifications));
    let inherited = channel.map(|c| match c.parent_id {
        Some(parent) => (t!("Use Category Default"), app.mutes.levels.get(&parent).copied().unwrap_or(guild_level)),
        None => (t!("Use Server Default"), guild_level),
    });
    let channel_id = channel.map(|c| c.id);
    submenu(ui, t!("Notification Settings"), |ui| {
        ui.set_width(250.0);
        if let Some((label, level)) = inherited {
            if item_with(ui, label, crate::i18n::tr(LEVELS[level.min(2) as usize]), Tail::Radio(own.is_none())).clicked() {
                app.actions.push(Action::NotifyLevel { guild: Some(guild), channel: channel_id, level: 3 });
                ui.close();
            }
        }
        let current = own.or_else(|| channel.is_none().then_some(guild_level));
        for (level, label) in LEVELS.iter().enumerate() {
            if item(ui, crate::i18n::tr(label), Tail::Radio(current == Some(level as u8))).clicked() {
                app.actions.push(Action::NotifyLevel { guild: Some(guild), channel: channel_id, level: level as u8 });
                ui.close();
            }
        }
    });
}

/// Settings → Developer → Developer Mode, which shows the "Copy … ID"
/// items; set every frame from the account's settings.
pub static DEVELOPER_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// "Copy … ID", with Discord's ID badge, in Developer Mode.
pub fn copy_id(ui: &mut egui::Ui, label: &str, id: Id) {
    if !DEVELOPER_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if item(ui, label, Tail::Id).clicked() {
        ui.ctx().copy_text(id.to_string());
        ui.close();
    }
}

const KICK_MEMBERS: u64 = 1 << 1;
const BAN_MEMBERS: u64 = 1 << 2;
const CHANGE_NICKNAME: u64 = 1 << 26;
const MANAGE_NICKNAMES: u64 = 1 << 27;
const MANAGE_ROLES: u64 = 1 << 28;
const MODERATE_MEMBERS: u64 = 1 << 40;

/// Discord's timeouts, in seconds.
pub const TIMEOUTS: [(&str, i64); 6] = [
    (n!("60 Seconds"), 60),
    (n!("5 Minutes"), 300),
    (n!("10 Minutes"), 600),
    (n!("1 Hour"), 3600),
    (n!("1 Day"), 86_400),
    (n!("1 Week"), 604_800),
];

/// Right click on someone (member list, chat, DM, voice, friends), as
/// Discord's user menu: profile, mention, message, call, note, friendship
/// and blocking, then in a server the nickname, roles and moderation.
pub fn user(app: &mut App, ui: &mut egui::Ui, user: &crate::discord::model::User, guild: Option<Id>) {
    let me = app.me_id();
    let mine = user.id == me;
    let (name, _) = app.display(guild, user);
    if item(ui, t!("Profile"), Tail::None).clicked() {
        let at = ui.ctx().pointer_latest_pos().unwrap_or_default();
        app.actions.push(Action::OpenProfile { user: user.clone(), guild, at });
        ui.close();
    }
    if app.selected_channel.is_some() && item(ui, t!("Mention"), Tail::None).clicked() {
        let shown = format!("@{name}");
        if !app.composer.is_empty() && !app.composer.ends_with(' ') {
            app.composer.push(' ');
        }
        app.composer.push_str(&shown);
        app.composer.push(' ');
        app.mention_tokens.push((shown, format!("<@{}>", user.id)));
        app.focus_composer = true;
        ui.close();
    }
    if !mine && !user.bot {
        if item(ui, t!("Message"), Tail::None).clicked() {
            app.actions.push(Action::MessageUser(user.id));
            ui.close();
        }
        if item(ui, t!("Call"), Tail::None).clicked() {
            app.actions.push(Action::CallUser(user.id));
            ui.close();
        }
    }
    if !mine && item(ui, t!("Add Note"), Tail::None).clicked() {
        app.modal = Some(crate::app::Modal::Note { user: user.id, name: user.username.clone(), note: String::new() });
        ui.close();
    }
    if !mine && !user.bot {
        separator(ui);
        let kind = app.relationships.get(&user.id).map(|r| r.kind);
        match kind {
            Some(1) => {
                if item(ui, t!("Remove Friend"), Tail::None).clicked() {
                    app.modal = Some(remove_friend(user));
                    ui.close();
                }
            }
            Some(3) => {
                if item(ui, t!("Accept Friend Request"), Tail::None).clicked() {
                    app.actions.push(Action::Relationship { user: user.id, kind: Some(1) });
                    ui.close();
                }
            }
            Some(4) => {
                if item(ui, t!("Cancel Friend Request"), Tail::None).clicked() {
                    app.actions.push(Action::Relationship { user: user.id, kind: None });
                    ui.close();
                }
            }
            Some(2) => {}
            _ => {
                if item(ui, t!("Add Friend"), Tail::None).clicked() {
                    app.actions.push(Action::Relationship { user: user.id, kind: Some(1) });
                    ui.close();
                }
            }
        }
        if kind == Some(2) {
            if item(ui, t!("Unblock"), Tail::None).clicked() {
                app.actions.push(Action::Relationship { user: user.id, kind: None });
                ui.close();
            }
        } else if danger(ui, t!("Block"), Tail::None).clicked() {
            app.actions.push(Action::Relationship { user: user.id, kind: Some(2) });
            ui.close();
        }
    }
    if let Some(guild_id) = guild
        && let Some(guild) = app.guild(guild_id).cloned()
    {
        let perms = guild.my_permissions(me);
        let owner = guild.owner_id == Some(user.id);
        let roles: Vec<crate::discord::model::Role> = {
            let mut list: Vec<_> = guild.roles.iter().filter(|r| r.id != guild_id).cloned().collect();
            list.sort_by_key(|r| std::cmp::Reverse(r.position));
            list
        };
        let member_roles = app.members.get(&(guild_id, user.id)).map(|m| m.roles.clone()).unwrap_or_default();
        let nick = app.members.get(&(guild_id, user.id)).and_then(|m| m.nick.clone()).unwrap_or_default();
        let can_nick = if mine { perms & (CHANGE_NICKNAME | MANAGE_NICKNAMES) != 0 } else { perms & MANAGE_NICKNAMES != 0 && !owner };
        let can_roles = perms & MANAGE_ROLES != 0 && !roles.is_empty();
        if can_nick || can_roles {
            separator(ui);
        }
        if can_nick && item(ui, t!("Change Nickname"), Tail::None).clicked() {
            app.modal = Some(crate::app::Modal::Nickname { guild: guild_id, user: user.id, nick });
            ui.close();
        }
        if can_roles {
            submenu(ui, t!("Roles"), |ui| {
                ui.set_width(240.0);
                for role in &roles {
                    let has = member_roles.contains(&role.id);
                    if item(ui, &role.name, Tail::Check(has)).clicked() {
                        app.commands.send(crate::backend::Command::Api {
                            key: "role".into(),
                            method: if has { "DELETE" } else { "PUT" },
                            path: format!("/guilds/{guild_id}/members/{}/roles/{}", user.id, role.id),
                            body: None,
                        });
                        if let Some(member) = app.members.get_mut(&(guild_id, user.id)) {
                            if has {
                                member.roles.retain(|r| *r != role.id);
                            } else {
                                member.roles.push(role.id);
                            }
                        }
                    }
                }
            });
        }
        if !mine && !owner && perms & (MODERATE_MEMBERS | KICK_MEMBERS | BAN_MEMBERS) != 0 {
            separator(ui);
            if perms & MODERATE_MEMBERS != 0 {
                submenu(ui, &t!("Timeout {name}", name = name), |ui| {
                    for (label, seconds) in TIMEOUTS {
                        if item(ui, crate::i18n::tr(label), Tail::None).clicked() {
                            let until = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(seconds);
                            app.commands.send(crate::backend::Command::Api {
                                key: "timeout".into(),
                                method: "PATCH",
                                path: format!("/guilds/{guild_id}/members/{}", user.id),
                                body: Some(serde_json::json!({ "communication_disabled_until": until.to_string() })),
                            });
                            ui.close();
                        }
                    }
                });
            }
            if perms & KICK_MEMBERS != 0 && danger(ui, &t!("Kick {name}", name = name), Tail::None).clicked() {
                app.modal = Some(crate::app::Modal::Confirm {
                    title: t!("Kick '{name}' from Server", name = name),
                    body: t!("Are you sure you want to kick @{name} from the server? They will be able to rejoin with another invite.", name = name),
                    button: t!("Kick").into(),
                    then: crate::app::Confirmed::Kick { guild: guild_id, user: user.id },
                });
                ui.close();
            }
            if perms & BAN_MEMBERS != 0 && danger(ui, &t!("Ban {name}", name = name), Tail::None).clicked() {
                app.modal = Some(crate::app::Modal::Confirm {
                    title: t!("Ban '{name}'", name = name),
                    body: t!("Are you sure you want to ban @{name}? They won't be able to return to the server until they are unbanned.", name = name),
                    button: t!("Ban").into(),
                    then: crate::app::Confirmed::Ban { guild: guild_id, user: user.id },
                });
                ui.close();
            }
        }
    }
    separator(ui);
    copy_id(ui, t!("Copy User ID"), user.id);
}

/// Discord's question before removing a friend.
pub fn remove_friend(user: &User) -> crate::app::Modal {
    let name = user.display_name().to_owned();
    crate::app::Modal::Confirm {
        title: t!("Remove '{name}'", name = name),
        body: t!("Are you sure you want to remove {name} from your friends?", name = name),
        button: t!("Remove Friend").into(),
        then: crate::app::Confirmed::RemoveFriend { user: user.id },
    }
}

/// A row that opens its menu beside it, as the cards of Discord's profile
/// do: on hover or click, kept open until a click elsewhere or another row
/// of the same card is hovered. egui's own submenus close when the pointer
/// crosses the card's padding on its way to them.
pub fn flyout(ui: &egui::Ui, row: &egui::Response, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    use egui::SetOpenCommand;
    let id = egui::Popup::default_response_id(row);
    let pointer = ui.ctx().pointer_hover_pos();
    let elsewhere = pointer.is_some_and(|at| ui.max_rect().contains(at) && !row.rect.contains(at));
    let command = if row.hovered() || row.clicked() {
        Some(SetOpenCommand::Bool(true))
    } else if elsewhere && egui::Popup::is_id_open(ui.ctx(), id) {
        Some(SetOpenCommand::Bool(false))
    } else {
        None
    };
    egui::Popup::menu(row)
        .open_memory(command)
        .align(egui::RectAlign::RIGHT_START)
        .gap(10.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 2.0;
            content(ui);
        });
}
