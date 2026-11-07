//! Discord's full profile (`user-profile-modal-v2`, "View Full Profile"):
//! a 962 × 800 window, the profile card on the left (a 140-point banner,
//! the 120-point avatar, the name, the actions, Member Since, Friends Since
//! and the note only you see) and tabs on the right: Activity, the mutual
//! friends and the mutual servers.

use egui::{Color32, CornerRadius, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{Action, App};
use crate::discord::model::{User, snowflake_ms};
use crate::icons::{self, Icon};
use crate::theme;

/// The window's state while open.
pub struct FullProfile {
    pub user: User,
    /// 0 Activity, 1 mutual friends, 2 mutual servers.
    pub tab: u8,
    /// The note as typed, and whether Discord's copy has come.
    pub note: String,
    pub note_loaded: bool,
}

impl FullProfile {
    pub fn new(user: User) -> Self {
        Self { user, tab: 0, note: String::new(), note_loaded: false }
    }
}

/// Opens it, asking for the profile and the note.
pub fn open(app: &mut App, user: User) {
    if !app.profiles.contains_key(&user.id) && app.requested_profiles.insert(user.id) {
        app.commands.send(crate::backend::Command::Profile { user: user.id, guild: None });
    }
    app.commands.send(crate::admin::get(&format!("note:{}", user.id), format!("/users/@me/notes/{}", user.id)));
    app.profile = None;
    app.full_profile = Some(FullProfile::new(user));
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.full_profile.take() else { return };
    let p = theme::p();
    let mut keep = !ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let screen = ctx.content_rect();
    egui::Area::new("full-profile-dim".into()).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(170));
        if response.clicked() {
            keep = false;
        }
    });
    let size = vec2(962.0f32.min(screen.width() - 40.0), 800.0f32.min(screen.height() - 40.0));
    let window = Rect::from_center_size(screen.center(), size);
    egui::Area::new("full-profile".into()).order(egui::Order::Foreground).fixed_pos(window.min).show(ctx, |ui| {
        let (frame, _) = ui.allocate_exact_size(size, Sense::click());
        ui.painter().rect(frame, 16.0, p.chat, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
        // Close, at the top right.
        let close = Rect::from_min_size(pos2(frame.right() - 40.0, frame.top() + 8.0), Vec2::splat(32.0));
        let response = ui.interact(close, ui.id().with("full-profile-close"), Sense::click());
        ui.painter().rect_filled(close, 8.0, if response.hovered() { p.hover } else { p.hover.gamma_multiply(0.6) });
        icons::paint(ui, close.shrink(8.0), Icon::Close, p.strong);
        if response.clicked() {
            keep = false;
        }
        let card = Rect::from_min_max(frame.min + vec2(48.0, 48.0), pos2(frame.left() + 448.0, frame.bottom()));
        let right = Rect::from_min_max(pos2(card.right(), frame.top() + 48.0), pos2(frame.right() - 32.0, frame.bottom()));
        left_card(app, ui, card, &mut state);
        tabs(app, ui, right, &mut state);
    });
    if keep {
        app.full_profile = Some(state);
    } else if state.note_loaded {
        // The note is saved as the window closes, as Discord saves it when
        // the box loses the keyboard.
        save_note(app, &state);
    }
}

fn save_note(app: &mut App, state: &FullProfile) {
    let saved = app.notes.get(&state.user.id).cloned().unwrap_or_default();
    if state.note.trim() != saved.trim() {
        app.notes.insert(state.user.id, state.note.trim().to_owned());
        app.commands.send(crate::backend::Command::Api {
            key: "note".into(),
            method: "PUT",
            path: format!("/users/@me/notes/{}", state.user.id),
            body: Some(serde_json::json!({ "note": state.note.trim() })),
        });
    }
}

