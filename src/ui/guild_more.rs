//! The rest of Discord's server settings: stickers, soundboard, webhooks,
//! integrations, AutoMod, the vanity URL, the widget, templates, pruning,
//! deleting the server, and each channel's permission overwrites. Pages read
//! what they need through `gates::fetch` and change it with the key
//! "gate:changed", which makes them ask again.

use egui::{Color32, Frame, Margin, RichText, Vec2};
use serde_json::{Value, json};

use super::form::{self, Kind};
use super::gates::fetch;
use crate::admin::Tab;
use crate::app::App;
use crate::backend::Command;
use crate::discord::model::{ChannelKind, Id};
use crate::theme;

fn change(app: &mut App, method: &'static str, path: String, body: Option<Value>) {
    app.commands.send(Command::Api { key: "gate:changed".into(), method, path, body });
}

/// What the page's call answered: its value once it is here.
fn page(app: &mut App, ui: &mut egui::Ui, key: &str, path: String) -> Option<Value> {
    let answer = fetch(app, &format!("gate:admin:{key}"), || path).cloned();
    match answer {
        Some(Ok(value)) => Some(value),
        Some(Err(error)) => {
            ui.label(RichText::new(error).color(theme::p().red));
            None
        }
        None => {
            ui.spinner();
            None
        }
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// A row: a name, a detail and a button at the right, true when clicked.
fn row(ui: &mut egui::Ui, name: &str, detail: &str, button: &str, kind: Kind) -> bool {
    let p = theme::p();
    let mut clicked = false;
    Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(name).size(15.0).color(p.strong));
                if !detail.is_empty() {
                    ui.label(RichText::new(detail).size(12.5).color(p.muted));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = form::button(ui, button, kind).clicked();
            });
        });
    });
    ui.add_space(6.0);
    clicked
}

/// The small text fields a page keeps between frames.
fn field(ui: &mut egui::Ui, id: &str, hint: &str, width: f32) -> String {
    let key = egui::Id::new(("guild-more", id.to_owned()));
    let mut value: String = ui.data(|d| d.get_temp(key)).unwrap_or_default();
    form::input(ui, &mut value, hint, width);
    ui.data_mut(|d| d.insert_temp(key, value.clone()));
    value
}

fn clear(ui: &mut egui::Ui, id: &str) {
    ui.data_mut(|d| d.remove::<String>(egui::Id::new(("guild-more", id.to_owned()))));
}

pub fn show(app: &mut App, ui: &mut egui::Ui, guild: Id, tab: Tab) {
    match tab {
        Tab::Stickers => stickers(app, ui, guild),
        Tab::Soundboard => soundboard(app, ui, guild),
        Tab::Webhooks => webhooks(app, ui, guild),
        Tab::Integrations => integrations(app, ui, guild),
        Tab::AutoMod => automod(app, ui, guild),
        Tab::Access => access(app, ui, guild),
        Tab::Templates => templates(app, ui, guild),
        Tab::Prune => prune(app, ui, guild),
        Tab::Delete => delete(app, ui, guild),
        _ => {}
    }
}

fn stickers(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Stickers"));
    ui.horizontal(|ui| {
        let name = field(ui, "sticker-name", t!("Name"), 180.0);
        let tag = field(ui, "sticker-tag", t!("Emoji"), 100.0);
        if !name.trim().is_empty()
            && !tag.trim().is_empty()
            && form::button(ui, t!("Upload Sticker"), Kind::Primary).clicked()
            && let Some(file) = app.settings.file_dialog().add_filter(t!("Image"), &["png", "apng", "gif", "json"]).pick_file()
        {
            app.settings.picked(&file);
            app.commands.send(Command::UploadSticker { guild, name: name.trim().to_owned(), tags: tag.trim().to_owned(), path: file });
            clear(ui, "sticker-name");
            clear(ui, "sticker-tag");
        }
    });
    ui.add_space(12.0);
    let Some(list) = page(app, ui, "stickers", format!("/guilds/{guild}/stickers")) else { return };
    for sticker in list.as_array().into_iter().flatten() {
        if row(ui, text(sticker, "name"), text(sticker, "tags"), t!("Delete"), Kind::Danger) {
            change(app, "DELETE", format!("/guilds/{guild}/stickers/{}", text(sticker, "id")), None);
        }
    }
}

