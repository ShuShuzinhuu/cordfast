//! The profile popout, as Discord's current one: 300 points wide, the
//! banner with the avatar over its edge and the custom status in a bubble
//! beside it, the name, the username with the badges in a small strip
//! ("+N" for those that do not fit), the bio and what they are doing; for
//! someone else when they joined, their roles, connections, what you share
//! and a box to message them; for you, Discord's two cards of actions. It
//! opens beside what was clicked and closes on a click outside.

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};

use super::{initials, markdown};
use crate::app::{Action, App, ProfileState};
use crate::discord::model::{Id, User, snowflake_ms};
use crate::discord::rest::Profile;
use crate::plugins::Plugin;
use crate::theme;

const WIDTH: f32 = 300.0;
const BANNER: f32 = 105.0;
const AVATAR: f32 = 80.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(view) = app.profile.take() else { return };
    let p = theme::p();
    let screen = ctx.content_rect();
    let id = egui::Id::new("profile");
    // Its height follows what it holds; last frame's keeps it on screen.
    let height = ctx.memory(|m| m.area_rect(id)).map_or(420.0, |r| r.height());
    // From the account panel it sits right over the card; elsewhere beside
    // what was clicked.
    let mut pos = if view.above { pos2(view.at.x, view.at.y - height - 8.0) } else { view.at + vec2(12.0, -60.0) };
    pos.x = pos.x.clamp(screen.left() + 8.0, screen.right() - WIDTH - 8.0);
    pos.y = pos.y.clamp(screen.top() + 8.0, (screen.bottom() - height - 8.0).max(screen.top() + 8.0));
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    let profile = match &view.state {
        ProfileState::Ready(profile) => Some(profile.as_ref().clone()),
        _ => None,
    };
    // The profile's own user has the banner and decoration; the clicked one
    // may be a partial copy from a message.
    let user = profile.as_ref().map_or_else(|| view.user.clone(), |pr| merge(&view.user, &pr.user));
    let (primary, secondary) = profile
        .as_ref()
        .and_then(|pr| pr.theme_colors)
        .map_or((None, None), |(a, b)| (theme::role_color(a), theme::role_color(b)));
    let mine = user.id == app.me_id();
    // Room for the middle once the header, the name and the bottom are in.
    let body_room = (screen.height() - 16.0 - BANNER - 150.0 - if mine { 230.0 } else { 70.0 }).max(80.0);

    let area = egui::Area::new(id)
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            Frame::new()
                .fill(p.rail)
                .stroke(Stroke::new(1.0, secondary.unwrap_or(p.border)))
                .corner_radius(8)
                .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    header(app, ui, &user, profile.as_ref(), primary, view.guild);
                    Frame::new().inner_margin(Margin { left: 16, right: 16, top: 0, bottom: 16 }).show(ui, |ui| {
                        ui.set_width(WIDTH - 32.0);
                        names(app, ui, &user, profile.as_ref(), view.guild);
                        ui.add_space(10.0);
                        ScrollArea::vertical().max_height(body_room).auto_shrink([false, true]).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match (&view.state, profile.as_ref()) {
                                (ProfileState::Loading, _) => {
                                    ui.spinner();
                                }
                                (ProfileState::Failed(error), _) => {
                                    ui.label(RichText::new(error).size(13.0).color(p.muted));
                                }
                                (_, Some(profile)) => about(app, ui, &user, profile, view.guild),
                                _ => {}
                            }
                        });
                        ui.add_space(12.0);
                        actions(app, ui, &user);
                    });
                });
        });
    if ctx.input(|i| i.pointer.any_pressed())
        && let Some(pointer) = ctx.pointer_interact_pos()
        && !area.response.rect.contains(pointer)
        && !egui::Popup::is_any_open(ctx)
    {
        close = true;
    }
    if !close {
        app.profile = Some(view);
    }
}

/// The clicked user with what the profile endpoint knows that a message
/// author may lack (banner, decoration, server tag).
fn merge(clicked: &User, full: &User) -> User {
    let mut user = full.clone();
    if user.username.is_empty() {
        user = clicked.clone();
    }
    if user.decoration.is_none() {
        user.decoration.clone_from(&clicked.decoration);
    }
    if user.tag.is_none() {
        user.tag.clone_from(&clicked.tag);
    }
    user
}