fn left_card(app: &mut App, ui: &mut egui::Ui, card: Rect, state: &mut FullProfile) {
    let p = theme::p();
    let profile = app.profiles.get(&state.user.id).cloned();
    let user = profile.as_ref().map_or_else(|| state.user.clone(), |pr| super::profile::merge(&state.user, &pr.user));
    let top = CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 };
    ui.painter().rect_filled(card, top, p.sidebar);
    // The banner: the picture, else the profile's colour.
    let banner = Rect::from_min_size(card.min, vec2(card.width(), 140.0));
    let colour = profile.as_ref().and_then(|pr| pr.theme_colors.map(|c| c.0).or(pr.accent)).and_then(theme::role_color).unwrap_or(p.hover);
    ui.painter().rect_filled(banner, top, colour);
    if let Some(url) = profile.as_ref().and_then(|pr| pr.banner.clone())
        && let Some((texture, size)) = app.images.get(&url, 800)
    {
        let scale = (banner.width() / size.x).max(banner.height() / size.y);
        let shown = vec2(banner.width() / (size.x * scale), banner.height() / (size.y * scale));
        egui::Image::from_texture((texture, banner.size())).uv(Rect::from_center_size(pos2(0.5, 0.5), shown)).corner_radius(top).paint_at(ui, banner);
    }
    // The avatar over the banner's edge, in a ring of the card's colour,
    // with its decoration and status.
    let avatar = Rect::from_min_size(pos2(card.left() + 32.0, banner.bottom() - 60.0), Vec2::splat(120.0));
    ui.painter().circle_filled(avatar.center(), 66.0, p.sidebar);
    app.images.circle(ui, avatar, Some(&user.avatar_moving(240)), &super::initials(user.display_name()), 60.0);
    if let Some(decoration) = user.decoration_url()
        && let Some((texture, _)) = app.images.get(&decoration, 288)
    {
        let frame = Rect::from_center_size(avatar.center(), Vec2::splat(144.0));
        egui::Image::from_texture((texture, frame.size())).paint_at(ui, frame);
    }
    if let Some(presence) = app.presences.get(&user.id).filter(|pr| pr.status != "offline") {
        super::friends::dot(ui, avatar, &presence.status, p.sidebar);
    }
    let inner = Rect::from_min_max(pos2(card.left() + 32.0, avatar.bottom() + 20.0), pos2(card.right() - 32.0, card.bottom() - 16.0));
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut content;
    ScrollArea::vertical().id_salt("full-profile-left").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(inner.width());
        // The name in 24-point bold, the username and the badges.
        ui.label(RichText::new(user.display_name()).font(theme::strong(24.0)).color(p.strong));
        super::profile::names_line(app, ui, &user, profile.as_deref());
        ui.add_space(16.0);
        // Message, the relationship and More.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let galley = ui.painter().layout_no_wrap(t!("Message").to_owned(), theme::strong(14.0), Color32::WHITE);
            let (button, response) = ui.allocate_exact_size(vec2(galley.size().x + 44.0, 32.0), Sense::click());
            ui.painter().rect_filled(button, 8.0, if response.hovered() { p.accent.linear_multiply(0.85) } else { p.accent });
            icons::paint(ui, Rect::from_center_size(pos2(button.left() + 20.0, button.center().y), Vec2::splat(16.0)), Icon::Chat, Color32::WHITE);
            ui.painter().galley(pos2(button.left() + 32.0, button.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::MessageUser(user.id));
                app.full_profile = None;
            }
            let square = |ui: &mut egui::Ui, icon: Icon, tip: &str| {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                ui.painter().rect_filled(rect, 8.0, if response.hovered() { p.hover } else { p.hover.gamma_multiply(0.6) });
                icons::paint(ui, rect.shrink(8.0), icon, p.strong);
                response.on_hover_text(tip)
            };
            let friend = app.relationships.get(&user.id).map(|r| r.kind);
            // No friendship with an app, nor while blocked.
            let relation = match friend {
                _ if user.bot => None,
                Some(1) => Some(square(ui, Icon::UserCheck, t!("Friend"))),
                Some(2) => None,
                _ => Some(square(ui, Icon::UserPlus, t!("Add Friend"))),
            };
            if let Some(relation) = relation {
                let target = user.clone();
                if friend == Some(1) {
                    egui::Popup::menu(&relation).show(|ui| {
                        super::menu::begin(ui);
                        if super::menu::danger(ui, t!("Remove Friend"), super::menu::Tail::None).clicked() {
                            app.modal = Some(super::menu::remove_friend(&target));
                            ui.close();
                        }
                    });
                } else if relation.clicked() {
                    app.actions.push(Action::Relationship { user: target.id, kind: Some(1) });
                }
            }
            let more = square(ui, Icon::More, t!("More"));
            let target = user.clone();
            egui::Popup::menu(&more).show(|ui| {
                super::menu::begin(ui);
                super::menu::user(app, ui, &target, None);
            });
        });
        ui.add_space(24.0);
        // Member Since, Friends Since: 12-point headings, 14-point dates.
        let heading = |ui: &mut egui::Ui, text: &str| {
            ui.label(RichText::new(text).font(theme::strong(12.0)).color(p.muted));
            ui.add_space(6.0);
        };
        heading(ui, t!("Member Since"));
        ui.label(RichText::new(crate::text::short_day(snowflake_ms(user.id))).size(14.0).color(p.text));
        ui.add_space(24.0);
        if let Some(since) = app.relationships.get(&user.id).filter(|r| r.kind == 1).and_then(|r| r.since.clone()) {
            heading(ui, t!("Friends Since"));
            let day = since.get(..10).and_then(|d| d.parse::<jiff::civil::Date>().ok()).and_then(|d| d.to_zoned(jiff::tz::TimeZone::UTC).ok()).map(|z| crate::text::short_day(z.timestamp().as_millisecond() as u64)).unwrap_or_default();
            ui.label(RichText::new(day).size(14.0).color(p.text));
            ui.add_space(24.0);
        }
        // The note only you see, written in place.
        heading(ui, t!("Note (only visible to you)"));
        if !state.note_loaded
            && let Some(note) = app.notes.get(&user.id)
        {
            state.note = note.clone();
            state.note_loaded = true;
        }
        let response = ui.add(egui::TextEdit::multiline(&mut state.note).frame(egui::Frame::NONE).char_limit(256).desired_rows(1).hint_text(t!("Click to add a note")).desired_width(f32::INFINITY));
        if response.lost_focus() && state.note_loaded {
            save_note(app, state);
        }
    });
}