fn soundboard(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Soundboard"));
    ui.horizontal(|ui| {
        let name = field(ui, "sound-name", t!("Name"), 200.0);
        if !name.trim().is_empty()
            && form::button(ui, t!("Upload Sound"), Kind::Primary).clicked()
            && let Some(file) = app.settings.file_dialog().add_filter(t!("Sound"), &["mp3", "ogg"]).pick_file()
        {
            app.settings.picked(&file);
            // 512 KB and about five seconds at most, as Discord allows.
            match std::fs::read(&file) {
                Ok(bytes) if bytes.len() <= 512 * 1024 => {
                    use base64::Engine;
                    let kind = if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("ogg")) { "audio/ogg" } else { "audio/mpeg" };
                    let data = format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
                    change(app, "POST", format!("/guilds/{guild}/soundboard-sounds"), Some(json!({ "name": name.trim(), "sound": data, "volume": 1 })));
                    clear(ui, "sound-name");
                }
                _ => app.notify(t!("Invalid sound, or larger than 512 KB.")),
            }
        }
    });
    ui.add_space(12.0);
    let Some(value) = page(app, ui, "sounds", format!("/guilds/{guild}/soundboard-sounds")) else { return };
    for sound in value.get("items").and_then(Value::as_array).into_iter().flatten() {
        if row(ui, text(sound, "name"), text(sound, "emoji_name"), t!("Delete"), Kind::Danger) {
            change(app, "DELETE", format!("/guilds/{guild}/soundboard-sounds/{}", text(sound, "sound_id")), None);
        }
    }
}

fn webhooks(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Webhooks"));
    let channels: Vec<(Id, String)> = app.guild(guild).map_or_else(Vec::new, |g| g.channels.iter().filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement)).map(|c| (c.id, c.title())).collect());
    let names: Vec<String> = channels.iter().map(|(_, name)| format!("#{name}")).collect();
    form::label(ui, t!("New Webhook"));
    if let Some(picked) = form::select(ui, "new-webhook", t!("Channel"), &names, 280.0) {
        change(app, "POST", format!("/channels/{}/webhooks", channels[picked].0), Some(json!({ "name": "Webhook" })));
    }
    ui.add_space(12.0);
    let Some(list) = page(app, ui, "webhooks", format!("/guilds/{guild}/webhooks")) else { return };
    for hook in list.as_array().into_iter().flatten() {
        let id = text(hook, "id");
        let where_to = hook.get("channel_id").and_then(Value::as_str).and_then(|c| c.parse().ok()).and_then(|c: Id| app.channel(c)).map(|c| format!("#{}", c.title())).unwrap_or_default();
        if row(ui, text(hook, "name"), &where_to, t!("Delete"), Kind::Danger) {
            change(app, "DELETE", format!("/webhooks/{id}"), None);
        }
        if let Some(token) = hook.get("token").and_then(Value::as_str)
            && form::link(ui, t!("Copy Webhook URL"))
        {
            ui.ctx().copy_text(format!("https://discord.com/api/webhooks/{id}/{token}"));
        }
    }
}

fn integrations(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Integrations"));
    let Some(list) = page(app, ui, "integrations", format!("/guilds/{guild}/integrations?include_applications=true")) else { return };
    for item in list.as_array().into_iter().flatten() {
        if row(ui, text(item, "name"), text(item, "type"), t!("Remove"), Kind::Danger) {
            change(app, "DELETE", format!("/guilds/{guild}/integrations/{}", text(item, "id")), None);
        }
    }
}

/// AutoMod: the server's rules with their switches, and new keyword rules
/// that block a message (trigger 1, action 1).
fn automod(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("AutoMod"));
    form::label(ui, t!("Block Custom Words"));
    ui.horizontal(|ui| {
        let name = field(ui, "rule-name", t!("Name"), 180.0);
        let words = field(ui, "rule-words", t!("Words, separated by commas"), 300.0);
        if !name.trim().is_empty() && !words.trim().is_empty() && form::button(ui, t!("Create"), Kind::Primary).clicked() {
            let list: Vec<&str> = words.split(',').map(str::trim).filter(|word| !word.is_empty()).collect();
            change(app, "POST", format!("/guilds/{guild}/auto-moderation/rules"), Some(json!({
                "name": name.trim(), "event_type": 1, "trigger_type": 1, "trigger_metadata": { "keyword_filter": list }, "actions": [{ "type": 1 }], "enabled": true,
            })));
            clear(ui, "rule-name");
            clear(ui, "rule-words");
        }
    });
    ui.add_space(12.0);
    let Some(list) = page(app, ui, "automod", format!("/guilds/{guild}/auto-moderation/rules")) else { return };
    for rule in list.as_array().into_iter().flatten() {
        let id = text(rule, "id");
        let mut on = rule.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        if form::switch_row(ui, &mut on, text(rule, "name"), "") {
            change(app, "PATCH", format!("/guilds/{guild}/auto-moderation/rules/{id}"), Some(json!({ "enabled": on })));
        }
        if form::link(ui, t!("Delete")) {
            change(app, "DELETE", format!("/guilds/{guild}/auto-moderation/rules/{id}"), None);
        }
        ui.add_space(6.0);
    }
}

