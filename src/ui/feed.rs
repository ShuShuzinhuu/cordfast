//! The member list's "Activity" section (the bundle's content inventory
//! feed, `CONTENT_INVENTORY_GROUP`): what the server's members played,
//! listened to and watched, from Discord's activity feed, three cards
//! unless expanded. Each card is Discord's: 70 points, rounded 12, the
//! person's face and name, the title, a line of badges and a 48-point
//! picture at the right.

use egui::{Align2, Color32, FontId, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::discord::model::{ChannelKind, FeedEntry, Id};
use crate::icons::{self, Icon};
use crate::theme;

const CARD: f32 = 70.0;
const GAP: f32 = 8.0;

/// Asks for the feed when it is older than Discord's refresh (15 minutes).
pub fn ask(app: &mut App) {
    if app.feed_asked.is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(15 * 60)) {
        app.feed_asked = Some(std::time::Instant::now());
        app.commands.send(crate::admin::get("content-inventory", "/content-inventory/users/@me".to_owned()));
    }
}

/// The entries this server's list shows: by its members.
fn entries(app: &App, guild: Id) -> Vec<FeedEntry> {
    app.feed.iter().filter(|e| app.members.contains_key(&(guild, e.author))).cloned().collect()
}

/// The section, above the member list.
pub fn section(app: &mut App, ui: &mut egui::Ui, guild: Id, channel: Id) {
    ask(app);
    if app.channel(channel).is_some_and(|c| c.kind == ChannelKind::Forum) {
        return;
    }
    let all = entries(app, guild);
    if all.is_empty() {
        return;
    }
    let p = theme::p();
    let shown: Vec<FeedEntry> = if app.feed_expanded { all.clone() } else { all.iter().take(3).cloned().collect() };
    // "Activity — 2" and its settings button, as a member group's header.
    let (header, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    let text = ui.painter().text(pos2(header.left() + 12.0, header.bottom() - 12.0), Align2::LEFT_CENTER, format!("{} — {}", t!("Activity"), shown.len()), FontId::proportional(14.0), p.muted);
    icons::paint(ui, Rect::from_center_size(pos2(text.right() + 12.0, text.center().y), Vec2::splat(12.0)), Icon::Settings, p.muted);
    if all.len() > 3 && response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.feed_expanded = !app.feed_expanded;
    }
    for entry in &shown {
        card(app, ui, guild, entry);
        ui.add_space(GAP);
    }
}