/// The banner, the avatar over its edge with its decoration and status,
/// the custom status in a bubble beside it, and "…" for the user menu.
fn header(app: &mut App, ui: &mut egui::Ui, user: &User, profile: Option<&Profile>, primary: Option<Color32>, guild: Option<Id>) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, BANNER + AVATAR / 2.0 + 8.0), Sense::hover());
    let banner = Rect::from_min_size(rect.min, vec2(WIDTH, BANNER));
    let radius = CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 };
    ui.painter().rect_filled(banner, radius, primary.or_else(|| profile.and_then(|pr| pr.accent).and_then(theme::role_color)).unwrap_or(p.accent));
    if let Some(url) = profile.and_then(|pr| pr.banner.clone())
        && let Some((texture, size)) = app.images.get(&url, 600)
    {
        // Discord fills the banner, cropping what does not fit.
        let scale = (banner.width() / size.x).max(banner.height() / size.y);
        let shown = vec2(banner.width() / (size.x * scale), banner.height() / (size.y * scale));
        let uv = Rect::from_center_size(pos2(0.5, 0.5), shown);
        egui::Image::from_texture((texture, banner.size())).uv(uv).corner_radius(radius).paint_at(ui, banner);
    }
    // The avatar sits on the banner's edge in a ring of the card colour.
    let avatar = Rect::from_min_size(pos2(rect.left() + 16.0, banner.bottom() - AVATAR / 2.0), Vec2::splat(AVATAR));
    ui.painter().circle_filled(avatar.center(), AVATAR / 2.0 + 6.0, p.rail);
    let url = user.avatar_moving(160);
    app.images.circle(ui, avatar, Some(&url), &initials(user.display_name()), AVATAR / 2.0);
    if let Some(decoration) = user.decoration_url()
        && let Some((texture, _)) = app.images.get(&decoration, 240)
    {
        let frame = Rect::from_center_size(avatar.center(), Vec2::splat(AVATAR * 1.2));
        egui::Image::from_texture((texture, frame.size())).paint_at(ui, frame);
    }
    let mine = user.id == app.me_id();
    let status = if mine { Some(app.status.clone()) } else { app.presences.get(&user.id).map(|pr| pr.status.clone()) };
    if let Some(status) = status.filter(|s| s != "offline" && s != "invisible") {
        super::friends::dot(ui, avatar, &status, p.rail);
    }
    if app.voice.speaking.contains(&user.id) {
        ui.painter().circle_stroke(avatar.center(), AVATAR / 2.0 + 3.0, Stroke::new(3.0, p.green));
    }
    // The custom status in a thought bubble beside the avatar; on your own
    // card with none, Discord's invitation to set one.
    let custom = if mine { app.custom_status.clone() } else { app.presences.get(&user.id).and_then(|pr| pr.custom.clone()) };
    if custom.is_some() || mine {
        let text = custom.clone().unwrap_or_else(|| t!("Add Status").into());
        let font = FontId::proportional(13.0);
        let room = rect.right() - avatar.right() - 22.0;
        let mut job = crate::text::script_job(&text, font, if custom.is_some() { p.text } else { p.muted });
        job.wrap = egui::text::TextWrapping::truncate_at_width(room - if custom.is_none() { 38.0 } else { 20.0 });
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let plus = custom.is_none();
        let width = galley.size().x + if plus { 40.0 } else { 22.0 };
        let bubble = Rect::from_min_size(pos2(avatar.right() + 10.0, banner.bottom() - 16.0), vec2(width, 32.0));
        let response = ui.interact(bubble, ui.id().with("status-bubble"), Sense::click());
        let fill = if response.hovered() && mine { p.hover } else { p.sidebar };
        // Two small circles lead from the avatar, as Discord's bubble.
        ui.painter().circle_filled(pos2(bubble.left() - 2.0, bubble.bottom() + 2.0), 5.0, fill);
        ui.painter().circle_filled(pos2(bubble.left() - 9.0, bubble.bottom() + 9.0), 2.5, fill);
        ui.painter().rect_filled(bubble, 16.0, fill);
        let mut x = bubble.left() + 11.0;
        if plus {
            let icon = Rect::from_center_size(pos2(x + 8.0, bubble.center().y), Vec2::splat(16.0));
            ui.painter().circle_filled(icon.center(), 8.0, p.muted);
            crate::icons::paint(ui, icon.shrink(4.0), crate::icons::Icon::Plus, p.sidebar);
            x += 22.0;
        }
        ui.painter().galley(pos2(x, bubble.center().y - galley.size().y / 2.0), galley, p.text);
        if mine && response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.modal = Some(crate::app::Modal::CustomStatus { text: custom.unwrap_or_default(), minutes: Some(1440) });
            app.profile = None;
        }
    }
    // "…" on the banner: the same menu as a right click on them.
    if !mine {
        let more = Rect::from_min_size(pos2(banner.right() - 40.0, banner.top() + 10.0), Vec2::splat(30.0));
        let response = ui.interact(more, ui.id().with("profile-more"), Sense::click());
        ui.painter().circle_filled(more.center(), 15.0, Color32::from_black_alpha(if response.hovered() { 200 } else { 140 }));
        crate::icons::paint(ui, more.shrink(7.0), crate::icons::Icon::More, Color32::WHITE);
        egui::Popup::menu(&response.on_hover_text(t!("More"))).show(|ui| {
            super::menu::begin(ui);
            super::menu::user(app, ui, user, guild);
        });
    }
}

