//! Drawing message components as Discord does: containers with an accent
//! bar, sections with a thumbnail or button beside their text, media
//! galleries, separators, files, and rows of buttons and select menus that
//! send their interaction to the bot.

use egui::{Color32, CornerRadius, Frame, Margin, Rect, RichText, Sense, Stroke, Vec2, vec2};
use serde_json::json;

use super::markdown;
use crate::app::App;
use crate::discord::components::{Component, Media, Origin};
use crate::discord::model::Message;
use crate::theme;

pub fn show(app: &mut App, ui: &mut egui::Ui, message: &Message) {
    for component in &message.components {
        draw(app, ui, message, component);
    }
}

fn draw(app: &mut App, ui: &mut egui::Ui, message: &Message, component: &Component) {
    let p = theme::p();
    match component {
        Component::Container { accent, children, .. } => {
            ui.add_space(4.0);
            let response = Frame::new()
                .fill(p.sidebar)
                .stroke(Stroke::new(1.0, p.selected))
                .corner_radius(8)
                .inner_margin(Margin { left: if accent.is_some() { 18 } else { 14 }, right: 14, top: 12, bottom: 12 })
                .show(ui, |ui| {
                    ui.set_max_width(452.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for child in children {
                        draw(app, ui, message, child);
                    }
                })
                .response;
            if let Some(color) = accent.and_then(theme::role_color) {
                let r = response.rect;
                ui.painter().rect_filled(
                    Rect::from_min_size(r.min, vec2(4.0, r.height())),
                    CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
                    color,
                );
            }
        }
        Component::Text(content) => {
            let blocks = markdown::parse(content);
            markdown::show(app, ui, &blocks, p.text, message.id);
        }
        Component::Section { texts, accessory } => {
            ui.horizontal_top(|ui| {
                let width = ui.available_width() - if accessory.is_some() { 96.0 } else { 0.0 };
                ui.vertical(|ui| {
                    ui.set_width(width.max(120.0));
                    for text in texts {
                        let blocks = markdown::parse(text);
                        markdown::show(app, ui, &blocks, p.text, message.id);
                    }
                });
                if let Some(accessory) = accessory {
                    match accessory.as_ref() {
                        Component::Thumbnail(media) => picture(app, ui, media, 85.0, 85.0),
                        other => draw(app, ui, message, other),
                    }
                }
            });
        }
        Component::Thumbnail(media) => picture(app, ui, media, 85.0, 85.0),
        Component::Gallery(items) => gallery(app, ui, items),
        Component::File { url, name } => {
            Frame::new().fill(p.chat).corner_radius(6).inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                    crate::icons::paint(ui, icon, crate::icons::Icon::File, p.muted);
                    ui.hyperlink_to(RichText::new(name).color(p.link), url);
                });
            });
        }
        Component::Separator { divider, large } => {
            let gap = if *large { 12.0 } else { 4.0 };
            ui.add_space(gap);
            if *divider {
                let y = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, p.selected));
            }
            ui.add_space(gap);
        }
        Component::Row(children) => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for child in children {
                    draw(app, ui, message, child);
                }
            });
        }
        Component::Button { style, label, emoji, custom_id, url, disabled } => {
            button(app, ui, message, *style, label.as_deref(), emoji.as_ref(), custom_id.as_deref(), url.as_deref(), *disabled);
        }
        Component::Select { kind, custom_id, placeholder, options, max, disabled, .. } => {
            let Some(origin) = message.origin.clone() else { return };
            let chosen = options.iter().find(|o| o.default).map(|o| o.label.clone());
            let text = chosen.or_else(|| placeholder.clone()).unwrap_or_else(|| t!("Make a selection").into());
            if *kind != 3 {
                // People, roles or channels: the server's, from what is known here.
                let guild = message.guild_id.or_else(|| app.channel(message.channel_id).and_then(|c| c.guild_id));
                let mut picks: Vec<(String, String)> = Vec::new();
                if let Some(guild) = guild.and_then(|g| app.guild(g)) {
                    if matches!(*kind, 5 | 7) {
                        let known = app.members.keys().filter(|(g, _)| *g == guild.id).filter_map(|(_, user)| app.users.get(user));
                        picks.extend(known.map(|user| (user.display_name().to_owned(), user.id.to_string())));
                    }
                    if matches!(*kind, 6 | 7) {
                        picks.extend(guild.roles.iter().map(|role| (format!("@{}", role.name), role.id.to_string())));
                    }
                    if *kind == 8 {
                        picks.extend(guild.channels.iter().filter(|c| c.kind != crate::discord::model::ChannelKind::Category).map(|c| (format!("#{}", c.title()), c.id.to_string())));
                    }
                }
                picks.sort_by_key(|(name, _)| name.to_lowercase());
                ui.add_enabled_ui(!disabled, |ui| {
                    egui::ComboBox::from_id_salt(("select", message.id, custom_id.as_str())).selected_text(text).width(ui.available_width().min(420.0)).show_ui(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                            for (name, value) in &picks {
                                if ui.selectable_label(false, name).clicked() {
                                    interact(app, &origin, json!({ "component_type": kind, "custom_id": custom_id, "type": kind, "values": [value] }));
                                }
                            }
                        });
                    });
                });
                return;
            }
            ui.add_enabled_ui(!disabled && *kind == 3, |ui| {
                egui::ComboBox::from_id_salt(("select", message.id, custom_id.as_str()))
                    .selected_text(text)
                    .width(ui.available_width().min(420.0))
                    .show_ui(ui, |ui| {
                        for option in options {
                            let label = match &option.emoji {
                                Some(e) if e.id.is_none() => format!("{} {}", e.name, option.label),
                                _ => option.label.clone(),
                            };
                            let response = ui.selectable_label(option.default, label);
                            let response = match &option.description {
                                Some(d) => response.on_hover_text(d),
                                None => response,
                            };
                            if response.clicked() {
                                let values = if *max > 1 { vec![option.value.clone()] } else { vec![option.value.clone()] };
                                interact(app, &origin, json!({ "component_type": 3, "custom_id": custom_id, "type": 3, "values": values }));
                            }
                        }
                    });
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn button(
    app: &mut App,
    ui: &mut egui::Ui,
    message: &Message,
    style: u64,
    label: Option<&str>,
    emoji: Option<&crate::discord::model::Emoji>,
    custom_id: Option<&str>,
    url: Option<&str>,
    disabled: bool,
) {
    let p = theme::p();
    let (fill, text) = match style {
        1 => (p.accent, Color32::WHITE),
        3 => (p.green, Color32::WHITE),
        4 => (p.red, Color32::WHITE),
        _ => (p.selected, p.strong),
    };
    let label_width = label.map_or(0.0, |l| l.chars().count() as f32 * 7.5);
    let emoji_width = if emoji.is_some() { 22.0 } else { 0.0 };
    let link = style == 5;
    let width = (label_width + emoji_width + if link { 20.0 } else { 0.0 } + 24.0).max(40.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 32.0), if disabled { Sense::hover() } else { Sense::click() });
    let fill = if disabled { fill.linear_multiply(0.5) } else if response.hovered() { fill.linear_multiply(0.85) } else { fill };
    ui.painter().rect_filled(rect, 4.0, fill);
    let mut x = rect.left() + 12.0;
    if let Some(emoji) = emoji {
        let icon = Rect::from_min_size(egui::pos2(x, rect.center().y - 9.0), Vec2::splat(18.0));
        match emoji.url() {
            Some(url) => {
                if let Some((texture, _)) = app.images.get(&url, 40) {
                    egui::Image::from_texture((texture, icon.size())).paint_at(ui, icon);
                }
            }
            None => fastframe_emoji::paint_cluster(ui, &emoji.name, icon),
        }
        x += 22.0;
    }
    if let Some(label) = label {
        ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, label, theme::strong(14.0), text);
    }
    if link {
        let icon = Rect::from_center_size(egui::pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(16.0));
        crate::icons::paint(ui, icon, crate::icons::Icon::Launch, text);
    }
    if !response.clicked() {
        return;
    }
    if let Some(url) = url {
        let _ = open::that_detached(url);
    } else if let (Some(custom_id), Some(origin)) = (custom_id, message.origin.clone()) {
        interact(app, &origin, json!({ "component_type": 2, "custom_id": custom_id }));
    }
}

