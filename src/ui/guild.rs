//! Server settings, as Discord's: the side column with the server's pages,
//! moderation and people; the overview (icon, name, inactive channel,
//! system messages, default notifications) and safety saved through the
//! unsaved changes bar; roles listed and edited in two tabs (display,
//! permissions with what each allows); emoji, members, invites and bans as
//! Discord's tables; the audit log with an icon per kind of change; and
//! CordFast's own page with every channel's settings. Built from the
//! shared form vocabulary (`form.rs`).

use egui::{Color32, FontId, Frame, Key, Margin, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};
use serde_json::{Value, json};

use super::form::{self, Kind};
use crate::admin::{self, ChannelDraft, RoleDraft, Tab};
use crate::app::{Action, App};
use crate::discord::model::{Channel, ChannelKind, Id, User, sf};
use crate::icons::{self, Icon};
use crate::theme;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(guild_id) = app.admin.as_ref().map(|a| a.guild) else { return };
    let Some(guild) = app.guild(guild_id).cloned() else {
        // Left or removed: nothing to save any more.
        app.admin = None;
        return;
    };
    let perms = guild.my_permissions(app.me_id());
    let tab = app.admin.as_ref().map_or(Tab::Overview, |a| a.tab);
    // Esc closes, unless a field is being typed in.
    if ui.input(|i| i.key_pressed(Key::Escape)) && !ui.ctx().egui_wants_keyboard_input() {
        app.actions.push(Action::CloseGuildSettings);
    }
    let mut page_rect = Rect::NOTHING;
    let mut clicked = None;
    let close = form::layer(
        ui,
        "guild-settings",
        |ui| {
            for (heading, pages) in admin::SECTIONS {
                match heading {
                    Some(heading) => {
                        form::nav_separator(ui);
                        form::nav_header(ui, crate::i18n::tr(heading));
                    }
                    None if pages[0].0 == Tab::Overview => form::nav_header(ui, &guild.name),
                    None => form::nav_separator(ui),
                }
                for (id, label, needs) in *pages {
                    let allowed = *needs == 0 || perms & needs != 0;
                    if form::nav_item(ui, crate::i18n::tr(label), *id == tab, allowed).clicked() {
                        clicked = Some(*id);
                    }
                }
            }
        },
        |ui| {
            page_rect = ui.max_rect();
            if let Some(error) = app.admin.as_ref().and_then(|a| a.error.clone()) {
                Frame::new().fill(p_red_soft()).corner_radius(8).inner_margin(Margin::same(10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(error).color(theme::p().strong));
                });
                ui.add_space(16.0);
            }
            match tab {
                Tab::Overview => overview(app, ui, guild_id),
                Tab::Safety => safety(app, ui),
                Tab::Roles => roles(app, ui, guild_id),
                Tab::Channels => channels(app, ui, guild_id),
                Tab::Members => members(app, ui, guild_id),
                Tab::Invites => invites(app, ui, guild_id),
                Tab::Bans => bans(app, ui, guild_id),
                Tab::Emojis => emojis(app, ui, guild_id),
                Tab::Audit => audit(app, ui),
                other => super::guild_more::show(app, ui, guild_id, other),
            }
        },
    );
    if let Some(tab) = clicked {
        app.gate_asked.retain(|asked| !asked.starts_with("gate:admin:"));
        app.actions.push(Action::GuildTab(tab));
    }
    if close {
        app.actions.push(Action::CloseGuildSettings);
    }
    save_bar(app, ui.ctx(), guild_id, page_rect);
}

/// The error box's colour: the theme's red, faded into the page.
fn p_red_soft() -> Color32 {
    let p = theme::p();
    Color32::from_rgba_unmultiplied(p.red.r(), p.red.g(), p.red.b(), 60)
}

/// Discord's bar while the overview, safety or a role hold changes.
fn save_bar(app: &mut App, ctx: &egui::Context, guild_id: Id, page: Rect) {
    let Some(admin) = app.admin.as_mut() else { return };
    if !admin.unsaved() || page == Rect::NOTHING {
        return;
    }
    let screen = ctx.content_rect();
    let area = Rect::from_min_max(pos2(page.left(), page.top()), pos2(page.right(), screen.bottom()));
    let bar = form::save_bar(ctx, "guild-settings", area, admin.nag);
    if bar.reset {
        match admin.tab {
            Tab::Roles => admin.role.clone_from(&admin.role_saved),
            _ => {
                admin.draft.clone_from(&admin.saved);
                admin.icon_preview = None;
            }
        }
    }
    if bar.save {
        match admin.tab {
            Tab::Roles => {
                if let Some(draft) = admin.role.clone() {
                    app.commands.send(admin::change("PATCH", format!("/guilds/{guild_id}/roles/{}", draft.id), Some(admin::role_body(&draft)), &format!("roles:{}", t!("Role saved."))));
                }
            }
            _ => {
                if let (Some(draft), Some(saved)) = (&admin.draft, &admin.saved) {
                    let body = draft.changes(saved);
                    app.commands.send(admin::change("PATCH", format!("/guilds/{guild_id}"), Some(body), &format!("overview:{}", t!("Changes saved."))));
                }
            }
        }
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// A table's column names, in Discord's small capitals.
fn table_header(ui: &mut egui::Ui, columns: &[(&str, f32)]) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    for (name, x) in columns {
        ui.painter().text(pos2(rect.left() + x, rect.center().y), egui::Align2::LEFT_CENTER, name.to_uppercase(), theme::strong(12.0), p.muted);
    }
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
}

/// An "ISO date" as Discord's tables write it: "Oct 3, 2015".
fn iso_day(text: &str) -> String {
    text.parse::<jiff::Timestamp>().map(|t| crate::text::short_day(t.as_millisecond() as u64)).unwrap_or_default()
}

// ---- overview ---------------------------------------------------------------------

fn overview(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    form::title(ui, t!("Server Overview"));
    let Some(guild) = app.guild(guild_id).cloned() else { return };
    if app.admin.as_ref().is_none_or(|a| a.draft.is_none()) {
        ui.spinner();
        return;
    }
    let community = app.admin.as_ref().and_then(|a| a.loaded.as_ref()).and_then(|l| l.get("features")).and_then(Value::as_array).is_some_and(|f| f.iter().any(|x| x.as_str() == Some("COMMUNITY")));

    // The icon, what Discord recommends, and the name.
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        ui.allocate_ui_with_layout(vec2(140.0, 160.0), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_width(140.0);
            icon_picker(app, ui, guild_id, &guild);
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(t!("Minimum Size: {size}x{size}", size = 128)).size(12.0).color(theme::p().muted)).extend());
        });
        ui.vertical(|ui| {
            ui.set_width(180.0);
            form::note(ui, t!("We recommend an image of at least 512x512."));
            ui.add_space(10.0);
            if form::button(ui, t!("Upload Image"), Kind::Secondary).clicked() {
                pick_icon(app, ui.ctx());
            }
        });
        ui.vertical(|ui| {
            form::label(ui, t!("Server Name"));
            let width = ui.available_width();
            if let Some(draft) = app.admin.as_mut().and_then(|a| a.draft.as_mut()) {
                form::input(ui, &mut draft.name, "", width);
            }
        });
    });
    form::divider(ui);

    // Inactive channel and timeout, side by side.
    let voice: Vec<(Id, String)> = sorted(&guild.channels).into_iter().filter(|c| matches!(c.kind, ChannelKind::Voice)).map(|c| (c.id, c.name.clone())).collect();
    let text: Vec<(Id, String)> = sorted(&guild.channels).into_iter().filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement)).map(|c| (c.id, c.name.clone())).collect();
    let Some(draft) = app.admin.as_mut().and_then(|a| a.draft.as_mut()) else { return };
    ui.columns(2, |columns| {
        let ui = &mut columns[0];
        form::label(ui, t!("Inactive Channel"));
        let mut options = vec![t!("No Inactive Channel").to_owned()];
        options.extend(voice.iter().map(|(_, n)| n.clone()));
        let shown = draft.afk_channel.and_then(|id| voice.iter().find(|(c, _)| *c == id)).map_or_else(|| options[0].clone(), |(_, n)| n.clone());
        let width = ui.available_width() - 10.0;
        if let Some(i) = form::select(ui, "afk-channel", &shown, &options, width) {
            draft.afk_channel = if i == 0 { None } else { voice.get(i - 1).map(|(id, _)| *id) };
        }
        let ui = &mut columns[1];
        form::label(ui, t!("Inactive Timeout"));
        const TIMES: [(u64, &str); 5] = [(60, n!("1 minute")), (300, n!("5 minutes")), (900, n!("15 minutes")), (1800, n!("30 minutes")), (3600, n!("1 hour"))];
        let options: Vec<String> = TIMES.iter().map(|(_, t)| crate::i18n::tr(t).to_owned()).collect();
        let shown = crate::i18n::tr(TIMES.iter().find(|(s, _)| *s == draft.afk_timeout).map_or(TIMES[1].1, |(_, t)| t));
        let width = ui.available_width();
        if let Some(i) = form::select(ui, "afk-timeout", shown, &options, width) {
            draft.afk_timeout = TIMES[i].0;
        }
    });
    ui.add_space(8.0);
    form::note(ui, t!("Automatically move members to this channel and mute them when they have been idle for longer than the inactive timeout. This does not affect browsers."));
    form::divider(ui);

    // System messages.
    form::label(ui, t!("System Messages Channel"));
    let mut options = vec![t!("No System Messages").to_owned()];
    options.extend(text.iter().map(|(_, n)| format!("# {n}")));
    let shown = draft.system_channel.and_then(|id| text.iter().find(|(c, _)| *c == id)).map_or_else(|| options[0].clone(), |(_, n)| format!("# {n}"));
    let width = ui.available_width().min(360.0);
    if let Some(i) = form::select(ui, "system-channel", &shown, &options, width) {
        draft.system_channel = if i == 0 { None } else { text.get(i - 1).map(|(id, _)| *id) };
    }
    ui.add_space(8.0);
    form::note(ui, t!("This is the channel we send system event messages to. These can be turned off at any time."));
    ui.add_space(20.0);
    // Each switch is on while its system_channel_flags bit is off.
    for (bit, title) in [
        (0, t!("Send a random welcome message when someone joins this server.")),
        (3, t!("Prompt members to reply to welcome messages with a sticker.")),
        (1, t!("Send a message when someone boosts this server.")),
        (2, t!("Send helpful tips for server setup.")),
    ] {
        let mut on = draft.system_flags & (1 << bit) == 0;
        if form::switch_row(ui, &mut on, title, "") {
            draft.system_flags ^= 1 << bit;
        }
    }
    form::divider(ui);

    // Default notifications.
    form::label(ui, t!("Default Notification Settings"));
    form::note(ui, t!("This will determine whether members who have not explicitly set their notification settings receive a notification for every message sent in this server or not."));
    ui.add_space(4.0);
    form::note(ui, t!("We highly recommend setting this to only @mentions for a Community Server."));
    ui.add_space(14.0);
    if let Some(value) = form::radio_cards(ui, draft.notifications, &[(0, t!("All Messages"), ""), (1, t!("Only @mentions"), "")]) {
        draft.notifications = value;
    }
    // A community server's description, shown in Discord's server discovery.
    if community || !draft.description.is_empty() {
        form::divider(ui);
        form::label(ui, t!("Server Description"));
        form::textarea(ui, &mut draft.description, t!("What's your server about?"), 3, 120);
    }
    ui.add_space(60.0);
}