/// The name, then the username (and pronouns, the server tag, APP) with the
/// badges in their strip.
fn names(app: &mut App, ui: &mut egui::Ui, user: &User, profile: Option<&Profile>, guild: Option<Id>) {
    let p = theme::p();
    // Discord's popout writes the name in the header colour wherever it
    // opens: a role's colour (often a grey) belongs to chat and the member
    // list, and made the same card look faded from a server.
    let (name, _) = app.display(guild, user);
    ui.add(egui::Label::new(RichText::new(&name).font(theme::strong(20.0)).color(p.strong)).wrap());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(&user.username).size(14.0).color(p.text));
        if let Some(pronouns) = profile.map(|pr| pr.pronouns.as_str()).filter(|x| !x.is_empty()) {
            ui.label(RichText::new("•").size(12.0).color(p.faint));
            ui.label(RichText::new(pronouns).size(14.0).color(p.muted));
        }
        if let Some((_, tag, _)) = &user.tag {
            tag_pill(app, ui, user, tag);
        }
        if user.bot {
            bot_pill(ui, user.verified_bot());
        }
        if let Some(profile) = profile {
            badges(app, ui, profile);
        }
    });
}

/// The badges in Discord's strip after the username: six at most (fewer if
/// the line is short), then "+N" for the rest, each with its description on
/// hover.
fn badges(app: &mut App, ui: &mut egui::Ui, profile: &Profile) {
    if profile.badges.is_empty() {
        return;
    }
    let p = theme::p();
    const ICON: f32 = 18.0;
    const STEP: f32 = 21.0;
    let room = ui.available_width().max(WIDTH - 32.0);
    let all = profile.badges.len();
    // Discord shows six at most, then "+N" for the rest.
    let mut shown = all.min(6);
    let width = |shown: usize, extra: bool| 8.0 + shown as f32 * STEP + if extra { 28.0 } else { 0.0 };
    while shown > 1 && width(shown, shown < all) > room {
        shown -= 1;
    }
    let (strip, _) = ui.allocate_exact_size(vec2(width(shown, shown < all), 26.0), Sense::hover());
    ui.painter().rect_filled(strip, 8.0, p.sidebar);
    for (i, (description, icon, link)) in profile.badges.iter().take(shown).enumerate() {
        let cell = Rect::from_min_size(pos2(strip.left() + 5.0 + i as f32 * STEP, strip.center().y - ICON / 2.0), Vec2::splat(ICON));
        if let Some((texture, _)) = app.images.get(icon, 48) {
            egui::Image::from_texture((texture, cell.size())).paint_at(ui, cell);
        }
        let response = ui.interact(cell, ui.id().with(("badge", i)), Sense::click()).on_hover_text(description);
        if response.clicked()
            && let Some(link) = link
        {
            let _ = open::that_detached(link);
        }
    }
    if shown < all {
        let rest = Rect::from_min_max(pos2(strip.right() - 30.0, strip.top()), strip.max);
        let names: Vec<&str> = profile.badges[shown..].iter().map(|(d, _, _)| d.as_str()).collect();
        ui.painter().text(rest.center(), Align2::CENTER_CENTER, format!("+{}", all - shown), theme::strong(12.0), p.text);
        ui.interact(rest, ui.id().with("badges-rest"), Sense::hover()).on_hover_text(names.join("\n"));
    }
}

/// The server tag beside a name: the guild's badge and its letters.
pub fn tag_pill(app: &mut App, ui: &mut egui::Ui, user: &User, tag: &str) {
    let (rect, _) = ui.allocate_exact_size(tag_size(ui, user, tag, 18.0), Sense::hover());
    paint_tag(app, ui, rect, user, tag);
}

/// A tag chip's size beside a name in a row `height` tall.
pub fn tag_size(ui: &egui::Ui, user: &User, tag: &str, height: f32) -> Vec2 {
    let text = ui.painter().layout_no_wrap(tag.to_owned(), tag_font(height), Color32::PLACEHOLDER).size().x;
    vec2(text + if user.tag_badge_url().is_some() { height + 4.0 } else { 10.0 }, height)
}

fn tag_font(height: f32) -> FontId {
    theme::strong(if height >= 18.0 { 12.0 } else { 11.0 })
}