fn tabs(app: &mut App, ui: &mut egui::Ui, area: Rect, state: &mut FullProfile) {
    let p = theme::p();
    let profile = app.profiles.get(&state.user.id).cloned();
    let friends = profile.as_ref().map_or(0, |pr| pr.mutual_friend_list.len());
    let servers = profile.as_ref().map_or(0, |pr| pr.mutual_guilds.iter().filter(|g| app.guild(**g).is_some()).count());
    // The tab bar: 14-point semibold, 20 apart, the open one underlined.
    let labels = [t!("Activity").to_owned(), tn!(friends, "{count} Mutual Friend", "{count} Mutual Friends"), tn!(servers, "{count} Mutual Server", "{count} Mutual Servers")];
    let mut x = area.left() + 32.0;
    let bar_y = area.top() + 20.0;
    for (i, label) in labels.iter().enumerate() {
        let open = state.tab == i as u8;
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::strong(14.0), p.strong);
        let rect = Rect::from_min_size(pos2(x, bar_y - 15.0), vec2(galley.size().x, 30.0));
        let response = ui.interact(rect, ui.id().with(("profile-tab", i)), Sense::click());
        let color = if open || response.hovered() { p.strong } else { p.muted };
        ui.painter().galley(pos2(x, bar_y - galley.size().y / 2.0), galley.clone(), color);
        if open {
            ui.painter().rect_filled(Rect::from_min_size(pos2(x, rect.bottom() - 2.0), vec2(galley.size().x, 2.0)), 1.0, p.accent);
        }
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            state.tab = i as u8;
        }
        x += galley.size().x + 20.0;
    }
    ui.painter().hline(area.x_range(), bar_y + 15.0, Stroke::new(1.0, p.border));
    let panel = Rect::from_min_max(pos2(area.left() + 32.0, bar_y + 35.0), pos2(area.right() - 16.0, area.bottom() - 16.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(panel).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut child;
    ScrollArea::vertical().id_salt(("full-profile-tab", state.tab)).auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(panel.width());
        match (state.tab, &profile) {
            (0, _) => {
                let activities = app.presences.get(&state.user.id).map(|pr| pr.activities.clone()).unwrap_or_default();
                let voice = app.user_voice_channel(state.user.id).is_some();
                if activities.is_empty() && !voice {
                    return;
                }
                ui.label(RichText::new(t!("Current activity")).font(theme::strong(12.0)).color(p.muted));
                ui.add_space(8.0);
                super::chat::voice_card_for(app, ui, state.user.id);
                for activity in &activities {
                    super::activity::card(app, ui, activity);
                    ui.add_space(8.0);
                }
            }
            (1, Some(profile)) => super::profile::mutual_friends(app, ui, profile),
            (2, Some(profile)) => super::profile::mutual_servers(app, ui, profile),
            _ => {
                ui.spinner();
            }
        }
    });
}