/// The server's icon, 100 points round: the new one when picked, "CHANGE
/// ICON" over it on hover, and "Remove" under it.
fn icon_picker(app: &mut App, ui: &mut egui::Ui, guild_id: Id, guild: &crate::discord::model::Guild) {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(100.0), Sense::click());
    let (removed, preview) = app.admin.as_ref().map_or((false, None), |a| (a.draft.as_ref().and_then(|d| d.icon.as_deref()) == Some(""), a.icon_preview.clone()));
    match (&preview, removed) {
        (Some(texture), _) => {
            egui::Image::from_texture((texture.id(), rect.size())).corner_radius(50.0).paint_at(ui, rect);
        }
        (None, true) => app.images.circle(ui, rect, None, &super::initials(&guild.name), 50.0),
        (None, false) => app.images.circle(ui, rect, guild.icon_url().as_deref(), &super::initials(&guild.name), 50.0),
    }
    if response.hovered() {
        ui.painter().circle_filled(rect.center(), 50.0, Color32::from_black_alpha(150));
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, t!("Change\nIcon").to_uppercase(), theme::strong(11.0), Color32::WHITE);
    }
    // Discord's small badge with a plus at the top right.
    let badge = rect.right_top() + vec2(-12.0, 12.0);
    ui.painter().circle_filled(badge, 13.0, p.strong);
    icons::paint(ui, Rect::from_center_size(badge, Vec2::splat(12.0)), Icon::Plus, p.rail);
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        pick_icon(app, ui.ctx());
    }
    let has_icon = preview.is_some() || (!removed && guild.icon.is_some());
    if has_icon {
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            if form::button(ui, t!("Remove"), Kind::Link).clicked()
                && let Some(admin) = app.admin.as_mut()
                && let Some(draft) = admin.draft.as_mut()
            {
                draft.icon = Some(String::new());
                admin.icon_preview = None;
            }
        });
    }
    let _ = guild_id;
}

/// Picks a picture for the icon: kept as the data URI the save sends, and
/// shown at once.
fn pick_icon(app: &mut App, ctx: &egui::Context) {
    let Some(file) = app.settings.file_dialog().add_filter(t!("Image"), &["png", "jpg", "jpeg", "gif", "webp"]).pick_file() else { return };
    app.settings.picked(&file);
    let Some(data) = admin::image_data(&file) else {
        app.notify(t!("Invalid image, or larger than 10 MB."));
        return;
    };
    let preview = image::open(&file).ok().map(|picture| {
        let picture = picture.thumbnail(256, 256).to_rgba8();
        let size = [picture.width() as usize, picture.height() as usize];
        ctx.load_texture("guild-icon-preview", egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw()), egui::TextureOptions::LINEAR)
    });
    if let Some(admin) = app.admin.as_mut()
        && let Some(draft) = admin.draft.as_mut()
    {
        draft.icon = Some(data);
        admin.icon_preview = preview;
    }
}

/// The server's channels in the order its list shows them.
fn sorted(channels: &[Channel]) -> Vec<&Channel> {
    let mut list: Vec<&Channel> = channels.iter().filter(|c| c.kind != ChannelKind::Thread).collect();
    list.sort_by_key(|c| {
        let parent = c.parent_id.and_then(|p| channels.iter().find(|x| x.id == p)).map_or(-1, |p| p.position);
        let own = if c.kind == ChannelKind::Category { (c.position, false, -1) } else { (parent, true, c.position) };
        (own.0, own.1, matches!(c.kind, ChannelKind::Voice | ChannelKind::Stage), own.2)
    });
    list
}

// ---- safety -----------------------------------------------------------------------