/// Discord's tag chip in `rect`: the server's badge and its letters. A
/// click opens the server's card beside it.
pub fn paint_tag(app: &mut App, ui: &mut egui::Ui, rect: Rect, user: &User, tag: &str) {
    let p = theme::p();
    let Some((guild, _, _)) = user.tag else { return };
    // Placed by hand in lists: its spot names it, as one author's tag shows
    // on many messages.
    let response = ui.interact(rect, egui::Id::new(("tag", user.id, rect.min.x as i32, rect.min.y as i32)), Sense::click());
    ui.painter().rect_filled(rect, 4.0, if response.hovered() { p.hover } else { p.selected });
    let mut x = rect.left() + 5.0;
    if let Some(badge) = user.tag_badge_url() {
        let icon = Rect::from_min_size(pos2(x - 1.0, rect.center().y - (rect.height() - 6.0) / 2.0), Vec2::splat(rect.height() - 6.0));
        if let Some((texture, _)) = app.images.get(&badge, 32) {
            egui::Image::from_texture((texture, icon.size())).paint_at(ui, icon);
        }
        x = icon.right() + 3.0;
    }
    let galley = ui.painter().layout_no_wrap(tag.to_owned(), tag_font(rect.height()), p.strong);
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.strong);
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t!("Server Tag")).clicked() {
        app.actions.push(Action::OpenTagCard { guild, at: rect.right_top() });
    }
}

/// A server tag's card, as Discord's: the server's banner and icon, its
/// name and tag, how many are online and in it, since when, what it says
/// of itself, its games and traits, and Join (or Go to Server for one of
/// yours). It closes on a click outside or Esc.
pub fn tag_card(app: &mut App, ctx: &egui::Context) {
    let Some((guild, at)) = app.tag_card.as_ref().map(|c| (c.guild, c.at)) else { return };
    let p = theme::p();
    let screen = ctx.content_rect();
    let id = egui::Id::new("tag-card");
    let height = ctx.memory(|m| m.area_rect(id)).map_or(360.0, |r| r.height());
    let mut pos = at + vec2(8.0, -40.0);
    pos.x = pos.x.clamp(screen.left() + 8.0, screen.right() - WIDTH - 8.0);
    pos.y = pos.y.clamp(screen.top() + 8.0, (screen.bottom() - height - 8.0).max(screen.top() + 8.0));
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    // Lent out for the frame and put back: no copy each frame.
    let state = app.guild_profiles.remove(&guild);
    let area = egui::Area::new(id).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        Frame::new()
            .fill(p.rail)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(8)
            .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) })
            .show(ui, |ui| {
                ui.set_width(WIDTH);
                match &state {
                    Some(Some(Ok(card))) => {
                        if card_body(app, ui, card) {
                            close = true;
                        }
                    }
                    Some(Some(Err(error))) => {
                        Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
                            ui.label(RichText::new(error).size(13.0).color(p.muted));
                        });
                    }
                    _ => {
                        Frame::new().inner_margin(Margin::same(24)).show(ui, |ui| {
                            ui.vertical_centered(|ui| ui.spinner());
                        });
                    }
                }
            });
    });
    if let Some(state) = state {
        app.guild_profiles.insert(guild, state);
    }
    if ctx.input(|i| i.pointer.any_pressed())
        && let Some(pointer) = ctx.pointer_interact_pos()
        && !area.response.rect.contains(pointer)
    {
        close = true;
    }
    if close {
        app.tag_card = None;
    }
}

