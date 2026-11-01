//! Activity cards, as Discord shows what someone is doing: Spotify with the
//! cover and a progress bar, a game's rich presence with its art and
//! elapsed time, Crunchyroll and other apps that report activities.

use egui::{Align2, FontId, Frame, Margin, Rect, RichText, Sense, Vec2, vec2};

use crate::app::App;
use crate::discord::gateway::Activity;
use crate::theme;

fn clock(ms: u64) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

pub fn card(app: &mut App, ui: &mut egui::Ui, activity: &Activity) {
    let p = theme::p();
    let now = crate::app::now_ms();
    Frame::new().fill(p.rail).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(activity.header()).font(theme::strong(12.5)).color(p.strong));
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            let art = 64.0;
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(art), Sense::hover());
            let picture = activity
                .large_image
                .clone()
                .or_else(|| activity.application.and_then(|a| app.app_icon(a)));
            match picture.as_deref().and_then(|url| app.images.get(url, 128)) {
                Some((texture, _)) => {
                    egui::Image::from_texture((texture, rect.size())).corner_radius(8.0).paint_at(ui, rect);
                }
                None => {
                    ui.painter().rect_filled(rect, 8.0, p.selected);
                    crate::icons::paint(ui, rect.shrink(16.0), crate::icons::Icon::Discord, p.muted);
                }
            }
            if let Some(text) = &activity.large_text {
                response.on_hover_text(text);
            }
            if let Some(small) = activity.small_image.as_deref().and_then(|url| app.images.get(url, 48)) {
                let badge = Rect::from_center_size(rect.right_bottom() - vec2(4.0, 4.0), Vec2::splat(22.0));
                ui.painter().circle_filled(badge.center(), 13.0, p.rail);
                egui::Image::from_texture((small.0, badge.size())).corner_radius(11.0).paint_at(ui, badge);
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let title = match activity.kind {
                    2 => activity.details.clone().unwrap_or_else(|| activity.name.clone()),
                    _ => activity.name.clone(),
                };
                ui.add(egui::Label::new(RichText::new(title).font(theme::strong(14.0)).color(p.strong)).truncate());
                let lines: Vec<String> = match activity.kind {
                    // Spotify: artists, then the album.
                    2 => [activity.state.clone().map(|s| t!("by {artist}", artist = s)), activity.large_text.clone().map(|a| t!("on {album}", album = a))]
                        .into_iter()
                        .flatten()
                        .collect(),
                    _ => [activity.details.clone(), activity.state.clone()].into_iter().flatten().collect(),
                };
                for line in lines {
                    ui.add(egui::Label::new(RichText::new(line).size(13.0).color(p.text)).truncate());
                }
                if activity.end.is_none()
                    && let Some(start) = activity.start
                {
                    ui.label(RichText::new(t!("{time} elapsed", time = clock(now.saturating_sub(start)))).size(12.5).color(p.muted));
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                }
            });
        });
        // A song or episode: the progress bar with both times.
        if let (Some(start), Some(end)) = (activity.start, activity.end)
            && end > start
        {
            ui.add_space(8.0);
            let elapsed = now.saturating_sub(start).min(end - start);
            let total = end - start;
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, p.selected);
            let done = Rect::from_min_size(rect.min, vec2(rect.width() * elapsed as f32 / total as f32, rect.height()));
            ui.painter().rect_filled(done, 2.0, p.strong);
            let (times, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
            ui.painter().text(times.left_center(), Align2::LEFT_CENTER, clock(elapsed), FontId::proportional(11.5), p.muted);
            ui.painter().text(times.right_center(), Align2::RIGHT_CENTER, clock(total), FontId::proportional(11.5), p.muted);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        // A game's buttons: Discord only hands their URLs to the game's
        // own players, so the labels show as they do there for others.
        for label in &activity.buttons {
            ui.add_space(6.0);
            ui.add_enabled(false, egui::Button::new(RichText::new(label).color(p.strong)).fill(p.selected).min_size(vec2(ui.available_width(), 30.0)));
        }
        if activity.kind == 1
            && let Some(url) = &activity.url
            && ui.link(t!("Watch")).clicked()
        {
            let _ = open::that_detached(url);
        }
    });
}

/// Minutes and seconds both in two figures, as the voice card's song times:
/// "00:19", "02:37".
fn two_figures(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Whether someone's voice card is open, which keeps their row lit.
pub fn voice_card_open(ui: &egui::Ui, user: crate::discord::model::Id) -> bool {
    ui.data(|d| d.get_temp::<Rect>(egui::Id::new(("voice-card", user)))).is_some()
}

/// Discord's card beside someone in a voice channel while the pointer is on
/// their row: what they are playing ("Playing Now") and listening to
/// ("Listening Now"), one compact block each. It stays while the pointer
/// crosses from the row to it, so its buttons can be clicked.
pub fn voice_card(app: &mut App, ui: &egui::Ui, row: &egui::Response, user: crate::discord::model::Id) {
    let ctx = ui.ctx().clone();
    let id = egui::Id::new(("voice-card", user));
    let shown: Option<Rect> = ctx.data(|d| d.get_temp(id));
    let pointer = ctx.pointer_hover_pos();
    let keep = shown.zip(pointer).is_some_and(|(card, at)| {
        let corridor = Rect::from_min_max(row.rect.min, egui::pos2(card.left() + 6.0, row.rect.max.y));
        card.expand(6.0).contains(at) || corridor.contains(at)
    });
    if !row.hovered() && !keep {
        if shown.is_some() {
            ctx.data_mut(|d| d.remove::<Rect>(id));
        }
        return;
    }
    // Only now, with the pointer on it, is anything copied.
    let Some(activities) = app.presences.get(&user).map(|p| p.activities.clone()).filter(|a| !a.is_empty()) else { return };
    let p = theme::p();
    // Beside the channel list, its top a little above the row, as Discord's.
    let at = egui::pos2(ui.clip_rect().right() + 12.0, row.rect.top() - 14.0);
    let area = egui::Area::new(id.with("area")).order(egui::Order::Foreground).fixed_pos(at).constrain(true).show(&ctx, |ui| {
        Frame::new().fill(p.rail).stroke(egui::Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(248.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, activity) in activities.iter().enumerate() {
                if i > 0 {
                    ui.add_space(16.0);
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().hline(line.x_range(), line.center().y, egui::Stroke::new(1.0, p.selected));
                    ui.add_space(16.0);
                }
                voice_block(app, ui, activity);
            }
        });
    });
    ctx.data_mut(|d| d.insert_temp(id, area.response.rect));
}

