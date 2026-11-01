//! Composer autocomplete for :emoji:, @people and #channels (the picker
//! itself is `expressions.rs`).

use egui::{Align2, Frame, Margin, RichText, Sense, Vec2, vec2};

use crate::app::App;
use crate::discord::model::{ChannelKind, Emoji, Id};
use crate::theme;

pub(super) fn insert_custom(app: &mut App, emoji: &Emoji) {
    let shown = format!(":{}:", emoji.name);
    let token = match emoji.id {
        Some(id) if emoji.animated => format!("<a:{}:{id}>", emoji.name),
        Some(id) => format!("<:{}:{id}>", emoji.name),
        None => emoji.name.clone(),
    };
    app.composer.push_str(&shown);
    app.composer.push(' ');
    app.mention_tokens.push((shown, token));
}

// ---- autocomplete ---------------------------------------------------------------------

#[derive(Clone)]
pub enum Suggestion {
    User { id: Id, name: String, username: String, avatar: String },
    Channel { id: Id, name: String },
    Custom(Emoji),
    Unicode(&'static str, &'static str),
}

/// What the word being typed could complete to, and where that word starts.
pub fn suggestions(app: &App, channel: Id) -> (usize, Vec<Suggestion>) {
    let text = &app.composer;
    let start = text.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
    let word = &text[start..];
    let mut out = Vec::new();
    if let Some(query) = word.strip_prefix('@') {
        let query = query.to_lowercase();
        let channel_info = app.channel(channel);
        let guild = channel_info.and_then(|c| c.guild_id);
        let mut seen = std::collections::HashSet::new();
        let mut candidates: Vec<&crate::discord::model::User> = Vec::new();
        if let Some(c) = channel_info {
            candidates.extend(c.recipients.iter());
        }
        if let Some(history) = app.histories.get(&channel) {
            candidates.extend(history.messages.iter().rev().map(|m| &m.author));
        }
        if let Some(guild) = guild {
            candidates.extend(
                app.members
                    .keys()
                    .filter(|(g, _)| *g == guild)
                    .filter_map(|(_, u)| app.users.get(u)),
            );
        }
        for user in candidates {
            if user.id == app.me_id() || !seen.insert(user.id) {
                continue;
            }
            let nick = guild
                .and_then(|g| app.members.get(&(g, user.id)))
                .and_then(|m| m.nick.clone());
            let name = nick.unwrap_or_else(|| user.display_name().to_owned());
            if query.is_empty()
                || name.to_lowercase().contains(&query)
                || user.username.to_lowercase().contains(&query)
            {
                out.push(Suggestion::User {
                    id: user.id,
                    name,
                    username: user.username.clone(),
                    avatar: user.avatar_url(),
                });
            }
            if out.len() >= 8 {
                break;
            }
        }
    } else if let Some(query) = word.strip_prefix('#') {
        let query = query.to_lowercase();
        if let Some(guild) = app.channel(channel).and_then(|c| c.guild_id).and_then(|g| app.guild(g)) {
            let me = app.me_id();
            for c in &guild.channels {
                if matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement | ChannelKind::Voice | ChannelKind::Forum)
                    && c.name.to_lowercase().contains(&query)
                    && guild.can_view(me, c)
                {
                    out.push(Suggestion::Channel { id: c.id, name: c.name.clone() });
                    if out.len() >= 8 {
                        break;
                    }
                }
            }
        }
    } else if let Some(query) = word.strip_prefix(':')
        && query.len() >= 2
        && !query.contains(':')
    {
        let query = query.to_lowercase();
        let guild = app.selected_guild;
        let mut guilds: Vec<_> = app.guilds.iter().collect();
        guilds.sort_by_key(|g| Some(g.id) != guild);
        for g in guilds {
            for e in &g.emojis {
                if e.name.to_lowercase().starts_with(&query) && out.len() < 4 {
                    out.push(Suggestion::Custom(e.clone()));
                }
            }
        }
        for e in emojis::iter() {
            if e.skin_tone().is_some_and(|t| t != emojis::SkinTone::Default) {
                continue;
            }
            if let Some(code) = e.shortcodes().find(|s| s.starts_with(&query)) {
                out.push(Suggestion::Unicode(e.as_str(), code));
                if out.len() >= 8 {
                    break;
                }
            }
        }
    }
    (start, out)
}

pub fn pick(app: &mut App, start: usize, suggestion: &Suggestion) {
    app.composer.truncate(start);
    match suggestion {
        Suggestion::User { id, name, .. } => {
            let shown = format!("@{name}");
            app.composer.push_str(&shown);
            app.mention_tokens.push((shown, format!("<@{id}>")));
        }
        Suggestion::Channel { id, name } => {
            let shown = format!("#{name}");
            app.composer.push_str(&shown);
            app.mention_tokens.push((shown, format!("<#{id}>")));
        }
        Suggestion::Custom(e) => {
            insert_custom(app, e);
            return;
        }
        Suggestion::Unicode(text, _) => app.composer.push_str(text),
    }
    app.composer.push(' ');
}

/// The list over the composer. Returns the suggestion clicked.
pub fn suggestion_list(app: &mut App, ui: &mut egui::Ui, list: &[Suggestion]) -> Option<usize> {
    let p = theme::p();
    let mut clicked = None;
    Frame::new()
        .fill(p.sidebar)
        .corner_radius(8)
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, suggestion) in list.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                if response.hovered() || i == 0 {
                    ui.painter().rect_filled(rect, 4.0, if i == 0 { p.selected } else { p.hover });
                }
                let icon = egui::Rect::from_min_size(rect.left_center() + vec2(8.0, -11.0), Vec2::splat(22.0));
                let (title, detail) = match suggestion {
                    Suggestion::User { name, username, avatar, .. } => {
                        app.images.circle(ui, icon, Some(avatar), &super::initials(name), 11.0);
                        (name.clone(), username.clone())
                    }
                    Suggestion::Channel { name, .. } => {
                        ui.painter().text(icon.center(), Align2::CENTER_CENTER, "#", egui::FontId::proportional(18.0), p.muted);
                        (name.clone(), String::new())
                    }
                    Suggestion::Custom(e) => {
                        if let Some(url) = e.url()
                            && let Some((texture, _)) = app.images.get(&url, 44)
                        {
                            egui::Image::from_texture((texture, icon.size())).paint_at(ui, icon);
                        }
                        (format!(":{}:", e.name), String::new())
                    }
                    Suggestion::Unicode(text, code) => {
                        fastframe_emoji::paint_cluster(ui, text, icon);
                        (format!(":{code}:"), String::new())
                    }
                };
                ui.painter().text(rect.left_center() + vec2(40.0, 0.0), Align2::LEFT_CENTER, title, theme::strong(14.0), p.strong);
                if !detail.is_empty() {
                    ui.painter().text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, detail, egui::FontId::proportional(12.5), p.muted);
                }
                if response.clicked() {
                    clicked = Some(i);
                }
            }
            ui.label(RichText::new(t!("Tab or Enter picks the first")).size(11.0).color(p.faint));
        });
    clicked
}