fn safety(app: &mut App, ui: &mut egui::Ui) {
    form::title(ui, t!("Safety Setup"));
    let Some(draft) = app.admin.as_mut().and_then(|a| a.draft.as_mut()) else {
        ui.spinner();
        return;
    };
    // The levels and filters as Discord explains them (the bundle's
    // guild settings safety page).
    form::label(ui, t!("Verification Level"));
    form::note(ui, t!("Members of the server must meet the following criteria before they can send messages in text channels or initiate a direct message conversation. If a member has an assigned role and server onboarding is not enabled, this does not apply. We recommend setting a verification level for a Community Server."));
    ui.add_space(14.0);
    if let Some(level) = form::radio_cards(
        ui,
        draft.verification,
        &[
            (0, t!("None"), t!("Unrestricted")),
            (1, t!("Low"), t!("Must have a verified email on their Discord account.")),
            (2, t!("Medium"), &t!("Must also be registered on Discord for longer than {min} minutes.", min = 5)),
            (3, t!("High"), &t!("Must also be a member of this server for longer than {min} minutes.", min = 10)),
            (4, t!("Highest"), t!("Must have a verified phone on their Discord account.")),
        ],
    ) {
        draft.verification = level;
    }
    form::divider(ui);
    form::label(ui, t!("Sensitive content filters"));
    form::note(ui, t!("Choose if server members can share image-based media detected by Discord’s sensitive content filters. This setting will apply to channels that are not age-restricted."));
    ui.add_space(14.0);
    // Discord's order: the strictest first.
    if let Some(filter) = form::radio_cards(
        ui,
        draft.content_filter,
        &[
            (2, t!("Filter messages from all members"), t!("All messages will be filtered for sensitive image-based media.")),
            (1, t!("Filter messages from server members without roles"), t!("Messages from server members without a role will be filtered for sensitive image-based media.")),
            (0, t!("Do not filter"), t!("Messages will not be filtered for sensitive image-based media.")),
        ],
    ) {
        draft.content_filter = filter;
    }
    ui.add_space(60.0);
}

// ---- roles --------------------------------------------------------------------------

/// Discord's role colours: the twenty swatches of its role editor.
const SWATCHES: [u32; 20] = [
    0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6, 0x607d8b,
    0x11806a, 0x1f8b4c, 0x206694, 0x71368a, 0xad1457, 0xc27c0e, 0xa84300, 0x992d22, 0x979c9f, 0x546e7a,
];

fn open_role(app: &mut App, role: &crate::discord::model::Role) {
    let Some(admin) = app.admin.as_mut() else { return };
    if admin.unsaved() {
        admin.nag = Some(std::time::Instant::now());
        return;
    }
    let draft = RoleDraft {
        id: role.id,
        name: role.name.clone(),
        color: [(role.color >> 16) as u8, (role.color >> 8) as u8, role.color as u8],
        hoist: role.hoist,
        mentionable: role.mentionable,
        permissions: role.permissions,
    };
    admin.role_saved = Some(draft.clone());
    admin.role = Some(draft);
    admin.role_tab = if role.id == admin.guild { 1 } else { 0 };
}

fn roles(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    let Some(guild) = app.guild(guild_id).cloned() else { return };
    let mut list = guild.roles.clone();
    list.sort_by_key(|r| std::cmp::Reverse(r.position));
    if app.admin.as_ref().is_some_and(|a| a.role.is_some()) {
        role_editor(app, ui, guild_id, &list);
        return;
    }
    let p = theme::p();
    form::title(ui, t!("Roles"));
    form::note(ui, t!("Use roles to group your server members and assign permissions."));
    ui.add_space(16.0);
    // @everyone first, as Discord's "Default Permissions" card.
    if let Some(everyone) = list.iter().find(|r| r.id == guild_id).cloned() {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
        ui.painter().rect_filled(rect, 8.0, if response.hovered() { p.hover } else { p.sidebar });
        let badge = pos2(rect.left() + 32.0, rect.center().y);
        ui.painter().circle_filled(badge, 20.0, p.selected);
        icons::paint(ui, Rect::from_center_size(badge, Vec2::splat(22.0)), Icon::Members, p.strong);
        ui.painter().text(pos2(rect.left() + 64.0, rect.center().y - 9.0), egui::Align2::LEFT_CENTER, t!("Default Permissions"), theme::strong(16.0), p.strong);
        ui.painter().text(pos2(rect.left() + 64.0, rect.center().y + 11.0), egui::Align2::LEFT_CENTER, t!("@everyone • applies to all server members"), FontId::proportional(14.0), p.muted);
        icons::paint_chevron_right(ui, Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), Vec2::splat(18.0)), p.muted);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open_role(app, &everyone);
        }
        ui.add_space(24.0);
    }
    // Search and create.
    let mut create = false;
    ui.horizontal(|ui| {
        let width = ui.available_width() - 130.0;
        if let Some(admin) = app.admin.as_mut() {
            form::input(ui, &mut admin.role_query, t!("Search Roles"), width);
        }
        create = form::button(ui, t!("Create Role"), Kind::Primary).clicked();
    });
    if create {
        app.commands.send(admin::change("POST", format!("/guilds/{guild_id}/roles"), Some(json!({ "name": t!("new role") })), &format!("roles:{}", t!("Role created."))));
    }
    ui.add_space(10.0);
    form::note(ui, t!("Members use the color of the highest role they have on this list."));
    ui.add_space(14.0);
    let query = app.admin.as_ref().map(|a| a.role_query.trim().to_lowercase()).unwrap_or_default();
    let shown: Vec<_> = list.iter().filter(|r| r.id != guild_id && (query.is_empty() || r.name.to_lowercase().contains(&query))).cloned().collect();
    table_header(ui, &[(&t!("Roles — {count}", count = shown.len()), 12.0)]);
    for role in shown {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
        let color = theme::role_color(role.color).unwrap_or(p.muted);
        role_shield(ui, pos2(rect.left() + 24.0, rect.center().y), color);
        let name_right = rect.right() - 80.0;
        super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 48.0, rect.top()), pos2(name_right, rect.bottom())), &role.name, theme::strong(16.0), p.strong);
        if role.managed {
            ui.painter().text(pos2(name_right - 4.0, rect.center().y), egui::Align2::RIGHT_CENTER, t!("Managed by an app"), FontId::proportional(12.5), p.faint);
        }
        // The pencil on hover, as Discord's row.
        let pencil = Rect::from_center_size(pos2(rect.right() - 28.0, rect.center().y), Vec2::splat(32.0));
        ui.painter().circle_filled(pencil.center(), 16.0, if response.hovered() { p.selected } else { p.sidebar });
        icons::paint(ui, pencil.shrink(8.0), Icon::Pencil, if response.hovered() { p.strong } else { p.muted });
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open_role(app, &role);
        }
    }
}

