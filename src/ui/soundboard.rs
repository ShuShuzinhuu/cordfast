//! Discord's soundboard: the row of buttons in the call panel and the popout
//! with every sound the account can reach. Sending goes over REST and each
//! client plays the file itself, so the voice connection does not change.

use std::collections::{HashMap, HashSet};

use egui::{Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};

use super::form;
use crate::app::App;
use crate::backend::Command;
use crate::discord::model::{Id, SoundboardSound};
use crate::icons::{self, Icon};
use crate::theme;

const USE_SOUNDBOARD: u64 = 1 << 42;
const USE_EXTERNAL_SOUNDS: u64 = 1 << 45;
const SPEAK: u64 = 1 << 21;
/// Discord keeps the default sounds under the server "0".
const DEFAULTS: Id = 0;
const TILES: usize = 3;
const TILE: Vec2 = vec2(136.0, 44.0);

/// The sounds known so far, per server.
#[derive(Default)]
pub struct Board {
    pub sounds: HashMap<Id, Vec<SoundboardSound>>,
    asked: HashSet<Id>,
    search: String,
    /// Opens the popout on the next frame.
    pub open: bool,
    /// The account's favorites, once its frecency has them.
    pub favorites: Vec<Id>,
}

impl Board {
    /// A server's sounds changed: asked again the next time the popout opens.
    pub fn changed(&mut self, guild: Id) {
        self.asked.remove(&guild);
    }

    pub fn find(&self, id: Id) -> Option<&SoundboardSound> {
        self.sounds.values().flatten().find(|sound| sound.id == id)
    }
}

/// Plays a sound here at its own volume, the account's soundboard volume and
/// the output's.
pub fn play(app: &App, id: Id, volume: f32) {
    let level = volume * app.preloaded.soundboard_volume() / 100.0 * app.settings.output_volume;
    crate::sounds::play_soundboard(id, app.settings.output_device.clone(), level.min(1.0));
}

/// The panel's row: camera, screen and activities, then the soundboard.
pub fn buttons(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let tile = (width - 3.0 * 8.0) / 4.0;
    let slot = |i: usize| Rect::from_min_size(pos2(row.left() + i as f32 * (tile + 8.0), row.top()), vec2(tile, 32.0));
    for (i, icon, tip) in [(0, Icon::VideoSlash, t!("Camera and screen sharing aren't available yet.")), (1, Icon::ScreenArrow, t!("Camera and screen sharing aren't available yet.")), (2, Icon::Activities, t!("Activities aren't available yet."))] {
        let rect = slot(i);
        let response = ui.interact(rect, ui.id().with(("panel-off", i)), Sense::hover());
        ui.painter().rect_filled(rect, 8.0, p.hover.gamma_multiply(0.5));
        icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(18.0)), icon, p.muted.gamma_multiply(0.5));
        response.on_hover_text(tip);
    }
    let rect = slot(3);
    let allowed = allowed(app);
    let response = ui.interact(rect, ui.id().with("panel-soundboard"), if allowed { Sense::click() } else { Sense::hover() });
    ui.painter().rect_filled(rect, 8.0, if response.hovered() && allowed { p.selected } else { p.hover });
    icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(18.0)), Icon::Soundboard, if allowed { p.strong } else { p.muted.gamma_multiply(0.5) });
    let tip = if app.voice.deaf { t!("Soundboard disabled when deafened") } else { t!("Soundboard") };
    let response = response.on_hover_text(tip);
    popout(app, &response);
}

/// Whether this channel lets the account send sounds at all.
fn allowed(app: &App) -> bool {
    if app.voice.deaf || !app.voice.connected {
        return false;
    }
    let Some((guild, channel)) = app.voice.channel else { return false };
    let Some(guild) = guild else { return true };
    let (Some(guild), Some(channel)) = (app.guild(guild), app.channel(channel)) else { return false };
    let perms = guild.channel_permissions(app.me_id(), channel);
    perms & USE_SOUNDBOARD != 0 && perms & SPEAK != 0
}