/// One activity in the voice card: its heading, the art with the name and
/// details, the time, and its buttons.
fn voice_block(app: &mut App, ui: &mut egui::Ui, activity: &Activity) {
    let p = theme::p();
    let now = crate::app::now_ms();
    let heading = match activity.kind {
        1 => t!("Streaming Now"),
        2 => t!("Listening Now"),
        3 => t!("Watching Now"),
        5 => t!("Competing Now"),
        _ => t!("Playing Now"),
    };
    ui.label(RichText::new(heading).font(theme::strong(12.0)).color(p.muted));
    ui.add_space(8.0);
    let music = activity.kind == 2;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (art, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
        let picture = activity.large_image.clone().or_else(|| activity.application.and_then(|a| app.app_icon(a)));
        match picture.as_deref().and_then(|url| app.images.get(url, 80)) {
            Some((texture, _)) => {
                egui::Image::from_texture((texture, art.size())).corner_radius(if music { 4.0 } else { 8.0 }).paint_at(ui, art);
            }
            None => {
                ui.painter().rect_filled(art, 8.0, p.selected);
                let icon = if music { crate::icons::Icon::Spotify } else { crate::icons::Icon::Controller };
                crate::icons::paint(ui, art.shrink(10.0), icon, p.muted);
            }
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let title = if music { activity.details.clone().unwrap_or_else(|| activity.name.clone()) } else { activity.name.clone() };
            ui.add(egui::Label::new(RichText::new(title).font(theme::strong(14.0)).color(p.strong)).truncate());
            let lines: Vec<&String> = if music { activity.state.iter().collect() } else { activity.details.iter().chain(activity.state.iter()).collect() };
            for line in lines {
                ui.add(egui::Label::new(RichText::new(line).size(12.0).color(p.muted)).truncate());
            }
            // A game's time in green with a controller, as Discord's.
            if !music
                && activity.end.is_none()
                && let Some(start) = activity.start
            {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    crate::icons::paint(ui, icon, crate::icons::Icon::Controller, p.green);
                    ui.label(RichText::new(clock(now.saturating_sub(start))).font(theme::strong(12.0)).color(p.green));
                });
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        });
    });
    // A song: the bar between both times.
    if let (Some(start), Some(end)) = (activity.start, activity.end)
        && end > start
    {
        ui.add_space(10.0);
        let total = end - start;
        let elapsed = now.saturating_sub(start).min(total);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let font = FontId::proportional(12.0);
            let left = ui.painter().layout_no_wrap(two_figures(elapsed), font.clone(), p.muted);
            let right = ui.painter().layout_no_wrap(two_figures(total), font, p.muted);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
            ui.painter().galley(egui::pos2(rect.left(), rect.center().y - left.size().y / 2.0), left.clone(), p.muted);
            ui.painter().galley(egui::pos2(rect.right() - right.size().x, rect.center().y - right.size().y / 2.0), right.clone(), p.muted);
            let bar = Rect::from_min_max(egui::pos2(rect.left() + left.size().x + 8.0, rect.center().y - 2.0), egui::pos2(rect.right() - right.size().x - 8.0, rect.center().y + 2.0));
            ui.painter().rect_filled(bar, 2.0, p.selected);
            let done = Rect::from_min_size(bar.min, vec2(bar.width() * elapsed as f32 / total as f32, bar.height()));
            ui.painter().rect_filled(done, 2.0, p.strong);
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }
    // The buttons: a game's own (Discord hands their links only to the
    // game's players, so they show as labels), "Play on Spotify" for a song.
    let button = |ui: &mut egui::Ui, icon: Option<crate::icons::Icon>, label: &str, width: f32, enabled: bool| -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(vec2(width, 32.0), if enabled { Sense::click() } else { Sense::hover() });
        let fill = if enabled && response.hovered() { p.hover } else { p.selected };
        ui.painter().rect_filled(rect, 8.0, fill);
        let color = if enabled { p.strong } else { p.muted };
        let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::strong(14.0), color);
        let icon_width = if icon.is_some() { 22.0 } else { 0.0 };
        let x = rect.center().x - (galley.size().x + icon_width) / 2.0;
        if let Some(icon) = icon {
            crate::icons::paint(ui, Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0)), icon, color);
        }
        ui.painter().galley(egui::pos2(x + icon_width, rect.center().y - galley.size().y / 2.0), galley, color);
        if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
    };
    for label in &activity.buttons {
        ui.add_space(8.0);
        let width = ui.available_width();
        button(ui, None, label, width, false);
    }
    if music {
        ui.add_space(12.0);
        let url = activity.sync_id.as_ref().map(|track| format!("https://open.spotify.com/track/{track}"));
        let width = ui.available_width();
        if button(ui, Some(crate::icons::Icon::Spotify), t!("Play on Spotify"), width, url.is_some()).clicked()
            && let Some(url) = url
        {
            let _ = open::that_detached(url);
        }
    }
}