/// A role's mark: Discord's small shield, here a filled badge of its colour.
fn role_shield(ui: &egui::Ui, center: egui::Pos2, color: Color32) {
    let points = vec![
        center + vec2(-8.0, -9.0),
        center + vec2(8.0, -9.0),
        center + vec2(8.0, 1.0),
        center + vec2(0.0, 9.0),
        center + vec2(-8.0, 1.0),
    ];
    ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

fn role_editor(app: &mut App, ui: &mut egui::Ui, guild_id: Id, list: &[crate::discord::model::Role]) {
    let p = theme::p();
    let Some(current) = app.admin.as_ref().and_then(|a| a.role.clone()) else { return };
    let everyone = current.id == guild_id;
    ui.horizontal_top(|ui| {
        // The roles on the left, as Discord's editor keeps them.
        ui.vertical(|ui| {
            ui.set_width(190.0);
            ui.horizontal(|ui| {
                if form::button(ui, t!("‹ Back"), Kind::Link).clicked()
                    && let Some(admin) = app.admin.as_mut()
                {
                    if admin.unsaved() {
                        admin.nag = Some(std::time::Instant::now());
                    } else {
                        admin.role = None;
                        admin.role_saved = None;
                    }
                }
            });
            ui.add_space(8.0);
            for role in list {
                let selected = role.id == current.id;
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                if selected || response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, if selected { p.selected } else { p.hover });
                }
                let color = theme::role_color(role.color).unwrap_or(p.muted);
                ui.painter().circle_filled(pos2(rect.left() + 14.0, rect.center().y), 6.0, color);
                let name = if role.id == guild_id { "@everyone" } else { &role.name };
                super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 28.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom())), name, FontId::proportional(15.0), if selected { p.strong } else { p.text });
                if response.clicked() && !selected {
                    open_role(app, role);
                }
            }
        });
        let (line, _) = ui.allocate_exact_size(vec2(1.0, 520.0), Sense::hover());
        ui.painter().vline(line.center().x, line.y_range(), Stroke::new(1.0, p.selected));
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let name = if everyone { "@everyone".to_owned() } else { current.name.clone() };
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Edit Role — {name}", name = name).to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                if !everyone {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if form::button(ui, t!("Delete Role"), Kind::Link).clicked() {
                            app.modal = Some(crate::app::Modal::Confirm {
                                title: t!("Delete Role").into(),
                                body: t!("Are you sure you want to delete the role {name}? This action cannot be undone.", name = name),
                                button: t!("Delete").into(),
                                then: crate::app::Confirmed::DeleteRole { guild: guild_id, role: current.id },
                            });
                        }
                    });
                }
            });
            ui.add_space(12.0);
            // Discord's tabs: underlined while chosen.
            let tab = app.admin.as_ref().map_or(0, |a| a.role_tab);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                for (i, label) in [(0u8, t!("Display")), (1, t!("Permissions"))] {
                    if everyone && i == 0 {
                        continue;
                    }
                    let chosen = tab == i;
                    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::strong(16.0), p.strong);
                    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x, 32.0), Sense::click());
                    let color = if chosen || response.hovered() { p.strong } else { p.muted };
                    ui.painter().galley(pos2(rect.left(), rect.top() + 4.0), galley, color);
                    if chosen {
                        ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(2.0, p.accent));
                    }
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                        && let Some(admin) = app.admin.as_mut()
                    {
                        admin.role_tab = i;
                    }
                }
            });
            let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().hline(rule.x_range(), rule.center().y, Stroke::new(1.0, p.selected));
            ui.add_space(20.0);
            let Some(admin) = app.admin.as_mut() else { return };
            let tab = admin.role_tab;
            let query = admin.permission_query.clone();
            let Some(draft) = admin.role.as_mut() else { return };
            if tab == 0 && !everyone {
                form::label(ui, t!("Role Name"));
                let width = ui.available_width();
                form::input(ui, &mut draft.name, "", width);
                form::divider(ui);
                form::label(ui, t!("Role Color"));
                form::note(ui, t!("Members use the color of the highest role they have on the roles list."));
                ui.add_space(14.0);
                color_swatches(ui, &mut draft.color);
                form::divider(ui);
                form::switch_row(ui, &mut draft.hoist, t!("Display role members separately from online members"), "");
                form::switch_row(ui, &mut draft.mentionable, t!("Allow anyone to @mention this role"), t!("Note: Members with the \"Mention @everyone, @here, and All Roles\" permission will always be able to ping this role."));
            } else {
                let mut query = query;
                ui.horizontal(|ui| {
                    let width = ui.available_width() - 150.0;
                    form::input(ui, &mut query, t!("Search permissions"), width);
                    if form::button(ui, t!("Clear permissions"), Kind::Link).clicked() {
                        draft.permissions = 0;
                    }
                });
                ui.add_space(16.0);
                let needle = query.trim().to_lowercase();
                for (group, perms) in admin::PERMISSIONS {
                    let shown: Vec<&(&str, u32)> = perms.iter().filter(|(name, _)| needle.is_empty() || crate::i18n::tr(name).to_lowercase().contains(&needle)).collect();
                    if shown.is_empty() {
                        continue;
                    }
                    form::label(ui, crate::i18n::tr(group));
                    ui.add_space(6.0);
                    for (name, bit) in shown {
                        let mask = 1u64 << bit;
                        let mut on = draft.permissions & mask != 0;
                        if form::switch_row(ui, &mut on, crate::i18n::tr(name), admin::permission_note(*bit)) {
                            if on { draft.permissions |= mask } else { draft.permissions &= !mask }
                        }
                    }
                    ui.add_space(8.0);
                }
                if let Some(admin) = app.admin.as_mut() {
                    admin.permission_query = query;
                }
            }
            ui.add_space(60.0);
        });
    });
}

/// Discord's colour choice: "Default" and the custom picker as big
/// swatches, then two rows of ten.
fn color_swatches(ui: &mut egui::Ui, color: &mut [u8; 3]) {
    let p = theme::p();
    let current = u32::from(color[0]) << 16 | u32::from(color[1]) << 8 | u32::from(color[2]);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        // "Default": no colour of its own.
        let (rect, response) = ui.allocate_exact_size(vec2(66.0, 50.0), Sense::click());
        ui.painter().rect_filled(rect, 6.0, Color32::from_rgb(0x99, 0xaa, 0xb5));
        if current == 0 {
            icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), Icon::Check, Color32::WHITE);
        }
        if response.on_hover_text(t!("Default")).clicked() {
            *color = [0, 0, 0];
        }
        // The custom colour: egui's picker, framed as Discord's swatch.
        ui.scope(|ui| {
            ui.spacing_mut().interact_size = vec2(66.0, 50.0);
            ui.color_edit_button_srgb(color).on_hover_text(t!("Custom Color"));
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for row in SWATCHES.chunks(10) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for hex in row {
                        let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                        let swatch = Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, *hex as u8);
                        ui.painter().rect_filled(rect, 4.0, swatch);
                        if *hex == current {
                            icons::paint(ui, rect.shrink(3.0), Icon::Check, Color32::WHITE);
                        } else if response.hovered() {
                            ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0, p.strong), egui::StrokeKind::Outside);
                        }
                        if response.clicked() {
                            *color = [(hex >> 16) as u8, (hex >> 8) as u8, *hex as u8];
                        }
                    }
                });
            }
        });
    });
}

// ---- channels -----------------------------------------------------------------------