/// Asks for what the popout has not got yet: the defaults over REST and the
/// servers' through the gateway, as the web client does, since a REST call
/// per server would hit the rate limit in accounts with many.
fn ask(app: &mut App) {
    if app.soundboard.asked.insert(DEFAULTS) {
        app.commands.send(Command::Api { key: "soundboard-defaults".into(), method: "GET", path: "/soundboard-default-sounds".into(), body: None });
    }
    let guilds: Vec<Id> = app.guilds.iter().map(|guild| guild.id).filter(|id| !app.soundboard.asked.contains(id)).collect();
    if !guilds.is_empty() {
        app.soundboard.asked.extend(guilds.iter().copied());
        app.commands.send(Command::RequestSoundboard { guilds });
    }
}

/// Whether a sound can be sent here: its own server's and the defaults
/// always, another server's with Nitro and the permission (the bundle's rule).
fn usable(app: &App, sound: &SoundboardSound) -> bool {
    if !sound.available {
        return false;
    }
    let Some((here, channel)) = app.voice.channel else { return false };
    if sound.guild.is_none() || sound.guild == here {
        return true;
    }
    if app.me.as_ref().is_none_or(|me| me.premium == 0) {
        return false;
    }
    let Some(guild) = here.and_then(|g| app.guild(g)) else { return true };
    app.channel(channel).is_some_and(|channel| guild.channel_permissions(app.me_id(), channel) & USE_EXTERNAL_SOUNDS != 0)
}

fn send(app: &mut App, sound: &SoundboardSound) {
    let Some((_, channel)) = app.voice.channel else { return };
    let mut body = serde_json::json!({
        "sound_id": sound.id.to_string(),
        "emoji_id": sound.emoji_id.map(|id| id.to_string()),
        "emoji_name": sound.emoji_name,
    });
    if let Some(guild) = sound.guild {
        body["source_guild_id"] = guild.to_string().into();
    }
    app.commands.send(Command::Api { key: "soundboard-sent".into(), method: "POST", path: format!("/channels/{channel}/send-soundboard-sound"), body: Some(body) });
    // The event comes back to everyone but is ignored for ourselves.
    play(app, sound.id, sound.volume);
}

fn popout(app: &mut App, button: &egui::Response) {
    let p = theme::p();
    if std::mem::take(&mut app.soundboard.open) {
        egui::Popup::open_id(&button.ctx, egui::Popup::default_response_id(button));
    }
    egui::Popup::from_toggle_button_response(button)
        .align(egui::RectAlign::TOP_START)
        .gap(8.0)
        .width(452.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(12)))
        .show(|ui| {
            ui.set_width(428.0);
            ask(app);
            let mut search = std::mem::take(&mut app.soundboard.search);
            form::input(ui, &mut search, t!("Search"), 428.0);
            app.soundboard.search = search;
            ui.add_space(8.0);
            effects(app, ui);
            egui::ScrollArea::vertical().max_height(360.0).min_scrolled_height(320.0).auto_shrink([false, true]).show(ui, |ui| sections(app, ui));
        });
}

/// The row of emoji effects over the sounds: the account's most used, with
/// the free animation (type 1, the only id there is 0).
fn effects(app: &mut App, ui: &mut egui::Ui) {
    let picks: Vec<String> = app.frecency.as_ref().map_or_else(Vec::new, |frecency| frecency.emojis.iter().filter(|key| key.parse::<Id>().is_err()).filter_map(|key| emojis::get_by_shortcode(key).or_else(|| emojis::get(key))).take(9).map(|emoji| emoji.as_str().to_owned()).collect());
    if picks.is_empty() {
        return;
    }
    let Some((_, channel)) = app.voice.channel else { return };
    let p = theme::p();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for emoji in picks {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 6.0, p.hover);
            }
            super::markdown::paint_twemoji(app, ui, &emoji, rect.shrink(7.0));
            if response.clicked() {
                let body = serde_json::json!({ "animation_type": 1, "animation_id": 0, "emoji_id": null, "emoji_name": emoji });
                app.commands.send(Command::Api { key: "soundboard-sent".into(), method: "POST", path: format!("/channels/{channel}/voice-channel-effects"), body: Some(body) });
            }
        }
    });
    ui.add_space(6.0);
}