fn card(app: &mut App, ui: &mut egui::Ui, guild: Id, entry: &FeedEntry) {
    let p = theme::p();
    let (outer, response) = ui.allocate_exact_size(vec2(ui.available_width(), CARD), Sense::click());
    let rect = Rect::from_min_max(pos2(outer.left() + 8.0, outer.top()), outer.right_bottom());
    let fill = if response.hovered() { p.hover } else { p.hover.gamma_multiply(0.6) };
    ui.painter().rect_filled(rect, 12.0, fill);
    let inner = rect.shrink(8.0);
    // The picture: the game's icon, the album or the show.
    let picture = match entry.kind {
        1 | 3 | 8 => entry.application.and_then(|a| app.app_icon(a)),
        _ => entry.image.clone(),
    };
    let thumb = Rect::from_min_size(pos2(inner.right() - 48.0, inner.center().y - 24.0), Vec2::splat(48.0));
    if let Some((texture, _)) = picture.as_deref().and_then(|url| app.images.get(url, 96)) {
        egui::Image::from_texture((texture, thumb.size())).corner_radius(4.0).paint_at(ui, thumb);
    }
    let right = thumb.left() - 8.0;
    // Who: the face and the name in their role colour.
    let user = app.users.get(&entry.author).cloned();
    let member = app.members.get(&(guild, entry.author)).cloned();
    let name = member.as_ref().and_then(|m| m.nick.clone()).or_else(|| user.as_ref().map(|u| u.display_name().to_owned())).unwrap_or_default();
    let color = member.as_ref().and_then(|m| app.guild(guild).and_then(|g| theme::role_color(g.role_color(&m.roles)))).unwrap_or(p.muted);
    let face = Rect::from_min_size(pos2(inner.left() + 1.0, inner.top() + 1.0), Vec2::splat(16.0));
    app.images.circle(ui, face, user.as_ref().map(|u| u.avatar_url()).as_deref(), &super::initials(&name), 8.0);
    super::paint_truncated(ui, Rect::from_min_max(pos2(face.right() + 6.0, inner.top()), pos2(right, inner.top() + 18.0)), &name, theme::strong(14.0), color);
    // What: the title.
    super::paint_truncated(ui, Rect::from_min_max(pos2(inner.left(), inner.top() + 20.0), pos2(right, inner.top() + 38.0)), &entry.title, theme::strong(14.0), p.text);
    // The badges, in a row, as many as fit.
    let mut x = inner.left();
    let y = inner.top() + 46.0;
    let mut badge = |ui: &egui::Ui, icon: Icon, text: String, color: Color32, icon_color: Color32| {
        if x >= right - 20.0 {
            return;
        }
        icons::paint(ui, Rect::from_center_size(pos2(x + 6.0, y), Vec2::splat(12.0)), icon, icon_color);
        let galley = ui.painter().layout_no_wrap(text, FontId::proportional(12.0), color);
        let width = galley.size().x.min(right - x - 16.0);
        super::paint_truncated(ui, Rect::from_min_max(pos2(x + 16.0, y - 8.0), pos2(x + 16.0 + width, y + 8.0)), galley.text(), FontId::proportional(12.0), color);
        x += 16.0 + width + 8.0;
    };
    match entry.kind {
        1 => {
            if entry.playing() {
                // How long so far, in the online green, as "1:39:57".
                if let Some(start) = entry.started {
                    let secs = crate::app::now_ms().saturating_sub(start) / 1000;
                    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
                    let text = if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") };
                    badge(ui, Icon::Controller, text, p.green, p.green);
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                }
                // And where they are talking, in this server.
                if let Some(channel) = app.user_voice_channel(entry.author).filter(|c| c.guild_id == Some(guild)).map(|c| c.name.clone()) {
                    badge(ui, Icon::Voice, channel, p.muted, p.muted);
                }
            } else if let Some(hours) = entry.duration.map(|d| (d as f64 / 3600.0).round() as u64).filter(|h| *h > 0) {
                badge(ui, Icon::Controller, t!("{hours}h", hours = hours), p.muted, p.muted);
            }
            // Three days or more in a row: "19x Streak".
            if let Some(days) = entry.streak.filter(|d| *d >= 3) {
                badge(ui, Icon::Flash, t!("{days}x Streak", days = days), p.muted, p.muted);
            }
        }
        3 => {
            if let Some(hours) = entry.duration.map(|d| (d as f64 / 3600.0).round() as u64).filter(|h| *h > 0) {
                badge(ui, Icon::Trophy, t!("{hours}h this week", hours = hours), p.muted, p.muted);
            }
        }
        6 => {
            if let Some(count) = entry.count {
                badge(ui, Icon::Trophy, t!("{count} tracks this week", count = count), p.muted, p.muted);
            }
        }
        // A session: the music note, green while it goes on (IS_LIVE),
        // and the track's artists.
        5 => {
            let tint = if entry.live { p.green } else { p.muted };
            badge(ui, Icon::Music, entry.detail.clone(), tint, tint);
        }
        2 => badge(ui, Icon::Tv, entry.title.clone(), p.muted, p.muted),
        _ => {}
    }
    if response.clicked()
        && let Some(user) = user
    {
        app.actions.push(Action::OpenProfile { user, guild: Some(guild), at: rect.left_top() - vec2(350.0, 0.0) });
    }
}