/// The card's content. Returns whether its button was used.
fn card_body(app: &mut App, ui: &mut egui::Ui, card: &crate::discord::model::GuildProfile) -> bool {
    let p = theme::p();
    const ICON: f32 = 64.0;
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 100.0 + ICON / 2.0), Sense::hover());
    let banner = Rect::from_min_size(rect.min, vec2(WIDTH, 100.0));
    let radius = CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 };
    ui.painter().rect_filled(banner, radius, card.brand.and_then(theme::role_color).unwrap_or(p.accent));
    if let Some(url) = card.banner_url()
        && let Some((texture, size)) = app.images.get(&url, 600)
    {
        let scale = (banner.width() / size.x).max(banner.height() / size.y);
        let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(banner.width() / (size.x * scale), banner.height() / (size.y * scale)));
        egui::Image::from_texture((texture, banner.size())).uv(uv).corner_radius(radius).paint_at(ui, banner);
    }
    // The icon on the banner's edge, a rounded square in a ring of the card.
    let icon = Rect::from_min_size(pos2(rect.left() + 16.0, banner.bottom() - ICON / 2.0), Vec2::splat(ICON));
    ui.painter().rect_filled(icon.expand(4.0), 20.0, p.rail);
    app.images.circle(ui, icon, card.icon_url().as_deref(), &initials(&card.name), 16.0);
    let mut used = false;
    Frame::new().inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 16 }).show(ui, |ui| {
        ui.set_width(WIDTH - 32.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        // The name, cut short, and the tag at the row's end.
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let mut right = row.right();
        if let Some((tag, badge)) = &card.tag {
            let font = tag_font(18.0);
            let text = ui.painter().layout_no_wrap(tag.clone(), font, p.strong);
            let has_badge = !badge.is_empty();
            let chip = Rect::from_min_size(pos2(row.right() - text.size().x - if has_badge { 22.0 } else { 10.0 }, row.center().y - 9.0), vec2(text.size().x + if has_badge { 22.0 } else { 10.0 }, 18.0));
            ui.painter().rect_filled(chip, 4.0, p.selected);
            let mut x = chip.left() + 5.0;
            if let Some(url) = card.badge_url() {
                let at = Rect::from_min_size(pos2(x - 1.0, chip.center().y - 6.0), Vec2::splat(12.0));
                if let Some((texture, _)) = app.images.get(&url, 32) {
                    egui::Image::from_texture((texture, at.size())).paint_at(ui, at);
                }
                x = at.right() + 3.0;
            }
            ui.painter().galley(pos2(x, chip.center().y - text.size().y / 2.0), text, p.strong);
            right = chip.left() - 8.0;
        }
        super::paint_truncated(ui, Rect::from_min_max(row.min, pos2(right, row.bottom())), &card.name, theme::strong(18.0), p.strong);
        // "● 1,650 Online  ● 17,826 Members", as Discord's invites write it.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (dot, text) in [
                (p.green, t!("{number} Online", number = crate::text::count(card.online))),
                (p.muted, tn!(card.members, "{number} Member", "{number} Members", number = crate::text::count(card.members))),
            ] {
                let (at, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().circle_filled(at.center(), 4.0, dot);
                ui.label(RichText::new(text).size(14.0).color(p.muted));
                ui.add_space(6.0);
            }
        });
        ui.label(RichText::new(t!("Est. {date}", date = crate::text::month_year(snowflake_ms(card.id)))).size(14.0).color(p.muted));
        if !card.description.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(&card.description).size(14.0).color(p.text));
        }
        // Its games, once their names are known (asked with their icons).
        for &game in card.games.iter().take(2) {
            let icon = app.app_icon(game);
            let Some(name) = app.app_names.get(&game).cloned() else { continue };
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let (at, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                match icon.and_then(|url| app.images.get(&url, 40)) {
                    Some((texture, _)) => {
                        egui::Image::from_texture((texture, at.size())).corner_radius(4.0).paint_at(ui, at);
                    }
                    None => crate::icons::paint(ui, at, crate::icons::Icon::Controller, p.muted),
                }
                ui.label(RichText::new(name).font(theme::strong(14.0)).color(p.strong));
            });
        }
        if !card.traits.is_empty() {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let shown = card.traits.len().min(4);
                for (emoji, unicode, label) in &card.traits[..shown] {
                    trait_chip(app, ui, *emoji, unicode.as_deref(), label);
                }
                if card.traits.len() > shown {
                    trait_chip(app, ui, None, None, &tn!(card.traits.len() - shown, "+ {count} more", "+ {count} more"));
                }
            });
        }
        ui.add_space(8.0);
        let member = app.guild(card.id).is_some();
        let label = if member { t!("Go to Server") } else { t!("Join") };
        let (button, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
        let fill = if member { p.selected } else { p.green };
        ui.painter().rect_filled(button, 8.0, if response.hovered() { fill.gamma_multiply(0.85) } else { fill });
        ui.painter().text(button.center(), Align2::CENTER_CENTER, label, theme::strong(14.0), Color32::WHITE);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            used = true;
            if member {
                app.actions.push(Action::SelectGuild(Some(card.id)));
            } else {
                app.actions.push(Action::JoinTagGuild(card.id));
            }
        }
    });
    used
}

/// One of a server's traits: its emoji and label in a round chip.
fn trait_chip(app: &mut App, ui: &mut egui::Ui, emoji: Option<Id>, unicode: Option<&str>, label: &str) {
    let p = theme::p();
    let text = match unicode {
        Some(unicode) => format!("{unicode} {label}"),
        None => label.to_owned(),
    };
    let galley = ui.painter().layout_no_wrap(text, FontId::proportional(13.0), p.text);
    let room = if emoji.is_some() { 20.0 } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + room + 20.0, 26.0), Sense::hover());
    ui.painter().rect_stroke(rect, 13.0, Stroke::new(1.0, p.selected), egui::StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    if let Some(id) = emoji {
        let at = Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(16.0));
        if let Some((texture, _)) = app.images.get(&format!("https://cdn.discordapp.com/emojis/{id}.png?size=32"), 32) {
            egui::Image::from_texture((texture, at.size())).paint_at(ui, at);
        }
        x += room;
    }
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.text);
}