fn sections(app: &mut App, ui: &mut egui::Ui) {
    let query = app.soundboard.search.to_lowercase();
    let matches = |sound: &SoundboardSound| query.is_empty() || sound.name.to_lowercase().contains(&query);
    let here = app.voice.channel.and_then(|(guild, _)| guild);
    let mut lists: Vec<(String, Option<Id>, Vec<SoundboardSound>)> = Vec::new();
    let favorites: Vec<SoundboardSound> = app.soundboard.favorites.iter().filter_map(|id| app.soundboard.find(*id)).filter(|sound| matches(sound)).cloned().collect();
    if !favorites.is_empty() {
        lists.push((t!("Favorites").to_owned(), None, favorites));
    }
    // This server first, then the rail's order, then the defaults.
    let mut guilds: Vec<(Id, String)> = app.guilds.iter().map(|guild| (guild.id, guild.name.clone())).collect();
    guilds.sort_by_key(|(id, _)| Some(*id) != here);
    for (id, name) in guilds {
        let sounds: Vec<SoundboardSound> = app.soundboard.sounds.get(&id).map_or_else(Vec::new, |sounds| sounds.iter().filter(|sound| matches(sound)).cloned().collect());
        if !sounds.is_empty() {
            lists.push((name, Some(id), sounds));
        }
    }
    let defaults: Vec<SoundboardSound> = app.soundboard.sounds.get(&DEFAULTS).map_or_else(Vec::new, |sounds| sounds.iter().filter(|sound| matches(sound)).cloned().collect());
    if !defaults.is_empty() {
        lists.push((t!("Default").to_owned(), None, defaults));
    }
    let p = theme::p();
    if lists.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| ui.label(RichText::new(if query.is_empty() { t!("Loading…") } else { t!("No results found") }).color(p.muted)));
        ui.add_space(24.0);
        return;
    }
    for (title, guild, sounds) in lists {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if let Some(guild) = guild.and_then(|g| app.guild(g)).cloned() {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                app.images.circle(ui, rect, guild.icon_url().as_deref(), &super::initials(&guild.name), 4.0);
            }
            ui.label(RichText::new(title.to_uppercase()).font(theme::strong(12.0)).color(p.muted));
        });
        ui.add_space(4.0);
        for chunk in sounds.chunks(TILES) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for sound in chunk {
                    tile(app, ui, sound);
                }
            });
            ui.add_space(2.0);
        }
        ui.add_space(6.0);
    }
}

/// A sound: its emoji and name, the speaker on hover to hear it alone.
fn tile(app: &mut App, ui: &mut egui::Ui, sound: &SoundboardSound) {
    let p = theme::p();
    let can = usable(app, sound);
    let (rect, response) = ui.allocate_exact_size(TILE, if can { Sense::click() } else { Sense::hover() });
    let hovered = ui.rect_contains_pointer(rect);
    ui.painter().rect_filled(rect, 8.0, if hovered && can { p.selected } else { p.hover });
    let tint = if can { Color32::WHITE } else { Color32::from_white_alpha(90) };
    let emoji = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(22.0));
    if let Some(id) = sound.emoji_id {
        if let Some((texture, _)) = app.images.get(&crate::discord::model::emoji_url(id, false, 48), 48) {
            egui::Image::from_texture((texture, emoji.size())).tint(tint).paint_at(ui, emoji);
        }
    } else if let Some(name) = &sound.emoji_name {
        super::markdown::paint_twemoji(app, ui, name, emoji);
    }
    let right = if hovered { rect.right() - 32.0 } else { rect.right() - 8.0 };
    let name = Rect::from_min_max(pos2(emoji.right() + 8.0, rect.top()), pos2(right, rect.bottom()));
    super::paint_truncated(ui, name.shrink2(vec2(0.0, 13.0)), &sound.name, FontId::proportional(13.0), if can { p.strong } else { p.muted });
    let mut previewing = false;
    if hovered {
        let speaker = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(24.0));
        let preview = ui.interact(speaker, ui.id().with(("sound-preview", sound.id)), Sense::click());
        previewing = preview.hovered();
        icons::paint(ui, speaker.shrink(3.0), Icon::Voice, if previewing { p.strong } else { p.muted });
        if preview.on_hover_text(t!("Preview")).clicked() {
            play(app, sound.id, sound.volume);
        }
    }
    if can && !previewing && response.clicked() {
        send(app, sound);
    }
}