fn channels(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    let p = theme::p();
    form::title(ui, t!("Channels"));
    form::note(ui, t!("Every channel of the server, in the list's order. Pick one to edit; to reorder them, drag them in the channel list."));
    ui.add_space(16.0);
    let Some(guild) = app.guild(guild_id).cloned() else { return };
    // A new channel: its name, kind and the button.
    let mut create = None;
    if let Some(admin) = app.admin.as_mut() {
        ui.horizontal(|ui| {
            let width = ui.available_width() - 150.0 - 130.0 - 16.0;
            form::input(ui, &mut admin.new_channel, t!("new-channel"), width);
            const KINDS: [(u8, &str); 5] = [(0, n!("Text")), (2, n!("Voice")), (4, n!("Category")), (5, n!("Announcement")), (15, n!("Forum"))];
            let options: Vec<String> = KINDS.iter().map(|(_, k)| crate::i18n::tr(k).to_owned()).collect();
            let shown = crate::i18n::tr(KINDS.iter().find(|(k, _)| *k == admin.new_channel_kind).map_or(KINDS[0].1, |(_, t)| t));
            if let Some(i) = form::select(ui, "new-channel-kind", shown, &options, 140.0) {
                admin.new_channel_kind = KINDS[i].0;
            }
            if form::button_enabled(ui, t!("Create Channel"), Kind::Primary, !admin.new_channel.trim().is_empty()).clicked() {
                create = Some(json!({ "name": admin.new_channel.trim(), "type": admin.new_channel_kind }));
                admin.new_channel.clear();
            }
        });
    }
    if let Some(body) = create {
        app.commands.send(admin::change("POST", format!("/guilds/{guild_id}/channels"), Some(body), &format!("channels:{}", t!("Channel created."))));
    }
    ui.add_space(20.0);
    let list: Vec<Channel> = sorted(&guild.channels).into_iter().cloned().collect();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(240.0);
            for channel in &list {
                let selected = app.admin.as_ref().and_then(|a| a.channel.as_ref()).is_some_and(|d| d.id == channel.id);
                let category = channel.kind == ChannelKind::Category;
                if category {
                    ui.add_space(10.0);
                }
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), if category { 24.0 } else { 32.0 }), Sense::click());
                if selected || response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, if selected { p.selected } else { p.hover });
                }
                let (icon, indent) = match channel.kind {
                    ChannelKind::Category => (None, 4.0),
                    ChannelKind::Voice => (Some(Icon::Voice), 8.0),
                    ChannelKind::Stage => (Some(Icon::Stage), 8.0),
                    ChannelKind::Announcement => (Some(Icon::Announcements), 8.0),
                    ChannelKind::Forum => (Some(Icon::Forum), 8.0),
                    _ => (Some(Icon::Text), 8.0),
                };
                let mut x = rect.left() + indent;
                if let Some(icon) = icon {
                    icons::paint(ui, Rect::from_center_size(pos2(x + 9.0, rect.center().y), Vec2::splat(18.0)), icon, p.faint);
                    x += 24.0;
                }
                let (name, font, color) = if category {
                    (channel.name.to_uppercase(), theme::strong(12.0), p.muted)
                } else {
                    (channel.name.clone(), FontId::proportional(15.0), if selected { p.strong } else { p.text })
                };
                super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 6.0, rect.bottom())), &name, font, color);
                if response.clicked()
                    && let Some(admin) = app.admin.as_mut()
                {
                    admin.channel = Some(ChannelDraft {
                        id: channel.id,
                        name: channel.name.clone(),
                        topic: channel.topic.clone().unwrap_or_default(),
                        nsfw: channel.nsfw,
                        slowmode: channel.slowmode,
                    });
                }
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let Some(admin) = app.admin.as_mut() else { return };
            let Some(draft) = admin.channel.as_mut() else {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(t!("Pick a channel to edit.")).size(15.0).color(p.muted)));
                return;
            };
            let channel = list.iter().find(|c| c.id == draft.id);
            let category = channel.is_some_and(|c| c.kind == ChannelKind::Category);
            let text = channel.is_some_and(|c| c.kind.is_text());
            form::label(ui, if category { t!("Category Name") } else { t!("Channel Name") });
            let width = ui.available_width();
            form::input(ui, &mut draft.name, "", width);
            if text {
                ui.add_space(20.0);
                form::label(ui, t!("Channel Topic"));
                form::textarea(ui, &mut draft.topic, t!("Let everyone know how to use this channel!"), 3, 1024);
                ui.add_space(20.0);
                form::label(ui, t!("Slowmode"));
                const SLOW: [(u32, &str); 14] = [
                    (0, n!("Off")), (5, "5s"), (10, "10s"), (15, "15s"), (30, "30s"), (60, "1min"), (120, "2min"),
                    (300, "5min"), (600, "10min"), (900, "15min"), (1800, "30min"), (3600, "1h"), (7200, "2h"), (21600, "6h"),
                ];
                let options: Vec<String> = SLOW.iter().map(|(_, t)| crate::i18n::tr(t).to_owned()).collect();
                let shown = SLOW.iter().find(|(s, _)| *s == draft.slowmode).map_or_else(|| format!("{}s", draft.slowmode), |(_, t)| crate::i18n::tr(t).to_owned());
                if let Some(i) = form::select(ui, "slowmode", &shown, &options, 200.0) {
                    draft.slowmode = SLOW[i].0;
                }
                ui.add_space(8.0);
                form::note(ui, t!("Members will be restricted to sending one message per this interval, unless they have Manage Channel or Manage Messages permissions."));
                ui.add_space(20.0);
                form::switch_row(ui, &mut draft.nsfw, t!("Age-Restricted Channel"), t!("Users will need to confirm they are of over legal age to view the content in this channel."));
            }
            ui.add_space(12.0);
            let draft = draft.clone();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if form::button(ui, t!("Save Changes"), Kind::Success).clicked() {
                    let mut body = json!({ "name": draft.name.trim() });
                    if text {
                        body["topic"] = json!(draft.topic);
                        body["nsfw"] = json!(draft.nsfw);
                        body["rate_limit_per_user"] = json!(draft.slowmode);
                    }
                    app.commands.send(admin::change("PATCH", format!("/channels/{}", draft.id), Some(body), &format!("channels:{}", t!("Channel saved."))));
                }
                let delete = if category { t!("Delete Category") } else { t!("Delete Channel") };
                if form::button(ui, delete, Kind::Danger).clicked() {
                    app.modal = Some(crate::app::Modal::Confirm {
                        title: delete.to_owned(),
                        body: t!("Are you sure you want to delete {name}? This cannot be undone.", name = draft.name),
                        button: delete.to_owned(),
                        then: crate::app::Confirmed::DeleteChannel { channel: draft.id },
                    });
                }
            });
        });
    });
}

// ---- members ------------------------------------------------------------------------

fn members(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    let p = theme::p();
    form::title(ui, t!("Server Members"));
    let Some(guild) = app.guild(guild_id).cloned() else { return };
    let perms = guild.my_permissions(app.me_id());
    let mut search = false;
    if let Some(admin) = app.admin.as_mut() {
        let width = ui.available_width();
        let response = form::input(ui, &mut admin.query, t!("Search by username or ID"), width);
        search = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if search && let Some(command) = admin.load(Tab::Members) {
            app.commands.send(command);
        }
    }
    let _ = search;
    ui.add_space(16.0);
    let Some(list) = app.admin.as_ref().and_then(|a| a.members.clone()) else {
        ui.spinner();
        return;
    };
    let width = ui.available_width();
    // Dates need some 130 points each; the name and the roles share the rest.
    let (since_x, joined_x, roles_x) = (width * 0.34, width * 0.34 + 150.0, width * 0.34 + 300.0);
    table_header(ui, &[(t!("Name"), 12.0), (t!("Member Since"), since_x), (t!("Joined Discord"), joined_x), (t!("Roles"), roles_x)]);
    if list.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(RichText::new(t!("No one found.")).color(p.muted)));
    }
    for member in &list {
        let Some(user) = member.get("user").and_then(User::parse) else { continue };
        let nick = str_of(member, "nick");
        let member_roles: Vec<Id> = member.get("roles").and_then(Value::as_array).map(|r| r.iter().filter_map(sf).collect()).unwrap_or_default();
        let timed_out = member.get("communication_disabled_until").and_then(Value::as_str).and_then(|t| t.parse::<jiff::Timestamp>().ok()).is_some_and(|t| t > jiff::Timestamp::now());
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
        if response.hovered() || response.context_menu_opened() {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
        let avatar = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 16.0), Vec2::splat(32.0));
        app.images.circle(ui, avatar, Some(&user.avatar_url()), &super::initials(user.display_name()), 16.0);
        let shown = if nick.is_empty() { user.display_name() } else { nick };
        let color = theme::role_color(guild.role_color(&member_roles)).unwrap_or(p.strong);
        let name_right = rect.left() + since_x - 12.0;
        super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 10.0, rect.center().y - 18.0), pos2(name_right, rect.center().y)), shown, theme::strong(15.0), color);
        let sub = if timed_out { format!("{} • {}", user.username, t!("timed out")) } else { user.username.clone() };
        super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 10.0, rect.center().y), pos2(name_right, rect.center().y + 18.0)), &sub, FontId::proportional(13.0), if timed_out { p.warning } else { p.muted });
        let since = iso_day(str_of(member, "joined_at"));
        ui.painter().text(pos2(rect.left() + since_x, rect.center().y), egui::Align2::LEFT_CENTER, since, FontId::proportional(14.0), p.text);
        let created = crate::text::short_day(crate::discord::model::snowflake_ms(user.id));
        ui.painter().text(pos2(rect.left() + joined_x, rect.center().y), egui::Align2::LEFT_CENTER, created, FontId::proportional(14.0), p.text);
        // The first role as a chip, then how many more.
        let roles = guild.roles_of(&member_roles);
        let mut x = rect.left() + roles_x;
        if let Some(role) = roles.first() {
            let galley = ui.painter().layout_no_wrap(role.name.clone(), FontId::proportional(12.5), p.text);
            let chip_width = (galley.size().x + 24.0).min(rect.right() - 60.0 - x);
            let chip = Rect::from_min_size(pos2(x, rect.center().y - 11.0), vec2(chip_width, 22.0));
            ui.painter().rect_filled(chip, 4.0, p.sidebar);
            ui.painter().circle_filled(pos2(chip.left() + 10.0, chip.center().y), 5.0, theme::role_color(role.color).unwrap_or(p.muted));
            super::paint_truncated(ui, Rect::from_min_max(pos2(chip.left() + 19.0, chip.top()), pos2(chip.right() - 4.0, chip.bottom())), &role.name, FontId::proportional(12.5), p.text);
            x = chip.right() + 6.0;
            if roles.len() > 1 {
                ui.painter().text(pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, format!("+{}", roles.len() - 1), theme::strong(12.5), p.muted);
            }
        }
        // "⋮" with what can be done to them.
        let more = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), Vec2::splat(32.0));
        let more_response = ui.interact(more, ui.id().with(("member-more", user.id)), Sense::click());
        if more_response.hovered() {
            ui.painter().circle_filled(more.center(), 16.0, p.selected);
        }
        for dy in [-6.0, 0.0, 6.0] {
            ui.painter().circle_filled(more.center() + vec2(0.0, dy), 2.0, if more_response.hovered() { p.strong } else { p.muted });
        }
        let menu = |app: &mut App, ui: &mut egui::Ui| member_menu(app, ui, guild_id, &guild, &user, &member_roles, timed_out, perms);
        egui::Popup::menu(&more_response).show(|ui| menu(app, ui));
        response.context_menu(|ui| menu(app, ui));
    }
}

