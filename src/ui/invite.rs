//! Discord's "Invite friends to <server>" window (the bundle's
//! `InstantInviteModal`): the friends to send the invite to, each with
//! "Invite" (then "Sent"), split into those already in the server and the
//! rest, and at its foot the link with "Copy" and when it expires; "Edit
//! invite link." opens the link's settings.

use egui::{Align2, Color32, FontId, Frame, Margin, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::App;
use crate::discord::model::{ChannelKind, Id};
use crate::icons::{self, Icon};
use crate::theme;

/// The window's state, kept while it is open.
pub struct Invite {
    pub guild: Id,
    pub channel: Id,
    pub filter: String,
    /// The invite's code once Discord made it.
    pub code: Option<String>,
    /// Those it went to: people and group DMs.
    pub sent: Vec<Id>,
    /// "Server Members" and "Invite to Server", open or folded.
    pub open: [bool; 2],
    /// The link's settings page, with its choices.
    pub settings: bool,
    pub max_age: u64,
    pub max_uses: u64,
    pub copied: bool,
}

/// Discord's choices for "Expire After" (seconds, 0 never).
const AGES: [u64; 7] = [1800, 3600, 21600, 43200, 86400, 604800, 0];
/// And for "Max Number of Uses" (0 no limit).
const USES: [u64; 7] = [0, 1, 5, 10, 25, 50, 100];

impl Invite {
    pub fn new(guild: Id, channel: Id) -> Self {
        Self { guild, channel, filter: String::new(), code: None, sent: Vec::new(), open: [true, true], settings: false, max_age: 604800, max_uses: 0, copied: false }
    }
}

/// Asks Discord for the invite the window shows (the "invite-modal" answer).
pub fn request(app: &mut App, channel: Id, max_age: u64, max_uses: u64) {
    app.commands.send(crate::backend::Command::Api {
        key: "invite-modal".into(),
        method: "POST",
        path: format!("/channels/{channel}/invites"),
        body: Some(serde_json::json!({ "max_age": max_age, "max_uses": max_uses })),
    });
}

fn age_label(age: u64) -> String {
    match age {
        0 => t!("Never").into(),
        a if a < 3600 => tn!(a / 60, "{count} minute", "{count} minutes"),
        a if a < 86400 => tn!(a / 3600, "{count} hour", "{count} hours"),
        a => tn!(a / 86400, "{count} day", "{count} days"),
    }
}

fn uses_label(uses: u64) -> String {
    match uses {
        0 => t!("No Limit").into(),
        n => tn!(n, "{count} use", "{count} uses"),
    }
}

/// "Your invite link expires in 7 days." and its variants, as Discord words
/// them by the age and the uses.
fn expiry(age: u64, uses: u64) -> String {
    let uses_text = tn!(uses, "{count} use", "{count} uses");
    match (age, uses) {
        (0, 0) => t!("Your invite link will never expire.").into(),
        (0, _) => t!("Your invite link expires after {uses}.", uses = uses_text),
        (1800, 0) => t!("Your invite link expires in 30 minutes.").into(),
        (1800, _) => t!("Your invite link expires in 30 minutes or after {uses}.", uses = uses_text),
        (a, u) => {
            let time = if a < 86400 { tn!(a / 3600, "{count} hour", "{count} hours") } else { tn!(a / 86400, "{count} day", "{count} days") };
            if u == 0 { t!("Your invite link expires in {time}.", time = time) } else { t!("Your invite link expires in {time} or after {uses}.", time = time, uses = uses_text) }
        }
    }
}

/// A person or a group the invite can go to.
struct Row {
    id: Id,
    /// A group DM's channel; a person's DM is opened when sending.
    group: bool,
    name: String,
    sub: String,
    /// The line is what they are doing, in the online green.
    active: bool,
    avatar: Option<String>,
    status: Option<String>,
    member: bool,
}

fn rows(app: &App, guild: Id, filter: &str) -> Vec<Row> {
    let needle = filter.to_lowercase();
    let recent = |id: Id| app.latest.get(&id).copied().unwrap_or(0);
    let mut out: Vec<(u64, Row)> = Vec::new();
    for relationship in app.relationships.values().filter(|r| r.kind == 1) {
        let user = &relationship.user;
        let name = user.display_name().to_owned();
        if !needle.is_empty() && !name.to_lowercase().contains(&needle) && !user.username.to_lowercase().contains(&needle) {
            continue;
        }
        let dm = app.dms.iter().find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == user.id)).map(|c| recent(c.id).max(c.last_message_id.unwrap_or(0))).unwrap_or(0);
        let presence = app.presences.get(&user.id);
        // Under the name: what they are playing, else the username.
        let doing = presence.and_then(|p| p.activities.iter().find(|a| a.kind != 4 && a.kind != 6)).map(|a| a.short());
        let active = doing.is_some();
        let sub = doing.unwrap_or_else(|| user.username.clone());
        out.push((dm, Row { id: user.id, group: false, name, sub, active, avatar: Some(user.avatar_url()), status: presence.map(|p| p.status.clone()).filter(|s| s != "offline"), member: app.members.contains_key(&(guild, user.id)) }));
    }
    for group in app.dms.iter().filter(|c| c.kind == ChannelKind::GroupDm && !c.message_request) {
        let name = group.title();
        if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
            continue;
        }
        let last = recent(group.id).max(group.last_message_id.unwrap_or(0));
        out.push((last, Row { id: group.id, group: true, name, sub: tn!(group.recipients.len() + 1, "{count} Member", "{count} Members"), active: false, avatar: None, status: None, member: false }));
    }
    // The most recently talked to first, as Discord's list.
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase())));
    out.into_iter().map(|(_, row)| row).collect()
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.invite.take() else { return };
    let p = theme::p();
    let mut keep = !ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let screen = ctx.content_rect();
    egui::Area::new("invite-dim".into()).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(170));
        if response.clicked() {
            keep = false;
        }
    });
    let width = 440.0;
    egui::Area::new("invite-modal".into()).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(12).show(ui, |ui| {
            ui.set_width(width);
            if state.settings {
                settings_page(app, ui, &mut state);
            } else {
                main_page(app, ui, &mut state, &mut keep);
            }
        });
    });
    if keep {
        app.invite = Some(state);
    }
}