/// Discord's APP tag, with a check for a verified bot.
pub fn bot_pill(ui: &mut egui::Ui, verified: bool) {
    let p = theme::p();
    let text = if verified { "✓ APP" } else { "APP" };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::strong(10.5), Color32::WHITE);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 8.0, 16.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, p.accent);
    ui.painter().galley(pos2(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).font(theme::strong(12.0)).color(theme::p().strong));
    ui.add_space(4.0);
}

fn date(text: &str) -> String {
    // "2024-05-07T..." as "May 7, 2024".
    text.get(..10)
        .and_then(|d| d.parse::<jiff::civil::Date>().ok())
        .map(|d| crate::text::short_day(d.to_zoned(jiff::tz::TimeZone::UTC).map(|z| z.timestamp().as_millisecond() as u64).unwrap_or(0)))
        .unwrap_or_default()
}

/// Discord's "Member Since" line: its logo with the account's date, a dot,
/// then the server's icon with the day they joined it. On two lines, without
/// the dot, when one is too narrow, so the card keeps its width.
fn member_since(app: &mut App, ui: &mut egui::Ui, user: &User, profile: &Profile, guild: Option<Id>) {
    let p = theme::p();
    let font = FontId::proportional(13.5);
    let created = ui.painter().layout_no_wrap(crate::text::short_day(snowflake_ms(user.id)), font.clone(), p.text);
    let joined = match (profile.joined_guild.as_deref(), guild.and_then(|g| app.guild(g))) {
        (Some(joined), Some(g)) => Some((ui.painter().layout_no_wrap(date(joined), font, p.text), g.icon_url(), g.name.clone())),
        _ => None,
    };
    let wide = |galley: &egui::Galley| 22.0 + galley.size().x;
    let one_line = joined.as_ref().is_none_or(|(galley, _, _)| wide(&created) + 18.0 + wide(galley) <= ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), if one_line { 20.0 } else { 44.0 }), Sense::hover());
    let first = pos2(rect.left(), rect.top() + 10.0);
    crate::icons::paint(ui, Rect::from_center_size(first + vec2(8.0, 0.0), Vec2::splat(16.0)), crate::icons::Icon::Discord, p.muted);
    let shift = wide(&created) + 18.0;
    ui.painter().galley(pos2(first.x + 22.0, first.y - created.size().y / 2.0), created, p.text);
    if let Some((galley, icon, name)) = joined {
        let second = if one_line { first + vec2(shift, 0.0) } else { first + vec2(0.0, 24.0) };
        if one_line {
            ui.painter().circle_filled(second - vec2(9.0, 0.0), 2.0, p.faint);
        }
        app.images.circle(ui, Rect::from_center_size(second + vec2(8.0, 0.0), Vec2::splat(16.0)), icon.as_deref(), &initials(&name), 8.0);
        ui.painter().galley(pos2(second.x + 22.0, second.y - galley.size().y / 2.0), galley, p.text);
    }
}

/// The middle of the card: the bio, what they are doing and, for someone
/// else, when they joined, their roles, connections and what you share.
fn about(app: &mut App, ui: &mut egui::Ui, user: &User, profile: &Profile, guild: Option<Id>) {
    let p = theme::p();
    let mine = user.id == app.me_id();
    let bio = if profile.guild_bio.is_empty() { &profile.bio } else { &profile.guild_bio };
    if !bio.is_empty() {
        let blocks = markdown::parse(bio);
        markdown::show(app, ui, &blocks, p.text, user.id);
    }
    // Yours come from what this computer reports (the pipe, the detected
    // game), as Discord shows them on your own card too.
    let activities = if mine { app.own_activities.clone() } else { app.presences.get(&user.id).map(|pr| pr.activities.clone()).unwrap_or_default() };
    for activity in &activities {
        ui.add_space(10.0);
        super::activity::card(app, ui, activity);
    }
    if mine {
        return;
    }
    section(ui, t!("Member Since"));
    member_since(app, ui, user, profile, guild);
    if app.plugin(Plugin::FriendsSince)
        && let Some(since) = app.relationships.get(&user.id).filter(|r| r.kind == 1).and_then(|r| r.since.clone())
    {
        section(ui, t!("Friends Since"));
        ui.label(RichText::new(date(&since)).size(13.5).color(p.text));
    }
    if let (Some(guild), Some(member)) = (guild.and_then(|g| app.guild(g)).cloned(), &profile.member) {
        let roles: Vec<(String, u32)> = guild.roles_of(&member.roles).into_iter().map(|r| (r.name.clone(), r.color)).collect();
        if !roles.is_empty() {
            section(ui, t!("Roles"));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (name, color) in roles {
                    role_chip(ui, &name, color);
                }
            });
        }
    }
    if !profile.connections.is_empty() {
        section(ui, t!("Connections"));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (kind, name, verified) in &profile.connections {
                connection_chip(ui, &connection_name(kind), name, *verified);
            }
        });
    }
    mutuals(app, ui, profile);
    // Their volume in our call.
    let in_call = app.voice.channel.is_some_and(|(_, c)| app.channel(c).is_some_and(|ch| app.voice_members(ch).contains(&user.id)));
    if in_call {
        section(ui, t!("User Volume"));
        let max = if app.plugin(Plugin::VolumeBooster) { 4.0 } else { 2.0 };
        let key = user.id.to_string();
        let mut volume = app.settings.user_volumes.get(&key).copied().unwrap_or(1.0);
        if ui.add(egui::Slider::new(&mut volume, 0.0..=max).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() {
            app.settings.user_volumes.insert(key, volume);
            app.actions.push(Action::SettingsChanged);
        }
    }
}