#[allow(clippy::too_many_arguments)]
fn member_menu(app: &mut App, ui: &mut egui::Ui, guild_id: Id, guild: &crate::discord::model::Guild, user: &User, member_roles: &[Id], timed_out: bool, perms: u64) {
    use super::menu;
    menu::begin(ui);
    let u = user.id;
    if menu::item(ui, t!("Profile"), menu::Tail::None).clicked() {
        app.actions.push(Action::OpenProfile { user: user.clone(), guild: Some(guild_id), at: ui.max_rect().left_top() });
        ui.close();
    }
    if perms & (1 << 28) != 0 {
        let mut roles = guild.roles.clone();
        roles.sort_by_key(|r| std::cmp::Reverse(r.position));
        menu::submenu(ui, t!("Roles"), |ui| {
            for role in roles.iter().filter(|r| r.id != guild_id && !r.managed) {
                let has = member_roles.contains(&role.id);
                if menu::item(ui, &role.name, menu::Tail::Check(has)).clicked() {
                    let method = if has { "DELETE" } else { "PUT" };
                    app.commands.send(admin::change(method, format!("/guilds/{guild_id}/members/{u}/roles/{}", role.id), None, &format!("members:{}", t!("Roles updated."))));
                }
            }
        });
    }
    if perms & (1 << 40) != 0 {
        menu::separator(ui);
        if timed_out {
            if menu::item(ui, t!("Remove Timeout"), menu::Tail::None).clicked() {
                app.commands.send(admin::change("PATCH", format!("/guilds/{guild_id}/members/{u}"), Some(json!({ "communication_disabled_until": null })), &format!("members:{}", t!("Timeout removed."))));
                ui.close();
            }
        } else {
            menu::submenu(ui, &t!("Timeout {name}", name = user.display_name()), |ui| {
                for (label, seconds) in menu::TIMEOUTS {
                    if menu::item(ui, crate::i18n::tr(label), menu::Tail::None).clicked() {
                        let until = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(seconds);
                        app.commands.send(admin::change("PATCH", format!("/guilds/{guild_id}/members/{u}"), Some(json!({ "communication_disabled_until": until.to_string() })), &format!("members:{}", t!("Member timed out."))));
                        ui.close();
                    }
                }
            });
        }
    }
    let name = user.display_name();
    if perms & (1 << 1) != 0 && menu::danger(ui, &t!("Kick {name}", name = name), menu::Tail::None).clicked() {
        app.modal = Some(crate::app::Modal::Confirm {
            title: t!("Kick '{name}' from Server", name = name),
            body: t!("Are you sure you want to kick @{name} from the server? They will be able to rejoin with another invite.", name = name),
            button: t!("Kick").into(),
            then: crate::app::Confirmed::Kick { guild: guild_id, user: u },
        });
        ui.close();
    }
    if perms & (1 << 2) != 0 && menu::danger(ui, &t!("Ban {name}", name = name), menu::Tail::None).clicked() {
        app.modal = Some(crate::app::Modal::Confirm {
            title: t!("Ban '{name}'", name = name),
            body: t!("Are you sure you want to ban @{name}? They won't be able to return to the server until they are unbanned.", name = name),
            button: t!("Ban").into(),
            then: crate::app::Confirmed::Ban { guild: guild_id, user: u },
        });
        ui.close();
    }
    menu::separator(ui);
    menu::copy_id(ui, t!("Copy User ID"), u);
}

// ---- invites, bans, emoji, audit ------------------------------------------------------

fn invites(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    let p = theme::p();
    form::title(ui, t!("Invites"));
    form::note(ui, t!("Here's a list of all active invite links. You can revoke any one."));
    ui.add_space(16.0);
    let first_text = app.guild(guild_id).and_then(|g| sorted(&g.channels).into_iter().find(|c| c.kind == ChannelKind::Text).map(|c| c.id));
    if let Some(channel) = first_text
        && form::button(ui, t!("Create Invite"), Kind::Primary).on_hover_text(t!("Valid for 7 days, with no limit on uses")).clicked()
    {
        app.commands.send(admin::change("POST", format!("/channels/{channel}/invites"), Some(json!({ "max_age": 604800, "max_uses": 0 })), &format!("invites:{}", t!("Invite created."))));
    }
    ui.add_space(20.0);
    let Some(list) = app.admin.as_ref().and_then(|a| a.invites.clone()) else {
        ui.spinner();
        return;
    };
    let width = ui.available_width();
    let (code_x, uses_x, expires_x) = (width * 0.38, width * 0.62, width * 0.76);
    table_header(ui, &[(t!("Inviter"), 12.0), (t!("Invite Code"), code_x), (t!("Uses"), uses_x), (t!("Expires"), expires_x)]);
    if list.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(RichText::new(t!("No active invites.")).color(p.muted)));
    }
    for invite in &list {
        let code = str_of(invite, "code").to_owned();
        let inviter = invite.get("inviter").and_then(User::parse);
        let channel = invite.get("channel").map(|c| str_of(c, "name").to_owned()).unwrap_or_default();
        let uses = invite.get("uses").and_then(Value::as_u64).unwrap_or(0);
        let max_uses = invite.get("max_uses").and_then(Value::as_u64).unwrap_or(0);
        let expires = invite
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|t| t.parse::<jiff::Timestamp>().ok())
            .map_or_else(|| t!("Never").to_owned(), |t| countdown(t.as_millisecond() as u64));
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
        if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
        let avatar = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 14.0), Vec2::splat(28.0));
        let name = inviter.as_ref().map_or_else(|| t!("Someone").to_owned(), |u| u.display_name().to_owned());
        app.images.circle(ui, avatar, inviter.as_ref().map(User::avatar_url).as_deref(), &super::initials(&name), 14.0);
        super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 10.0, rect.center().y - 18.0), pos2(rect.left() + code_x - 12.0, rect.center().y)), &name, theme::strong(15.0), p.strong);
        super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 10.0, rect.center().y), pos2(rect.left() + code_x - 12.0, rect.center().y + 18.0)), &format!("#{channel}"), FontId::proportional(13.0), p.muted);
        ui.painter().text(pos2(rect.left() + code_x, rect.center().y), egui::Align2::LEFT_CENTER, &code, FontId::monospace(14.0), p.text);
        let uses_text = if max_uses > 0 { format!("{uses}/{max_uses}") } else { uses.to_string() };
        ui.painter().text(pos2(rect.left() + uses_x, rect.center().y), egui::Align2::LEFT_CENTER, uses_text, FontId::proportional(14.0), p.text);
        ui.painter().text(pos2(rect.left() + expires_x, rect.center().y), egui::Align2::LEFT_CENTER, expires, FontId::monospace(13.5), p.text);
        // The code copies its link on a click.
        let code_rect = Rect::from_min_max(pos2(rect.left() + code_x, rect.center().y - 12.0), pos2(rect.left() + uses_x - 12.0, rect.center().y + 12.0));
        let copy = ui.interact(code_rect, ui.id().with(("copy", &code)), Sense::click());
        if copy.on_hover_text(t!("Copy Link")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ui.ctx().copy_text(format!("https://discord.gg/{code}"));
            app.notify(t!("Link copied"));
        }
        // Revoke on hover, as Discord's row.
        if response.hovered() || ui.rect_contains_pointer(rect) {
            let revoke = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), Vec2::splat(28.0));
            let r = ui.interact(revoke, ui.id().with(("revoke", &code)), Sense::click());
            ui.painter().circle_filled(revoke.center(), 14.0, if r.hovered() { p.red } else { p.selected });
            icons::paint(ui, revoke.shrink(7.0), Icon::Close, Color32::WHITE);
            if r.on_hover_text(t!("Revoke Invite")).clicked() {
                app.commands.send(admin::change("DELETE", format!("/invites/{code}"), None, &format!("invites:{}", t!("Invite revoked."))));
            }
        }
    }
}