fn main_page(app: &mut App, ui: &mut egui::Ui, state: &mut Invite, keep: &mut bool) {
    let p = theme::p();
    let guild_name = app.guild(state.guild).map(|g| g.name.clone()).unwrap_or_default();
    let channel = app.channel(state.channel).cloned();
    Frame::new().inner_margin(Margin { left: 16, right: 16, top: 16, bottom: 8 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // "Invite friends to **Server**", the server in bold, and where they
        // will land under it.
        let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let mut job = egui::text::LayoutJob::default();
        let plain = egui::TextFormat::simple(FontId::proportional(16.0), p.strong);
        let bold = egui::TextFormat::simple(theme::strong(16.0), p.strong);
        super::rich_template(&mut job, t!("Invite friends to {name}"), &plain, &bold, &[("name", &guild_name)]);
        job.wrap = egui::text::TextWrapping::truncate_at_width(title.width() - 32.0);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        ui.painter().galley(pos2(title.left(), title.center().y - galley.size().y / 2.0), galley, p.strong);
        let close = Rect::from_center_size(pos2(title.right() - 10.0, title.center().y), Vec2::splat(24.0));
        let response = ui.interact(close, ui.id().with("invite-close"), Sense::click());
        icons::paint(ui, close.shrink(2.0), Icon::Close, if response.hovered() { p.strong } else { p.muted });
        if response.clicked() {
            *keep = false;
        }
        if let Some(channel) = &channel {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.label(RichText::new(t!("Recipients will land in")).size(12.0).color(p.muted));
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                icons::paint(ui, icon, if channel.kind == ChannelKind::Voice { Icon::Voice } else { Icon::Text }, p.muted);
                ui.label(RichText::new(&channel.name).size(12.0).color(p.text));
            });
        }
        ui.add_space(12.0);
        let search = Frame::new().fill(p.chat).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::symmetric(10, 7)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut state.filter).frame(Frame::NONE).hint_text(RichText::new(t!("Search for friends")).color(p.faint)).desired_width(ui.available_width() - 22.0));
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                icons::paint(ui, icon, Icon::Search, p.muted);
                response
            })
            .inner
        });
        // The box has the keyboard when the window opens (`autoFocus`).
        if state.sent.is_empty() && state.filter.is_empty() && !state.copied && ui.ctx().memory(|m| m.focused().is_none()) {
            search.inner.request_focus();
        }
    });
    let list = rows(app, state.guild, &state.filter);
    let (members, others): (Vec<&Row>, Vec<&Row>) = list.iter().partition(|r| r.member);
    ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
        Frame::new().inner_margin(Margin::symmetric(8, 0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if list.is_empty() {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(t!("No results found")).size(14.0).color(p.muted)));
                ui.add_space(60.0);
            }
            // With both kinds the list is in two foldable sections.
            let sections = !members.is_empty();
            for (index, part) in [members, others].into_iter().enumerate() {
                if part.is_empty() {
                    continue;
                }
                if sections {
                    let title = if index == 0 { t!("Server Members") } else { t!("Invite to Server") };
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0 + if index == 1 { 12.0 } else { 0.0 }), Sense::click());
                    let y = rect.bottom() - 12.0;
                    let galley = ui.painter().layout_no_wrap(title.to_owned(), FontId::proportional(14.0), p.muted);
                    ui.painter().galley(pos2(rect.left() + 8.0, y - galley.size().y / 2.0), galley.clone(), p.muted);
                    let chevron = Rect::from_center_size(pos2(rect.left() + 8.0 + galley.size().x + 12.0, y), Vec2::splat(16.0));
                    // Folded, the chevron turns -90° (`--custom-icon-collapse-rotate`).
                    let angle = if state.open[index] { 0.0 } else { -std::f32::consts::FRAC_PI_2 };
                    icons::paint_rotated(ui, chevron, Icon::ChevronDown, p.muted, angle);
                    if response.clicked() {
                        state.open[index] = !state.open[index];
                    }
                    if !state.open[index] {
                        continue;
                    }
                }
                for row in part {
                    invite_row(app, ui, state, row);
                }
            }
        });
    });
    // The foot: the link with "Copy", and when it expires.
    Frame::new().fill(p.sidebar).corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 }).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(t!("Or send a server invite link to a friend")).font(theme::strong(12.0)).color(p.text));
        ui.add_space(8.0);
        let link = state.code.as_ref().map(|code| format!("https://discord.gg/{code}"));
        let hidden = app.streamer_on() && app.settings.streamer_hide_invites;
        Frame::new().fill(p.chat).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin { left: 10, right: 4, top: 4, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                let shown = match (&link, hidden) {
                    (_, true) => "•".repeat(24),
                    (Some(link), false) => link.clone(),
                    (None, false) => String::new(),
                };
                let room = ui.available_width() - 84.0;
                let (text, _) = ui.allocate_exact_size(vec2(room, 32.0), Sense::hover());
                super::paint_truncated(ui, text, &shown, FontId::proportional(15.0), p.text);
                let label = if state.copied { t!("Copied") } else { t!("Copy") };
                let (button, response) = ui.allocate_exact_size(vec2(76.0, 32.0), Sense::click());
                let fill = if state.copied { p.green } else if response.hovered() { p.accent.linear_multiply(0.85) } else { p.accent };
                ui.painter().rect_filled(button, 8.0, fill);
                ui.painter().text(button.center(), Align2::CENTER_CENTER, label, theme::strong(14.0), Color32::WHITE);
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                    && let Some(link) = &link
                {
                    app.clipboard = Some(link.clone());
                    state.copied = true;
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new(expiry(state.max_age, state.max_uses)).size(12.0).color(p.muted));
            let edit = ui.add(egui::Label::new(RichText::new(t!("Edit invite link.")).size(12.0).color(p.link)).sense(Sense::click()));
            if edit.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                state.settings = true;
            }
        });
    });
}