/// Access: the vanity URL and the widget.
fn access(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Access"));
    form::label(ui, t!("Custom Invite Link"));
    if let Some(vanity) = page(app, ui, "vanity", format!("/guilds/{guild}/vanity-url")) {
        let current = text(&vanity, "code").to_owned();
        ui.horizontal(|ui| {
            let typed = field(ui, "vanity", &current, 240.0);
            if !typed.trim().is_empty() && form::button(ui, t!("Save"), Kind::Primary).clicked() {
                change(app, "PATCH", format!("/guilds/{guild}/vanity-url"), Some(json!({ "code": typed.trim() })));
                clear(ui, "vanity");
            }
        });
    }
    ui.add_space(16.0);
    if let Some(widget) = page(app, ui, "widget", format!("/guilds/{guild}/widget")) {
        let mut on = widget.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        if form::switch_row(ui, &mut on, t!("Enable Server Widget"), "") {
            change(app, "PATCH", format!("/guilds/{guild}/widget"), Some(json!({ "enabled": on })));
        }
    }
}

fn templates(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Server Template"));
    ui.horizontal(|ui| {
        let name = field(ui, "template", t!("Name"), 240.0);
        if !name.trim().is_empty() && form::button(ui, t!("Create"), Kind::Primary).clicked() {
            change(app, "POST", format!("/guilds/{guild}/templates"), Some(json!({ "name": name.trim() })));
            clear(ui, "template");
        }
    });
    ui.add_space(12.0);
    let Some(list) = page(app, ui, "templates", format!("/guilds/{guild}/templates")) else { return };
    for template in list.as_array().into_iter().flatten() {
        let code = text(template, "code");
        if row(ui, text(template, "name"), code, t!("Delete"), Kind::Danger) {
            change(app, "DELETE", format!("/guilds/{guild}/templates/{code}"), None);
        }
        ui.horizontal(|ui| {
            if form::link(ui, t!("Sync")) {
                change(app, "PUT", format!("/guilds/{guild}/templates/{code}"), None);
            }
            if form::link(ui, t!("Copy Link")) {
                ui.ctx().copy_text(format!("https://discord.new/{code}"));
            }
        });
    }
}

fn prune(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    form::title(ui, t!("Prune Members"));
    const DAYS: [u64; 4] = [1, 7, 30, 90];
    let key = egui::Id::new("prune-days");
    let mut days: u64 = ui.data(|d| d.get_temp(key)).unwrap_or(7);
    let label = |days: u64| tn!(days, "{count} day", "{count} days");
    let names: Vec<String> = DAYS.iter().map(|d| label(*d)).collect();
    form::label(ui, t!("Last Seen"));
    if let Some(picked) = form::select(ui, "prune-days", &label(days), &names, 200.0) {
        days = DAYS[picked];
        ui.data_mut(|d| d.insert_temp(key, days));
        app.gate_asked.remove("gate:admin:prune");
    }
    ui.add_space(12.0);
    let Some(count) = page(app, ui, "prune", format!("/guilds/{guild}/prune?days={days}")) else { return };
    let pruned = count.get("pruned").and_then(Value::as_u64).unwrap_or(0);
    ui.label(RichText::new(tn!(pruned, "{count} member", "{count} members")).color(theme::p().strong));
    ui.add_space(8.0);
    if pruned > 0 && form::button(ui, t!("Prune"), Kind::Danger).clicked() {
        change(app, "POST", format!("/guilds/{guild}/prune"), Some(json!({ "days": days, "compute_prune_count": false })));
    }
}

/// Deleting: only the owner, after typing the server's name.
fn delete(app: &mut App, ui: &mut egui::Ui, guild: Id) {
    let p = theme::p();
    form::title(ui, t!("Delete Server"));
    let Some(found) = app.guild(guild) else { return };
    if found.owner_id != Some(app.me_id()) {
        ui.label(RichText::new(t!("Only the server's owner can do this.")).color(p.muted));
        return;
    }
    let name = found.name.clone();
    ui.label(RichText::new(t!("Are you sure you want to delete {name}? This action cannot be undone.", name = name)).color(p.red));
    ui.add_space(10.0);
    form::label(ui, t!("Enter Server Name"));
    let typed = field(ui, "delete-name", &name, 320.0);
    ui.add_space(10.0);
    if typed == name && form::button(ui, t!("Delete Server"), Kind::Danger).clicked() {
        clear(ui, "delete-name");
        change(app, "POST", format!("/guilds/{guild}/delete"), Some(json!({})));
        app.actions.push(crate::app::Action::CloseGuildSettings);
    }
}