/// How long until an invite expires: "6d 23h", "4h 12min", "8min".
fn countdown(until_ms: u64) -> String {
    let left = until_ms.saturating_sub(crate::app::now_ms()) / 1000;
    match left {
        0 => t!("Expired").into(),
        1..3600 => format!("{}min", left.div_ceil(60)),
        3600..86_400 => format!("{}h {}min", left / 3600, left / 60 % 60),
        _ => format!("{}d {}h", left / 86_400, left / 3600 % 24),
    }
}

fn bans(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    let p = theme::p();
    let count = app.admin.as_ref().and_then(|a| a.bans.as_ref()).map_or(0, Vec::len);
    form::title(ui, &t!("Bans — {count}", count = count));
    form::note(ui, t!("Bans by default are by account and IP. A user can circumvent an IP ban by using a proxy."));
    ui.add_space(16.0);
    if let Some(admin) = app.admin.as_mut() {
        let width = ui.available_width();
        form::input(ui, &mut admin.ban_query, t!("Search bans by username or ID"), width);
    }
    ui.add_space(16.0);
    let Some(list) = app.admin.as_ref().and_then(|a| a.bans.clone()) else {
        ui.spinner();
        return;
    };
    let needle = app.admin.as_ref().map(|a| a.ban_query.trim().to_lowercase()).unwrap_or_default();
    let shown: Vec<&Value> = list
        .iter()
        .filter(|b| {
            needle.is_empty()
                || b.get("user").is_some_and(|u| str_of(u, "username").to_lowercase().contains(&needle) || str_of(u, "id") == needle)
        })
        .collect();
    if shown.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(if list.is_empty() { t!("No bans") } else { t!("No bans with that name") }).font(theme::strong(16.0)).color(p.strong));
            if list.is_empty() {
                ui.label(RichText::new(t!("Banned people show up here.")).size(14.0).color(p.muted));
            }
        });
    }
    for ban in shown {
        let Some(user) = ban.get("user").and_then(User::parse) else { continue };
        let reason = str_of(ban, "reason").to_owned();
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        let _ = response;
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
        let avatar = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 16.0), Vec2::splat(32.0));
        app.images.circle(ui, avatar, Some(&user.avatar_url()), &super::initials(user.display_name()), 16.0);
        let right = rect.right() - if hovered { 170.0 } else { 12.0 };
        super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 10.0, rect.center().y - 18.0), pos2(right, rect.center().y)), user.display_name(), theme::strong(15.0), p.strong);
        let sub = if reason.is_empty() { user.username.clone() } else { format!("{} • {}", user.username, reason) };
        super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 10.0, rect.center().y), pos2(right, rect.center().y + 18.0)), &sub, FontId::proportional(13.0), p.muted);
        if hovered {
            let button = Rect::from_min_size(pos2(rect.right() - 160.0, rect.center().y - 16.0), vec2(150.0, 32.0));
            let r = ui.interact(button, ui.id().with(("unban", user.id)), Sense::click());
            ui.painter().rect(button, 8.0, if r.hovered() { p.red } else { Color32::TRANSPARENT }, Stroke::new(1.0, p.red), egui::StrokeKind::Inside);
            ui.painter().text(button.center(), egui::Align2::CENTER_CENTER, t!("Revoke Ban"), theme::strong(13.0), if r.hovered() { Color32::WHITE } else { p.red });
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.commands.send(admin::change("DELETE", format!("/guilds/{guild_id}/bans/{}", user.id), None, &format!("bans:{}", t!("Ban revoked."))));
            }
        }
    }
}