/// A connected account as a compact chip: the service, the name and a
/// check when verified.
fn connection_chip(ui: &mut egui::Ui, service: &str, name: &str, verified: bool) {
    let p = theme::p();
    let service_galley = ui.painter().layout_no_wrap(service.to_owned(), FontId::proportional(12.0), p.muted);
    let name_galley = ui.painter().layout_no_wrap(name.to_owned(), theme::strong(13.0), p.strong);
    let width = (10.0 + service_galley.size().x + 6.0 + name_galley.size().x + if verified { 22.0 } else { 10.0 }).min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    ui.painter().rect(rect, 6.0, p.sidebar, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    ui.painter().galley(pos2(x, rect.center().y - service_galley.size().y / 2.0), service_galley.clone(), p.muted);
    x += service_galley.size().x + 6.0;
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - if verified { 20.0 } else { 6.0 }, rect.bottom()));
    super::paint_truncated(ui, clip, name, theme::strong(13.0), p.strong);
    if verified {
        crate::icons::paint(ui, Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), Vec2::splat(13.0)), crate::icons::Icon::Check, p.green);
    }
}

/// "N Mutual Friends • N Mutual Servers", which opens the list it
/// names under it, as Discord's popout leads to them.
fn mutuals(app: &mut App, ui: &mut egui::Ui, profile: &Profile) {
    let p = theme::p();
    let servers = profile.mutual_guilds.iter().filter(|g| app.guild(**g).is_some()).count();
    let friends = profile.mutual_friend_list.len();
    if servers == 0 && friends == 0 {
        return;
    }
    let id = ui.id().with("profile-mutuals");
    let mut open: u8 = ui.data(|d| d.get_temp(id)).unwrap_or(0);
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Each part whole: one that does not fit moves to the next line
        // rather than leaving "comum" alone under it.
        let mut link = |ui: &mut egui::Ui, text: String, which: u8| {
            let color = if open == which { p.strong } else { p.muted };
            let label = egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap_mode(egui::TextWrapMode::Extend).sense(Sense::click());
            let response = ui.add(label).on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                open = if open == which { 0 } else { which };
            }
        };
        if friends > 0 {
            link(ui, tn!(friends, "{count} Mutual Friend", "{count} Mutual Friends"), 1);
        }
        if friends > 0 && servers > 0 {
            ui.label(RichText::new("•").size(13.0).color(p.faint));
        }
        if servers > 0 {
            link(ui, tn!(servers, "{count} Mutual Server", "{count} Mutual Servers"), 2);
        }
    });
    ui.data_mut(|d| d.insert_temp(id, open));
    match open {
        1 => mutual_friends(app, ui, profile),
        2 => mutual_servers(app, ui, profile),
        _ => {}
    }
}

/// A role as Discord shows it in a profile: a coloured dot and the name.
fn role_chip(ui: &mut egui::Ui, name: &str, color: u32) {
    let p = theme::p();
    let galley = ui.painter().layout_no_wrap(name.to_owned(), FontId::proportional(12.5), p.text);
    let width = (galley.size().x + 24.0).min(ui.available_width().max(60.0));
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, p.rail);
    ui.painter().circle_filled(pos2(rect.left() + 10.0, rect.center().y), 5.0, theme::role_color(color).unwrap_or(p.muted));
    let text = Rect::from_min_max(pos2(rect.left() + 19.0, rect.top()), pos2(rect.right() - 4.0, rect.bottom()));
    super::paint_truncated(ui, text, name, FontId::proportional(12.5), p.text);
}