fn invite_row(app: &mut App, ui: &mut egui::Ui, state: &mut Invite, row: &Row) {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let avatar = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 16.0), Vec2::splat(32.0));
    app.images.circle(ui, avatar, row.avatar.as_deref(), &super::initials(&row.name), 16.0);
    if let Some(status) = &row.status {
        super::friends::dot(ui, avatar, status, if response.hovered() { p.hover } else { p.rail });
    }
    let x = avatar.right() + 12.0;
    let right = rect.right() - 90.0;
    super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.center().y - 17.0), pos2(right, rect.center().y + 1.0)), &row.name, theme::strong(16.0), p.strong);
    super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.center().y + 1.0), pos2(right, rect.center().y + 17.0)), &row.sub, FontId::proportional(12.0), if row.active { p.green } else { p.muted });
    if row.member {
        // Already in the server: Discord writes "Member" in the button's place.
        ui.painter().text(pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, t!("Member"), FontId::proportional(14.0), p.muted);
        return;
    }
    let sent = state.sent.contains(&row.id);
    let label = if sent { t!("Sent") } else { t!("Invite") };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::strong(14.0), p.strong);
    let button = Rect::from_min_size(pos2(rect.right() - 8.0 - galley.size().x - 24.0, rect.center().y - 16.0), vec2(galley.size().x + 24.0, 32.0));
    let clicked = ui.interact(button, ui.id().with(("invite-row", row.id)), if sent { Sense::hover() } else { Sense::click() });
    let fill = if clicked.hovered() && !sent { p.hover.gamma_multiply(1.8) } else { p.selected };
    ui.painter().rect(button, 8.0, fill, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    ui.painter().galley(button.center() - galley.size() / 2.0, galley, if sent { p.muted } else { p.strong });
    if clicked.clicked()
        && let Some(code) = &state.code
    {
        // Sent where it goes without leaving the server: a person's DM (made
        // if new) or the group, as a message with the link.
        let content = format!("https://discord.gg/{code}");
        app.commands.send(crate::backend::Command::SendInvite { user: (!row.group).then_some(row.id), channel: row.group.then_some(row.id), content });
        state.sent.push(row.id);
    }
}