fn emojis(app: &mut App, ui: &mut egui::Ui, guild_id: Id) {
    let p = theme::p();
    form::title(ui, t!("Emoji"));
    form::note(ui, t!("Add up to 50 custom emoji that anyone can use in this server. Animated GIF emoji may be used by members with Discord Nitro."));
    ui.add_space(14.0);
    form::label(ui, t!("Upload Requirements"));
    for line in [t!("File type: JPEG, PNG, GIF, WEBP"), t!("Max file size: 256 KB"), t!("Recommended dimensions: 128x128"), t!("Naming: Emoji names must be at least 2 characters long and can only contain alphanumeric characters and underscores")] {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 2.0, p.muted);
            ui.label(RichText::new(line).size(14.0).color(p.muted));
        });
    }
    ui.add_space(16.0);
    if form::button(ui, t!("Upload Emoji"), Kind::Primary).clicked()
        && let Some(file) = app.settings.file_dialog().add_filter(t!("Image"), &["png", "jpg", "jpeg", "gif", "webp"]).pick_file()
    {
        app.settings.picked(&file);
        let name: String = file
            .file_stem()
            .map(|n| n.to_string_lossy().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect())
            .unwrap_or_default();
        match admin::image_data(&file) {
            Some(data) => app.commands.send(admin::change("POST", format!("/guilds/{guild_id}/emojis"), Some(json!({ "name": if name.len() >= 2 { name } else { "emoji".into() }, "image": data })), &format!("emojis:{}", t!("Emoji uploaded.")))),
            None => app.notify(t!("Invalid image, or too large.")),
        }
    }
    form::divider(ui);
    let Some(list) = app.admin.as_ref().and_then(|a| a.emojis.clone()) else {
        ui.spinner();
        return;
    };
    // Slots by boost level, as Discord counts them (static and animated apart).
    let tier = app.admin.as_ref().and_then(|a| a.loaded.as_ref()).and_then(|l| l.get("premium_tier")).and_then(Value::as_u64).unwrap_or(0);
    let limit: usize = match tier {
        1 => 100,
        2 => 150,
        3 => 250,
        _ => 50,
    };
    let animated = list.iter().filter(|e| e.get("animated").and_then(Value::as_bool).unwrap_or(false)).count();
    let still = list.len() - animated;
    form::label(ui, &tn!(limit.saturating_sub(still), "Emoji — {count} slot available", "Emoji — {count} slots available"));
    if animated > 0 {
        form::note(ui, &t!("Animated: {animated} of {limit}", animated = animated, limit = limit));
        ui.add_space(8.0);
    }
    let width = ui.available_width();
    let (name_x, uploader_x) = (72.0, width * 0.5);
    table_header(ui, &[(t!("Image"), 12.0), (t!("Name"), name_x), (t!("Uploaded By"), uploader_x)]);
    if list.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("No emoji yet")).font(theme::strong(16.0)).color(p.strong));
            ui.label(RichText::new(t!("Upload the first one and it shows up here.")).size(14.0).color(p.muted));
        });
    }
    for emoji in &list {
        let id = emoji.get("id").and_then(sf).unwrap_or(0);
        let name = str_of(emoji, "name").to_owned();
        let animated = emoji.get("animated").and_then(Value::as_bool).unwrap_or(false);
        let uploader = emoji.get("user").and_then(User::parse);
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        let _ = response;
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
        let icon = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 16.0), Vec2::splat(32.0));
        let url = format!("https://cdn.discordapp.com/emojis/{id}.{}?size=64", if animated { "gif" } else { "png" });
        if let Some((texture, _)) = app.images.get(&url, 64) {
            egui::Image::from_texture((texture, icon.size())).paint_at(ui, icon);
        }
        // The name, renamed in place as in Discord: click, type, Enter.
        let name_rect = Rect::from_min_max(pos2(rect.left() + name_x, rect.center().y - 16.0), pos2(rect.left() + uploader_x - 16.0, rect.center().y + 16.0));
        let editing = app.admin.as_ref().and_then(|a| a.emoji_edit.as_ref()).is_some_and(|(e, _)| *e == id);
        if editing {
            let mut done = None;
            if let Some((_, text)) = app.admin.as_mut().and_then(|a| a.emoji_edit.as_mut()) {
                let response = ui.put(name_rect, egui::TextEdit::singleline(text).font(FontId::proportional(15.0)).char_limit(32));
                response.request_focus();
                if response.lost_focus() {
                    done = Some((ui.input(|i| i.key_pressed(Key::Enter)), text.clone()));
                }
            }
            if let Some((save, text)) = done {
                if save && text.len() >= 2 && text != name {
                    app.commands.send(admin::change("PATCH", format!("/guilds/{guild_id}/emojis/{id}"), Some(json!({ "name": text })), &format!("emojis:{}", t!("Emoji renamed."))));
                }
                if let Some(admin) = app.admin.as_mut() {
                    admin.emoji_edit = None;
                }
            }
        } else {
            let label = ui.interact(name_rect, ui.id().with(("emoji-name", id)), Sense::click());
            if label.hovered() {
                ui.painter().rect_stroke(name_rect, 4.0, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
            }
            super::paint_truncated(ui, name_rect.shrink2(vec2(6.0, 0.0)), &format!(":{name}:"), FontId::proportional(15.0), p.strong);
            if label.on_hover_text(t!("Click to rename")).clicked()
                && let Some(admin) = app.admin.as_mut()
            {
                admin.emoji_edit = Some((id, name.clone()));
            }
        }
        if let Some(user) = &uploader {
            let avatar = Rect::from_min_size(pos2(rect.left() + uploader_x, rect.center().y - 12.0), Vec2::splat(24.0));
            app.images.circle(ui, avatar, Some(&user.avatar_url()), &super::initials(user.display_name()), 12.0);
            super::paint_truncated(ui, Rect::from_min_max(pos2(avatar.right() + 8.0, rect.top()), pos2(rect.right() - 48.0, rect.bottom())), user.display_name(), FontId::proportional(14.0), p.text);
        }
        if hovered {
            let delete = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), Vec2::splat(28.0));
            let r = ui.interact(delete, ui.id().with(("emoji-delete", id)), Sense::click());
            ui.painter().circle_filled(delete.center(), 14.0, if r.hovered() { p.red } else { p.selected });
            icons::paint(ui, delete.shrink(7.0), Icon::Close, Color32::WHITE);
            if r.on_hover_text(t!("Delete Emoji")).clicked() {
                app.commands.send(admin::change("DELETE", format!("/guilds/{guild_id}/emojis/{id}"), None, &format!("emojis:{}", t!("Emoji deleted."))));
            }
        }
    }
}

/// The kind of an audit log entry: its icon and colour (green for what
/// was created, red for what was removed, yellow for changes), as
/// Discord's log marks them.
fn audit_mark(code: u64) -> (Icon, Color32) {
    let p = theme::p();
    let icon = match code {
        10..=15 => Icon::Text,
        20..=28 => Icon::Members,
        30..=32 => Icon::Members,
        40..=42 => Icon::UserPlus,
        60..=62 | 90..=92 => Icon::Reaction,
        72..=75 => Icon::Chat,
        110..=112 => Icon::Thread,
        _ => Icon::Settings,
    };
    let color = match code {
        10 | 13 | 28 | 30 | 40 | 50 | 60 | 80 | 90 | 110 | 140 => p.green,
        12 | 15 | 20 | 21 | 22 | 32 | 42 | 52 | 62 | 72 | 73 | 82 | 92 | 112 | 142 => p.red,
        _ => p.warning,
    };
    (icon, color)
}

fn audit(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    form::title(ui, t!("Audit Log"));
    let Some(log) = app.admin.as_ref().and_then(|a| a.audit.clone()) else {
        ui.spinner();
        return;
    };
    let users: std::collections::HashMap<Id, User> = log
        .get("users")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(|u| User::parse(u).map(|u| (u.id, u))).collect())
        .unwrap_or_default();
    let entries: Vec<Value> = log.get("audit_log_entries").and_then(Value::as_array).cloned().unwrap_or_default();
    if entries.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("No logs yet")).font(theme::strong(16.0)).color(p.strong));
            ui.label(RichText::new(t!("Changes made to the server show up here.")).size(14.0).color(p.muted));
        });
        return;
    }
    for entry in &entries {
        let code = entry.get("action_type").and_then(Value::as_u64).unwrap_or(0);
        let who = entry.get("user_id").and_then(sf).and_then(|u| users.get(&u)).cloned();
        let target = entry.get("target_id").and_then(sf).and_then(|t| users.get(&t)).map(|u| u.display_name().to_owned());
        let when = entry.get("id").and_then(sf).map(|id| crate::text::time(crate::discord::model::snowflake_ms(id))).unwrap_or_default();
        let reason = str_of(entry, "reason").to_owned();
        Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, color) = audit_mark(code);
                let (mark, _) = ui.allocate_exact_size(vec2(20.0, 40.0), Sense::hover());
                icons::paint(ui, Rect::from_center_size(mark.center(), Vec2::splat(18.0)), icon, color);
                let (avatar, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                let name = who.as_ref().map_or_else(|| t!("Someone").to_owned(), |u| u.display_name().to_owned());
                app.images.circle(ui, avatar, who.as_ref().map(User::avatar_url).as_deref(), &super::initials(&name), 20.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    // "Ana banned Leo", the names in bold where the language
                    // puts them.
                    let mut job = egui::text::LayoutJob::default();
                    let plain = egui::TextFormat::simple(FontId::proportional(15.0), p.text);
                    let bold = egui::TextFormat::simple(theme::strong(15.0), p.strong);
                    let mut rest = admin::audit_action(code);
                    while let Some(start) = rest.find('{') {
                        let end = rest[start..].find('}').map_or(rest.len(), |e| start + e + 1);
                        job.append(&rest[..start], 0.0, plain.clone());
                        match &rest[start..end] {
                            "{user}" => job.append(&name, 0.0, bold.clone()),
                            "{target}" => job.append(target.as_deref().unwrap_or(t!("someone")), 0.0, bold.clone()),
                            other => job.append(other, 0.0, plain.clone()),
                        }
                        rest = &rest[end..];
                    }
                    job.append(rest, 0.0, plain);
                    job.wrap.max_width = ui.available_width();
                    ui.label(job);
                    let mut sub = when.clone();
                    if !reason.is_empty() {
                        sub.push_str(&format!(" • {}", t!("Reason: {reason}", reason = reason)));
                    }
                    ui.label(RichText::new(sub).size(13.0).color(p.muted));
                });
            });
        });
        ui.add_space(8.0);
    }
}