fn connection_name(kind: &str) -> String {
    match kind {
        "twitter" => "X".into(),
        "youtube" => "YouTube".into(),
        "twitch" => "Twitch".into(),
        "spotify" => "Spotify".into(),
        "steam" => "Steam".into(),
        "github" => "GitHub".into(),
        "tiktok" => "TikTok".into(),
        "instagram" => "Instagram".into(),
        "reddit" => "Reddit".into(),
        "roblox" => "Roblox".into(),
        "xbox" => "Xbox".into(),
        "playstation" => "PlayStation".into(),
        "battlenet" => "Battle.net".into(),
        "riotgames" => "Riot Games".into(),
        "leagueoflegends" => "League of Legends".into(),
        "epicgames" => "Epic Games".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn mutual_servers(app: &mut App, ui: &mut egui::Ui, profile: &Profile) {
    let p = theme::p();
    let guilds: Vec<(crate::discord::model::Id, Option<String>, String, Option<String>)> = profile
        .mutual_guilds
        .iter()
        .filter_map(|id| {
            let g = app.guild(*id)?;
            let nick = profile.mutual_nicks.iter().find(|(gid, _)| gid == id).and_then(|(_, n)| n.clone());
            Some((*id, g.icon_url(), g.name.clone(), nick))
        })
        .collect();
    if guilds.is_empty() {
        ui.label(RichText::new(t!("No servers in common.")).size(13.5).color(p.muted));
    }
    for (id, icon, name, nick) in guilds {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        let avatar = Rect::from_min_size(pos2(rect.left() + 6.0, rect.center().y - 16.0), Vec2::splat(32.0));
        app.images.circle(ui, avatar, icon.as_deref(), &initials(&name), 10.0);
        let x = avatar.right() + 10.0;
        let name_y = if nick.is_some() { rect.center().y - 8.0 } else { rect.center().y };
        super::paint_truncated(ui, Rect::from_min_max(pos2(x, name_y - 9.0), pos2(rect.right() - 6.0, name_y + 9.0)), &name, theme::strong(14.0), p.strong);
        if let Some(nick) = nick {
            super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.center().y + 2.0), pos2(rect.right() - 6.0, rect.center().y + 18.0)), &nick, FontId::proportional(12.5), p.muted);
        }
        if response.clicked() {
            app.actions.push(Action::SelectGuild(Some(id)));
            app.profile = None;
        }
    }
}

fn mutual_friends(app: &mut App, ui: &mut egui::Ui, profile: &Profile) {
    let p = theme::p();
    if profile.mutual_friend_list.is_empty() {
        ui.label(RichText::new(t!("No friends in common.")).size(13.5).color(p.muted));
    }
    for friend in &profile.mutual_friend_list {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        let avatar = Rect::from_min_size(pos2(rect.left() + 6.0, rect.center().y - 16.0), Vec2::splat(32.0));
        app.images.circle(ui, avatar, Some(&friend.avatar_url()), &initials(friend.display_name()), 16.0);
        if let Some(presence) = app.presences.get(&friend.id).filter(|pr| pr.status != "offline") {
            super::friends::dot(ui, avatar, &presence.status, if response.hovered() { p.hover } else { p.sidebar });
        }
        let x = avatar.right() + 10.0;
        super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.center().y - 17.0), pos2(rect.right() - 6.0, rect.center().y + 1.0)), friend.display_name(), theme::strong(14.0), p.strong);
        super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.center().y + 1.0), pos2(rect.right() - 6.0, rect.center().y + 17.0)), &friend.username, FontId::proportional(12.5), p.muted);
        if response.clicked() {
            let at = rect.left_top();
            app.actions.push(Action::OpenProfile { user: friend.clone(), guild: None, at });
        }
    }
}

/// The bottom of the card: for you, Discord's two cards of actions; for
/// someone else, the box to message them.
fn actions(app: &mut App, ui: &mut egui::Ui, user: &User) {
    let p = theme::p();
    let card = |ui: &mut egui::Ui, content: &mut dyn FnMut(&mut egui::Ui)| {
        Frame::new().fill(p.sidebar).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            content(ui);
        });
    };
    if user.id == app.me_id() {
        card(ui, &mut |ui| {
            if super::menu_row(ui, t!("Edit Profile"), Some(crate::icons::Icon::Pencil), None, false, p.text).clicked() {
                app.actions.push(Action::OpenSettings(crate::app::SettingsTab::Profile));
                app.profile = None;
            }
            super::me_menu(app, ui);
        });
        ui.add_space(8.0);
        card(ui, &mut |ui| super::me_accounts(app, ui, user));
        return;
    }
    // Discord's box: "Message @name", Enter opens the DM and sends.
    let id = ui.id().with(("profile-message", user.id));
    let mut text: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let response = Frame::new().fill(p.composer).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::symmetric(12, 9)).show(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut text)
                .frame(Frame::NONE)
                .desired_width(f32::INFINITY)
                .hint_text(RichText::new(t!("Message @{name}", name = user.username)).color(p.faint))
                .text_color(p.text),
        )
    });
    let sent = response.inner.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if sent && !text.trim().is_empty() {
        app.actions.push(Action::SendDm { user: user.id, text: text.trim().to_owned() });
        text.clear();
    }
    ui.data_mut(|d| d.insert_temp(id, text));
}