/// Sends a component interaction. Its server comes from the channel, as
/// Discord's `executeStateUpdate` takes `channel.guild_id`: messages loaded
/// from history carry no `guild_id`, and without one a server's bot answers
/// "This interaction failed".
fn interact(app: &mut App, origin: &Origin, data: serde_json::Value) {
    let mut origin = origin.clone();
    if let Some(guild) = app.channel(origin.channel).and_then(|c| c.guild_id) {
        origin.guild = Some(guild);
    }
    // Its nonce comes back in INTERACTION_FAILURE when the app does not
    // answer, which then says so.
    let nonce = crate::discord::model::now_snowflake().to_string();
    app.sent_commands.push(nonce.clone());
    if app.sent_commands.len() > 16 {
        app.sent_commands.remove(0);
    }
    app.commands.send(crate::backend::Command::Interact { origin, data, nonce });
}

fn picture(app: &mut App, ui: &mut egui::Ui, media: &Media, max_w: f32, max_h: f32) {
    let p = theme::p();
    let (w, h) = (media.width.unwrap_or(400).max(1) as f32, media.height.unwrap_or(300).max(1) as f32);
    let scale = (max_w / w).min(max_h / h).min(1.0);
    let size = vec2((w * scale).round(), (h * scale).round());
    let ppp = ui.ctx().pixels_per_point();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    match app.images.get(media.shown(), (size.x.max(size.y) * ppp) as u32) {
        Some((texture, _)) => {
            egui::Image::from_texture((texture, size)).corner_radius(6.0).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 6.0, p.chat);
        }
    }
    if media.spoiler && !app.revealed.contains(&(0, media.url.len())) {
        ui.painter().rect_filled(rect, 6.0, p.rail);
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, t!("Spoiler").to_uppercase(), theme::strong(13.0), p.strong);
    }
    let response = match &media.description {
        Some(d) => response.on_hover_text(d),
        None => response,
    };
    if response.clicked() {
        if media.spoiler {
            app.revealed.insert((0, media.url.len()));
        } else {
            app.viewer = Some(crate::app::Viewer {
                url: media.shown().to_owned(),
                original: media.url.clone(),
                size: vec2(w, h),
                zoom: 1.0,
                offset: Vec2::ZERO,
                from: None,
            });
        }
    }
}

/// One picture large; several in a grid of two or three columns.
fn gallery(app: &mut App, ui: &mut egui::Ui, items: &[Media]) {
    match items.len() {
        0 => {}
        1 => picture(app, ui, &items[0], 420.0, 420.0),
        n => {
            let columns = if n == 2 || n == 4 { 2 } else { 3 };
            let cell = (ui.available_width().min(452.0) - 4.0 * (columns as f32 - 1.0)) / columns as f32;
            for row in items.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for item in row {
                        picture(app, ui, item, cell, cell);
                    }
                });
            }
        }
    }
}