fn settings_page(app: &mut App, ui: &mut egui::Ui, state: &mut Invite) {
    let p = theme::p();
    Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(t!("Server invite link settings")).font(theme::strong(20.0)).color(p.strong));
        ui.add_space(16.0);
        super::form::label(ui, t!("Expire After"));
        let ages: Vec<String> = AGES.iter().map(|a| age_label(*a)).collect();
        if let Some(i) = super::form::select(ui, "invite-age", &age_label(state.max_age), &ages, ui.available_width()) {
            state.max_age = AGES[i];
        }
        ui.add_space(12.0);
        super::form::label(ui, t!("Max Number of Uses"));
        let uses: Vec<String> = USES.iter().map(|u| uses_label(*u)).collect();
        if let Some(i) = super::form::select(ui, "invite-uses", &uses_label(state.max_uses), &uses, ui.available_width()) {
            state.max_uses = USES[i];
        }
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::form::button(ui, t!("Generate a New Link"), super::form::Kind::Primary).clicked() {
                    state.code = None;
                    state.copied = false;
                    state.settings = false;
                    request(app, state.channel, state.max_age, state.max_uses);
                }
                if super::form::button(ui, t!("Cancel"), super::form::Kind::Secondary).clicked() {
                    state.settings = false;
                }
            });
        });
    });
}