/// The permissions a channel's overwrites offer, by kind of channel.
const TEXT_PERMISSIONS: &[(u64, &str)] = &[
    (10, n!("View Channel")),
    (0, n!("Create Invite")),
    (4, n!("Manage Channel")),
    (28, n!("Manage Permissions")),
    (29, n!("Manage Webhooks")),
    (11, n!("Send Messages")),
    (38, n!("Send Messages in Threads")),
    (35, n!("Create Public Threads")),
    (36, n!("Create Private Threads")),
    (14, n!("Embed Links")),
    (15, n!("Attach Files")),
    (6, n!("Add Reactions")),
    (18, n!("Use External Emoji")),
    (17, n!("Mention @everyone, @here and All Roles")),
    (13, n!("Manage Messages")),
    (51, n!("Pin Messages")),
    (34, n!("Manage Threads")),
    (16, n!("Read Message History")),
    (49, n!("Create Polls")),
];
const VOICE_PERMISSIONS: &[(u64, &str)] = &[
    (10, n!("View Channel")),
    (0, n!("Create Invite")),
    (4, n!("Manage Channel")),
    (28, n!("Manage Permissions")),
    (20, n!("Connect")),
    (21, n!("Speak")),
    (9, n!("Video")),
    (42, n!("Use Soundboard")),
    (45, n!("Use External Sounds")),
    (25, n!("Use Voice Activity")),
    (8, n!("Priority Speaker")),
    (22, n!("Mute Members")),
    (23, n!("Deafen Members")),
    (24, n!("Move Members")),
    (11, n!("Send Messages")),
];

/// The "Permissions" dialog: who has overwrites here, and for the one picked
/// each permission allowed, denied or passed on (✓ / ✕ / /).
pub fn overwrites(app: &mut App, ui: &mut egui::Ui, channel: Id, picked: &mut Option<Id>) -> bool {
    let p = theme::p();
    let Some(found) = app.channel(channel).cloned() else { return false };
    let Some(guild) = found.guild_id.and_then(|g| app.guild(g)).cloned() else { return false };
    ui.label(RichText::new(t!("Permissions")).font(theme::strong(20.0)).color(p.strong));
    ui.label(RichText::new(format!("#{}", found.title())).size(13.0).color(p.muted));
    ui.add_space(8.0);
    let everyone = guild.id;
    let selected = picked.unwrap_or(everyone);
    let names: Vec<String> = guild.roles.iter().map(|role| format!("@{}", role.name.trim_start_matches('@'))).collect();
    let shown = guild.roles.iter().find(|role| role.id == selected).map_or_else(|| selected.to_string(), |role| format!("@{}", role.name.trim_start_matches('@')));
    if let Some(i) = form::select(ui, "overwrite-role", &shown, &names, 300.0) {
        *picked = Some(guild.roles[i].id);
    }
    ui.add_space(8.0);
    let (allow, deny) = found.overwrites.iter().find(|o| o.id == selected).map_or((0, 0), |o| (o.allow, o.deny));
    let list = if matches!(found.kind, ChannelKind::Voice | ChannelKind::Stage) { VOICE_PERMISSIONS } else { TEXT_PERMISSIONS };
    let mut change_to = None;
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for (bit, label) in list {
            let mask = 1u64 << bit;
            ui.horizontal(|ui| {
                ui.set_min_width(380.0);
                ui.label(RichText::new(crate::i18n::tr(label)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let tri = |ui: &mut egui::Ui, mark: &str, on: bool, color: Color32| {
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(28.0, 24.0), egui::Sense::click());
                        ui.painter().rect_filled(rect, 4.0, if on { color } else { p.hover });
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, mark, theme::strong(13.0), if on { Color32::WHITE } else { p.muted });
                        response.clicked()
                    };
                    if tri(ui, "✓", allow & mask != 0, p.green) {
                        change_to = Some((allow | mask, deny & !mask));
                    }
                    if tri(ui, "/", allow & mask == 0 && deny & mask == 0, p.selected) {
                        change_to = Some((allow & !mask, deny & !mask));
                    }
                    if tri(ui, "✕", deny & mask != 0, p.red) {
                        change_to = Some((allow & !mask, deny | mask));
                    }
                });
            });
        }
    });
    if let Some((allow, deny)) = change_to {
        // Role overwrites are type 0; the gateway brings the channel back changed.
        app.commands.send(Command::Api { key: "gate:changed".into(), method: "PUT", path: format!("/channels/{channel}/permissions/{selected}"), body: Some(json!({ "type": 0, "allow": allow.to_string(), "deny": deny.to_string() })) });
    }
    ui.add_space(10.0);
    !ui.button(t!("Close")).clicked()
}
