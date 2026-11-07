//! The open conversation: header, call bar, messages, composer, forums and
//! the image viewer.

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Label, Layout, Margin, Modifiers,
    Rect, RichText, ScrollArea, Sense, Shape, Stroke, Vec2, pos2, vec2,
};

use super::{initials, markdown};
use crate::app::{Action, App, Viewer};
use crate::discord::model::{Channel, ChannelKind, Id, Message};
use crate::plugins::Plugin;
use crate::{text, theme};

const AVATAR: f32 = 40.0;
const GUTTER: f32 = 56.0;
/// Messages from one author closer than this share a header.
const GROUP_MS: u64 = 7 * 60 * 1000;
const QUICK_REACTIONS: [&str; 8] = ["👍", "❤️", "😂", "😮", "😢", "🔥", "🎉", "👀"];

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let Some(channel_id) = app.selected_channel else {
        if app.selected_guild.is_none() {
            match app.home {
                crate::app::Home::Friends => super::friends::show(app, ui),
                crate::app::Home::MessageRequests => super::requests::show(app, ui),
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(t!("Pick a channel  ·  Ctrl+K to search")).color(p.muted));
            });
        }
        return;
    };
    let Some(channel) = app.channel(channel_id).cloned() else { return };
    if matches!(channel.kind, ChannelKind::Voice | ChannelKind::Stage) && !app.voice_chat {
        voice_view(app, ui, &channel);
        return;
    }
    header(app, ui, &channel);
    if channel.kind == ChannelKind::Forum {
        forum(app, ui, &channel);
        return;
    }
    let right = if app.side.is_some() {
        Some(egui::Panel::right("side")
            .exact_size(380.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar))
            .show(ui, |ui| side(app, ui, &channel)))
    } else if channel.kind == ChannelKind::Dm
        && app.show_dm_profile
        && let Some(user) = channel.recipients.first().cloned()
    {
        Some(egui::Panel::right("dm-profile")
            .exact_size(340.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar))
            .show(ui, |ui| dm_profile(app, ui, &user)))
    } else if channel.guild_id.is_some() && app.show_members && app.member_list.is_some() {
        Some(egui::Panel::right("members")
            .exact_size(240.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar))
            .show(ui, |ui| member_list(app, ui)))
    } else {
        None
    };
    if let Some(right) = right {
        super::seam(ui, right.response.rect, super::Side::Left, p.sidebar, p.chat);
    }
    if channel.guild_id.is_none() && (app.calls.contains_key(&channel.id) || app.voice.channel.map(|(_, c)| c) == Some(channel.id)) {
        call_bar(app, ui, &channel);
    }
    egui::Panel::bottom("composer")
        .show_separator_line(false)
        .frame(Frame::new().fill(p.chat).inner_margin(Margin { left: 16, right: 16, top: 0, bottom: 6 }))
        .show(ui, |ui| composer(app, ui, &channel));
    messages(app, ui, channel_id);
}

/// The overlay's Text Chat: the conversation open in the app, or else the
/// call's. Its own ids, so its scroll stays apart from the main window's.
pub fn overlay_widget(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let picked = app.selected_channel.or_else(|| app.voice.channel.map(|(_, channel)| channel));
    let Some(channel) = picked.and_then(|id| app.channel(id).cloned()) else {
        ui.painter().text(ui.max_rect().center(), egui::Align2::CENTER_CENTER, t!("Pick a channel"), egui::FontId::proportional(13.0), p.muted);
        return;
    };
    if app.selected_channel.is_none() {
        app.actions.push(Action::SelectChannel(channel.id));
    }
    ui.push_id("overlay-chat", |ui| {
        egui::Panel::bottom("overlay-composer")
            .show_separator_line(false)
            .frame(Frame::new().inner_margin(Margin { left: 8, right: 8, top: 0, bottom: 6 }))
            .show(ui, |ui| composer(app, ui, &channel));
        messages(app, ui, channel.id);
    });
}

fn header(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let p = theme::p();
    let dm = channel.guild_id.is_none();
    Frame::new().inner_margin(Margin::symmetric(16, 0)).show(ui, |ui| {
        ui.set_height(48.0);
        ui.horizontal_centered(|ui| {
            let icon = match channel.kind {
                ChannelKind::Dm | ChannelKind::GroupDm => None,
                ChannelKind::Thread => Some(crate::icons::Icon::Thread),
                ChannelKind::Forum => Some(crate::icons::Icon::Forum),
                ChannelKind::Announcement => Some(crate::icons::Icon::Announcements),
                _ => Some(crate::icons::Icon::Text),
            };
            match icon {
                Some(icon) => {
                    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                    crate::icons::paint(ui, rect, icon, p.faint);
                }
                None => {
                    ui.label(RichText::new("@").size(20.0).color(p.faint));
                }
            }
            ui.add(Label::new(RichText::new(channel.title()).font(theme::strong(16.0)).color(p.strong)).truncate());
            // An announcement channel: Discord's "Follow" beside its name.
            if channel.kind == ChannelKind::Announcement {
                ui.add_space(4.0);
                if follow_button(ui, 28.0) {
                    app.modal = Some(crate::app::Modal::Follow { channel: channel.id, guild: None, target: None });
                }
            }
            if channel.kind == ChannelKind::Dm && channel.recipients.first().is_some_and(|u| app.user_voice_channel(u.id).is_some()) {
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                crate::icons::paint(ui, rect, crate::icons::Icon::Voice, p.green);
                ui.label(RichText::new(t!("In voice")).size(14.0).color(p.text));
            }
            if let Some(topic) = &channel.topic {
                let (bar, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                ui.painter().vline(bar.center().x, bar.y_range(), Stroke::new(1.0, p.selected));
                let first = topic.lines().next().unwrap_or_default();
                let width = (ui.available_width() - 200.0).max(60.0);
                ui.add_sized(vec2(width, 20.0), Label::new(RichText::new(first).size(13.5).color(p.muted)).truncate())
                    .on_hover_text(topic);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if matches!(channel.kind, ChannelKind::Voice | ChannelKind::Stage) && crate::icons::button(ui, crate::icons::Icon::Chat, 28.0, t!("Hide Chat")).clicked() {
                    app.voice_chat = false;
                }
                // Discord's header, right to left: the search box ("Search
                // SkinsRestorer"), the member list, pins, the notification
                // bell and threads. The inbox and help are in the title bar.
                let searching = matches!(app.side, Some(crate::app::Side::Search { .. }));
                let hint = match channel.guild_id.and_then(|g| app.guild(g)) {
                    Some(guild) => t!("Search {guildName}", guildName = guild.name),
                    None => t!("Search").to_owned(),
                };
                let width = (ui.available_width() * 0.3).clamp(144.0, 244.0);
                let (rect, response) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
                ui.painter().rect(rect, 8.0, p.rail, Stroke::new(1.0, if searching || response.hovered() { p.selected } else { p.border }), egui::StrokeKind::Inside);
                let glass = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(16.0));
                crate::icons::paint(ui, glass, crate::icons::Icon::Search, p.muted);
                super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 10.0, rect.top()), pos2(glass.left() - 6.0, rect.bottom())), &hint, egui::FontId::proportional(14.0), p.muted);
                if response.on_hover_cursor(egui::CursorIcon::Text).clicked() {
                    app.toggle_search();
                }
                ui.add_space(8.0);
                if !dm {
                    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    let color = if app.show_members || response.hovered() { p.strong } else { p.muted };
                    crate::icons::paint(ui, rect.shrink(3.0), crate::icons::Icon::Members, color);
                    if response.on_hover_text(if app.show_members { t!("Hide Member List") } else { t!("Show Member List") }).clicked() {
                        app.toggle_members();
                    }
                }
                let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                let pins = matches!(app.side, Some(crate::app::Side::Pins(_)));
                crate::icons::paint(ui, rect.shrink(3.0), crate::icons::Icon::Pin, if pins || response.hovered() { p.strong } else { p.muted });
                if response.on_hover_text(t!("Pinned Messages")).clicked() {
                    app.toggle_pins();
                }
                // The bell: crossed out while the channel is muted; it opens
                // the channel's mute and notification menus.
                if let Some(guild) = channel.guild_id {
                    let muted = app.is_muted(channel);
                    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    let icon = if muted { crate::icons::Icon::BellSlash } else { crate::icons::Icon::Bell };
                    crate::icons::paint(ui, rect.shrink(3.0), icon, if response.hovered() { p.strong } else { p.muted });
                    let response = response.on_hover_text(t!("Notification Settings"));
                    egui::Popup::menu(&response).show(|ui| {
                        super::menu::begin(ui);
                        super::menu::mute(app, ui, super::menu::Muted::Channel, Some(guild), Some(channel.id));
                        super::menu::notifications(app, ui, guild, Some(channel));
                    });
                }
                if !dm && channel.kind != crate::discord::model::ChannelKind::Thread {
                    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    crate::icons::paint(ui, rect.shrink(3.0), crate::icons::Icon::Thread, if response.hovered() { p.strong } else { p.muted });
                    if response.on_hover_text(t!("Threads")).clicked() {
                        app.archived_threads = None;
                        app.commands.send(crate::admin::get("threads-archived", format!("/channels/{}/threads/archived/public?limit=25", channel.id)));
                        app.modal = Some(crate::app::Modal::Threads { channel: channel.id, name: String::new() });
                    }
                }
                if dm {
                    let in_call = app.voice.channel.map(|(_, c)| c) == Some(channel.id);
                    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    let color = if response.hovered() { p.strong } else { p.muted };
                    if !in_call {
                        crate::icons::paint(ui, rect.shrink(3.0), crate::icons::Icon::PhoneCall, color);
                    }
                    if !in_call && response.on_hover_text(t!("Start Voice Call")).clicked() {
                        app.actions.push(Action::JoinVoice { guild: None, channel: channel.id, ring: true });
                    }
                    if channel.kind == ChannelKind::Dm
                        && let Some(user) = channel.recipients.first().cloned()
                    {
                        let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                        let color = if app.show_dm_profile || response.hovered() { p.strong } else { p.muted };
                        crate::icons::paint(ui, rect.shrink(3.0), crate::icons::Icon::Friends, color);
                        if response.on_hover_text(if app.show_dm_profile { t!("Hide User Profile") } else { t!("Show User Profile") }).clicked() {
                            app.show_dm_profile = !app.show_dm_profile;
                        }
                        let _ = user;
                    }
                }
            });
        });
    });
    let line = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.border));
}

/// Discord's secondary "Follow" button, `height` tall.
fn follow_button(ui: &mut egui::Ui, height: f32) -> bool {
    let p = theme::p();
    let galley = ui.painter().layout_no_wrap(t!("Follow").to_owned(), theme::strong(14.0), p.strong);
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 24.0, height), Sense::click());
    ui.painter().rect_filled(rect, 8.0, if response.hovered() { p.hover.gamma_multiply(1.6) } else { p.selected });
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.strong);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The strip over a DM with a call: who is in it, join or leave, and the
/// voice region (lowest measured latency first).
fn call_bar(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let p = theme::p();
    let members = app.voice_members(channel);
    let in_call = app.voice.channel.map(|(_, c)| c) == Some(channel.id);
    Frame::new().fill(p.rail).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                let width = members.len() as f32 * 64.0;
                ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                for user in &members {
                    let Some(u) = app.users.get(user).cloned() else { continue };
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::click());
                    app.images.circle(ui, rect, Some(&u.avatar_url()), &initials(u.display_name()), 28.0);
                    if app.voice.speaking.contains(user) {
                        ui.painter().circle_stroke(rect.center(), 30.0, Stroke::new(3.0, p.green));
                    }
                    if response.on_hover_text(u.display_name()).clicked() {
                        app.actions.push(Action::OpenProfile { user: u, guild: None, at: rect.left_bottom() });
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 330.0).max(0.0) / 2.0);
                if in_call {
                    if ui.add(egui::Button::new(RichText::new(t!("Leave Call")).color(Color32::WHITE)).fill(p.red)).clicked() {
                        app.actions.push(Action::LeaveVoice);
                    }
                } else if ui.add(egui::Button::new(RichText::new(t!("Join Call")).color(Color32::WHITE)).fill(p.green)).clicked() {
                    app.actions.push(Action::JoinVoice { guild: None, channel: channel.id, ring: false });
                }
                region_picker(app, ui, channel.id);
            });
        });
    });
}

pub fn region_picker(app: &mut App, ui: &mut egui::Ui, channel: Id) {
    let current = app
        .calls
        .get(&channel)
        .and_then(|c| c.region.clone())
        .unwrap_or_else(|| t!("automatic").into());
    let regions = app.voice.regions.clone();
    egui::ComboBox::from_id_salt(("region", channel))
        .selected_text(t!("Region: {region}", region = current))
        .width(200.0)
        .show_ui(ui, |ui| {
            if let Some((best, ms)) = regions.first()
                && ui.selectable_label(false, t!("Best route ({region}, {ms} ms)", region = best, ms = ms)).clicked()
            {
                app.actions.push(Action::SetRegion(best.clone()));
            }
            for (region, ms) in &regions {
                if ui.selectable_label(current == *region, format!("{region} — {ms} ms")).clicked() {
                    app.actions.push(Action::SetRegion(region.clone()));
                }
            }
            if regions.is_empty() {
                ui.label(t!("Measuring latency…"));
            }
        });
}

fn messages(app: &mut App, ui: &mut egui::Ui, channel: Id) {
    let p = theme::p();
    let me = app.me_id();
    let guild = app.channel(channel).and_then(|c| c.guild_id);
    let my_roles: Vec<Id> = guild.and_then(|g| app.guild(g)).map(|g| g.my_roles.clone()).unwrap_or_default();
    let hide_blocked = app.plugin(Plugin::NoBlockedMessages);
    // Borrowed out of the history for the frame instead of copied, and put
    // back at the end: nothing adds messages while views draw.
    let (list, loading, complete, error) = match app.histories.get_mut(&channel) {
        Some(h) => (std::mem::take(&mut h.messages), h.loading, h.complete, h.error.clone()),
        None => (Vec::new(), true, false, None),
    };
    let mut area = ScrollArea::vertical()
        .id_salt(("messages", channel))
        .stick_to_bottom(true)
        .auto_shrink(false);
    if let Some((c, y)) = app.scroll_to
        && c == channel
    {
        // Not stuck to the bottom this frame, or egui puts it back there.
        area = area.vertical_scroll_offset(y).stick_to_bottom(false);
        app.scroll_to = None;
    }
    let mut unread_count = 0usize;
    let mut first_unread_at: Option<(f32, u64)> = None;
    let mut jump_found = false;
    let output = area.show(ui, |ui| {
            let width = ui.available_width();
            ui.set_width(width);
            ui.add_space(8.0);
            if let Some(error) = &error {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(t!("Couldn't load: {error}", error = error)).color(p.red));
                    if ui.button(t!("Try Again")).clicked() {
                        app.actions.push(Action::LoadOlder(channel));
                    }
                });
            } else if loading {
                ui.vertical_centered(|ui| ui.spinner());
            } else if complete {
                ui.add_space(16.0);
                Frame::new().inner_margin(Margin::symmetric(16, 0)).show(ui, |ui| {
                    ui.label(RichText::new(t!("This is the beginning of the conversation.")).color(p.muted));
                });
            }
            let clip = ui.clip_rect();
            let mut previous_message: Option<&Message> = None;
            let unread_from = app.unread_from.get(&channel).copied();
            let jump = app.jump.as_ref().filter(|j| j.channel == channel).map(|j| (j.message, j.at.elapsed().as_secs_f32()));
            for message in &list {
                if unread_from.is_some_and(|from| message.id > from) {
                    unread_count += 1;
                }
                let target = jump.is_some_and(|(id, _)| id == message.id);
                // A line with the date where the day changes, and Discord's
                // red "NEW" line where unread messages begin.
                let day = text::day(message.timestamp_ms());
                let previous = previous_message;
                let new_day = previous.is_none_or(|p| text::day(p.timestamp_ms()) != day);
                let first_unread = unread_from.is_some_and(|from| {
                    message.id > from && previous.is_none_or(|p| p.id <= from)
                });
                if first_unread {
                    first_unread_at = Some((ui.cursor().top(), message.timestamp_ms()));
                }
                if new_day || first_unread {
                    ui.add_space(14.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
                    let y = rect.center().y;
                    let x = rect.x_range();
                    let color = if first_unread { p.red } else { p.selected };
                    ui.painter().hline(egui::Rangef::new(x.min + 16.0, x.max - 16.0), y, Stroke::new(1.0, color));
                    if new_day {
                        let galley = ui.painter().layout_no_wrap(day.clone(), theme::strong(12.0), if first_unread { p.red } else { p.muted });
                        let bg = Rect::from_center_size(rect.center(), galley.size() + vec2(12.0, 2.0));
                        ui.painter().rect_filled(bg, 0.0, p.chat);
                        ui.painter().galley(bg.min + vec2(6.0, 1.0), galley, p.muted);
                    }
                    if first_unread {
                        let tag = Rect::from_min_size(egui::pos2(x.max - 56.0, y - 7.0), vec2(40.0, 14.0));
                        ui.painter().rect_filled(tag, egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 }, p.red);
                        ui.painter().text(tag.center(), Align2::CENTER_CENTER, t!("New").to_uppercase(), theme::strong(10.0), Color32::WHITE);
                    }
                }
                let previous = if new_day || first_unread { None } else { previous };
                let known = matches!(message.kind, 0..=12 | 18..=24 | 27..=31 | 46 | 63 | 67);
                if !known && message.content.is_empty() && message.attachments.is_empty() && message.embeds.is_empty() {
                    continue;
                }
                let blocked = app.relationships.get(&message.author.id).is_some_and(|r| r.kind == 2);
                if blocked && hide_blocked {
                    continue;
                }
                let grouped = previous.is_some_and(|p| {
                    p.author.id == message.author.id
                        && message.reply.is_none()
                        && message.interaction.is_none()
                        && message.timestamp_ms().saturating_sub(p.timestamp_ms()) < GROUP_MS
                });
                let _ = previous;
                previous_message = Some(message);
                // Off-screen messages take their measured height undrawn.
                let top = ui.cursor().top();
                if let Some(&(w, h)) = app.heights.get(&message.id)
                    && (w - width).abs() < 1.0
                    && (top + h < clip.top() - 300.0 || top > clip.bottom() + 300.0)
                    && !target
                {
                    ui.allocate_space(vec2(width, h));
                    continue;
                }
                let mentioned = message.author.id != me
                    && (message.mention_everyone
                        || message.mentions.iter().any(|u| u.id == me)
                        || message.mention_roles.iter().any(|r| my_roles.contains(r)));
                if blocked {
                    ui.add_space(6.0);
                    Frame::new().inner_margin(Margin::symmetric(16 + GUTTER as i8, 2)).show(ui, |ui| {
                        ui.label(RichText::new(t!("Message from someone you blocked")).italics().color(p.faint));
                    });
                } else {
                    // A jumped-to message lights up, then fades, as in Discord.
                    let light = jump.filter(|_| target).map_or(0.0, |(_, age)| (1.0 - (age - 1.0).max(0.0) / 1.5).clamp(0.0, 1.0));
                    message_view(app, ui, message, guild, grouped, mentioned, light);
                }
                let height = ui.cursor().top() - top;
                app.heights.insert(message.id, (width, height));
                if target {
                    jump_found = true;
                    if let Some(jump) = app.jump.as_mut().filter(|j| !j.scrolled) {
                        jump.scrolled = true;
                        ui.scroll_to_rect(Rect::from_min_size(pos2(ui.max_rect().left(), top), vec2(width, height)), Some(Align::Center));
                    }
                }
            }
            ui.add_space(12.0);
        });
    // Near the top: ask for older messages, remembering how far the reader
    // is from the bottom, which the older messages do not change.
    let offset = output.state.offset.y;
    let height = output.content_size.y;
    let area = output.inner_rect;
    // A jump to a message not loaded yet reads older pages until it comes
    // (up to ten); a found one fades out and is done.
    if let Some(jump) = app.jump.as_mut().filter(|j| j.channel == channel) {
        let age = jump.at.elapsed().as_secs_f32();
        if jump_found {
            if age > 2.5 {
                app.jump = None;
            } else {
                ui.ctx().request_repaint();
            }
        } else if !loading && error.is_none() {
            if complete || jump.pages >= 10 {
                app.notify(t!("Couldn't find the original message."));
                app.jump = None;
            } else {
                jump.pages += 1;
                jump.at = std::time::Instant::now();
                app.actions.push(Action::LoadOlder(channel));
            }
        }
    }
    // Shift+PgUp: straight to the oldest unread message.
    if let Some((first_y, _)) = first_unread_at
        && crate::keybinds::pressed(ui, &app.settings, crate::keybinds::Shortcut::OldestUnread)
    {
        app.scroll_to = Some((channel, (offset + first_y - area.top() - 48.0).max(0.0)));
    }
    // Discord's bar over the messages: how many are new since when, a click
    // to go to the first, and "Mark As Read".
    if let Some((first_y, since)) = first_unread_at
        && unread_count > 0
        && first_y < area.top()
    {
        let bar = Rect::from_min_size(area.min + vec2(16.0, 0.0), vec2(area.width() - 32.0, 26.0));
        ui.painter().rect_filled(bar, CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 }, p.accent);
        let text = tn!(unread_count, "{count} new message since {time}", "{count} new messages since {time}", time = text::clock(since));
        ui.painter().text(bar.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, text, theme::strong(13.0), Color32::WHITE);
        let read = Rect::from_min_max(pos2(bar.right() - 150.0, bar.top()), bar.max);
        ui.painter().text(read.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, t!("Mark As Read"), theme::strong(13.0), Color32::WHITE);
        // The whole bar first, then "Mark As Read" on top of it.
        let jump = ui.interact(bar, ui.id().with("new-messages-bar"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let mark = ui.interact(read, ui.id().with("mark-read-bar"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if mark.clicked() {
            app.actions.push(Action::MarkRead(vec![channel]));
        } else if jump.clicked() {
            app.scroll_to = Some((channel, (offset + first_y - area.top() - 48.0).max(0.0)));
        }
    }
    // Far from the newest: "You're Viewing Older Messages".
    let from_bottom = height - offset - area.height();
    if from_bottom > area.height() * 1.5 {
        let bar = Rect::from_min_size(pos2(area.left() + 16.0, area.bottom() - 34.0), vec2(area.width() - 32.0, 30.0));
        ui.painter().rect_filled(bar, 8.0, p.rail);
        ui.painter().text(bar.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, t!("You're Viewing Older Messages"), FontId::proportional(13.0), p.text);
        ui.painter().text(bar.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, t!("Jump To Present"), theme::strong(13.0), p.link);
        if ui.interact(bar, ui.id().with("jump-present"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.scroll_to = Some((channel, height));
        }
    }
    if offset < 300.0 && !loading && !complete && error.is_none() && !list.is_empty() && app.scroll_anchor.is_none() {
        app.scroll_anchor = Some((channel, height - offset, false));
        app.actions.push(Action::LoadOlder(channel));
    }
    if let Some((c, from_bottom, arrived)) = app.scroll_anchor
        && c == channel
        && arrived
        && !loading
    {
        app.scroll_to = Some((channel, (height - from_bottom).max(0.0)));
        app.scroll_anchor = None;
        ui.ctx().request_repaint();
    }
    if let Some(h) = app.histories.get_mut(&channel) {
        let added = std::mem::replace(&mut h.messages, list);
        h.messages.extend(added);
    }
}

fn message_view(app: &mut App, ui: &mut egui::Ui, message: &Message, guild: Option<Id>, grouped: bool, mentioned: bool, light: f32) {
    let p = theme::p();
    let compact = app.settings.compact;
    if !grouped {
        ui.add_space(if compact { 4.0 } else { 14.0 });
    }
    let background = ui.painter().add(Shape::Noop);
    let top = ui.cursor().top();
    let left = ui.max_rect().left();
    let width = ui.available_width();
    let gutter = if compact || !app.settings.show_avatars { 16.0 } else { GUTTER + 16.0 };
    let content_width = (width - gutter - 16.0).max(120.0);
    let (name, name_color) = app.display(guild, &message.author);
    let system = !matches!(message.kind, 0 | 19 | 20 | 23);
    // The message's own area goes in before its content, with last frame's
    // height, so the name, the reply line and links stay clickable: egui
    // gives a click to the widget added last.
    let gap = if grouped { 0.0 } else if compact { 4.0 } else { 14.0 };
    let message_id = ui.id().with(("message", message.id));
    let early = app
        .heights
        .get(&message.id)
        .filter(|(w, _)| (*w - width).abs() < 1.0)
        .map(|(_, h)| ui.interact(Rect::from_min_size(pos2(left, top), vec2(width, (h - gap).max(1.0))), message_id, Sense::click()));

    Frame::new()
        .inner_margin(Margin { left: gutter as i8, right: 16, top: 2, bottom: 2 })
        .show(ui, |ui| {
            ui.set_width(content_width);
            if system {
                ui.label(RichText::new(system_text(message, &name, app.me_id(), &app.channel(message.channel_id).map(Channel::title).unwrap_or_default())).italics().color(p.muted));
                return;
            }
            if let Some((user, command)) = &message.interaction {
                super::commands::used_line(app, ui, guild, user, command);
            }
            if let Some((author, quoted)) = &message.reply {
                // Discord's reply line: a curved spine, a small avatar, the
                // name in its role colour, and the first line of the text.
                let quoted = text::resolve(quoted, |k, id| app.name_for(k, id));
                let line: String = quoted.lines().next().unwrap_or("").chars().take(120).collect();
                let (name, color) = match &message.reply_author {
                    Some(user) => app.display(guild, user),
                    None => (author.clone(), None),
                };
                ui.horizontal(|ui| {
                    let start = ui.cursor().left_center();
                    let spine = [
                        egui::pos2(start.x - 34.0, start.y + 10.0),
                        egui::pos2(start.x - 34.0, start.y + 2.0),
                        egui::pos2(start.x - 6.0, start.y + 2.0),
                    ];
                    ui.painter().add(egui::Shape::line(spine.to_vec(), Stroke::new(2.0, p.selected)));
                    if let Some(user) = &message.reply_author {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        app.images.circle(ui, rect, Some(&user.avatar_url()), &initials(&name), 8.0);
                    }
                    let who = ui.add(Label::new(RichText::new(format!("@{name}")).size(13.0).font(theme::strong(13.0)).color(color.unwrap_or(p.muted))).sense(Sense::click()));
                    let what = ui.add(Label::new(RichText::new(line).size(13.0).color(p.muted)).truncate().sense(Sense::click()));
                    // A click goes to the answered message, as in Discord.
                    if let Some(id) = message.reply_id
                        && (who.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() || what.on_hover_cursor(egui::CursorIcon::PointingHand).clicked())
                    {
                        app.actions.push(Action::JumpTo { channel: message.channel_id, message: id });
                    }
                });
            }
            if !grouped {
                ui.horizontal(|ui| {
                    let response = match app.gradient(guild, message.author.id) {
                        Some((from, to)) => ui.add(Label::new(theme::gradient_job(&name, theme::strong(15.0), from, to)).sense(Sense::click())),
                        None => {
                            let name_text = RichText::new(&name).font(theme::strong(15.0)).color(name_color.unwrap_or(p.strong));
                            ui.add(Label::new(name_text).sense(Sense::click()))
                        }
                    };
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        let at = ui.ctx().pointer_latest_pos().unwrap_or_default();
                        app.actions.push(Action::OpenProfile { user: message.author.clone(), guild, at });
                    }
                    // Right click on the name: Discord's user menu.
                    response.context_menu(|ui| {
                        super::menu::begin(ui);
                        super::menu::user(app, ui, &message.author, guild);
                    });
                    if app.plugin(Plugin::ShowMeYourName) && name != message.author.username {
                        ui.label(RichText::new(format!("({})", message.author.username)).size(13.0).color(p.faint));
                    }
                    if let Some((_, tag, _)) = message.author.tag.clone() {
                        super::profile::tag_pill(app, ui, &message.author, &tag);
                    }
                    if message.author.bot {
                        super::profile::bot_pill(ui, message.author.verified_bot());
                    }
                    // The whole date on hover, as Discord's tooltip.
                    ui.label(RichText::new(text::time(message.timestamp_ms())).size(12.0).color(p.faint)).on_hover_text(text::full(message.timestamp_ms()));
                });
            }
            let color = if message.deleted {
                p.red
            } else if message.pending {
                p.faint
            } else {
                p.text
            };
            for old in &message.edits {
                ui.label(RichText::new(old).size(13.5).strikethrough().color(p.faint));
            }
            if message.forwarded {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("↪").color(p.faint));
                    ui.label(RichText::new(t!("Forwarded")).italics().size(13.0).color(p.muted));
                });
            }
            if app.editing == Some(message.id) {
                edit_box(app, ui);
            } else if !message.content.is_empty() {
                let blocks = markdown::parse(&message.content);
                markdown::show(app, ui, &blocks, color, message.id);
                if message.edited {
                    ui.label(RichText::new(t!("(edited)")).size(11.0).color(p.faint));
                }
            }
            if message.deleted {
                ui.label(RichText::new(t!("(deleted)")).size(11.0).color(p.red));
            }
            // Settings → Appearance → Messages, the account's own.
            use crate::discord::preloaded as pre;
            let inline = app.preloaded.flag(pre::INLINE_UPLOADS);
            let spoilers_open = app.preloaded.text(pre::RENDER_SPOILERS).as_deref() == Some("ALWAYS");
            for attachment in &message.attachments {
                let video = attachment.content_type.as_deref().is_some_and(|t| t.starts_with("video/"));
                if !inline && (video || attachment.is_image()) {
                    Frame::new().fill(p.sidebar).corner_radius(6).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (icon, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                            crate::icons::paint(ui, icon, crate::icons::Icon::File, p.muted);
                            ui.hyperlink_to(RichText::new(&attachment.filename).color(p.link), &attachment.url);
                        });
                    });
                } else if video {
                    let (w, h) = (attachment.width.unwrap_or(400), attachment.height.unwrap_or(225));
                    video_view(app, ui, &attachment.proxy_url, &attachment.url, w, h);
                } else if attachment.content_type.as_deref().is_some_and(|t| t.starts_with("audio/")) {
                    // Audio and voice messages (flag 8192) play here.
                    Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                            ui.painter().circle_filled(rect.center(), 16.0, if response.hovered() { p.accent.linear_multiply(0.85) } else { p.accent });
                            let c = rect.center();
                            ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -7.0), c + vec2(7.0, 0.0), c + vec2(-4.0, 7.0)], Color32::WHITE, Stroke::NONE));
                            if response.on_hover_text(t!("Play")).clicked() {
                                crate::sounds::play_url(attachment.url.clone(), app.settings.output_device.clone(), app.settings.output_volume.min(1.0));
                            }
                            let name = if message.flags & 8192 != 0 { t!("Voice Message").to_owned() } else { attachment.filename.clone() };
                            ui.hyperlink_to(RichText::new(name).color(p.link), &attachment.url);
                        });
                    });
                } else if attachment.is_image() {
                    let (w, h) = (attachment.width.unwrap_or(400), attachment.height.unwrap_or(300));
                    let key = (message.id, usize::MAX - attachment.filename.len());
                    if attachment.filename.starts_with("SPOILER_") && !spoilers_open && !app.revealed.contains(&key) {
                        // Spoilers stay covered until clicked, as in Discord.
                        let scale = (400.0 / w.max(1) as f32).min(300.0 / h.max(1) as f32).min(1.0);
                        let size = vec2(w as f32 * scale, h as f32 * scale);
                        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                        ui.painter().rect_filled(rect, 6.0, p.rail);
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, t!("Spoiler").to_uppercase(), theme::strong(14.0), p.strong);
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            app.revealed.insert(key);
                        }
                    } else {
                        picture(app, ui, &attachment.proxy_url, &attachment.url, w, h, 400.0, 300.0, Some(message));
                    }
                } else {
                    Frame::new().fill(p.sidebar).corner_radius(6).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (icon, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                            crate::icons::paint(ui, icon, crate::icons::Icon::File, p.muted);
                            ui.hyperlink_to(RichText::new(&attachment.filename).color(p.link), &attachment.url);
                        });
                    });
                }
            }
            for (id, name, format) in &message.stickers {
                sticker(app, ui, *id, name, *format);
            }
            if let Some(poll) = &message.poll {
                poll_view(app, ui, message, poll);
            }
            // Discord's order: the embeds, then the message's buttons.
            if app.preloaded.flag(pre::RENDER_EMBEDS) {
                for embed in &message.embeds {
                    embed_view(app, ui, embed, message);
                }
            }
            if !message.components.is_empty() {
                super::components::show(app, ui, message);
            }
            if app.preloaded.flag(pre::RENDER_REACTIONS) {
                reactions(app, ui, message);
            }
        });

    let rect = Rect::from_min_max(pos2(left, top), pos2(left + width, ui.cursor().top()));
    let response = early.unwrap_or_else(|| ui.interact(rect, message_id, Sense::click()));
    // The hover bar sits on its own layer, above the message: while the
    // pointer is on it the message no longer "contains" it. The bar's last
    // rectangle, with egui's topmost layer under the pointer, keeps both
    // the bar and the highlight, or they blink on and off under the pointer.
    let bar_id = ui.id().with(("toolbar", message.id));
    let bar_rect = ui.data(|d| d.get_temp::<Rect>(bar_id));
    let on_bar = ui.input(|i| i.pointer.hover_pos()).zip(bar_rect).is_some_and(|(at, bar)| {
        bar.contains(at) && ui.ctx().layer_id_at(at) == Some(egui::LayerId::new(egui::Order::Foreground, bar_id))
    });
    let hovered = ui.rect_contains_pointer(rect) || on_bar;
    if light > 0.0 {
        ui.painter().set(background, Shape::rect_filled(rect, 0.0, p.accent.linear_multiply(0.25 * light)));
    } else if mentioned {
        ui.painter().set(background, Shape::rect_filled(rect, 0.0, p.mention));
        ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, p.warning);
    } else if hovered {
        ui.painter().set(background, Shape::rect_filled(rect, 0.0, p.message_hover));
    }
    let show_avatar = !compact && app.settings.show_avatars;
    if grouped || !show_avatar {
        if hovered && grouped && show_avatar {
            let clock = ui.painter().text(pos2(left + 16.0 + GUTTER / 2.0 - 4.0, top + 12.0), Align2::CENTER_CENTER, text::clock(message.timestamp_ms()), FontId::proportional(11.0), p.faint);
            ui.interact(clock, ui.id().with(("clock", message.id)), Sense::hover()).on_hover_text(text::full(message.timestamp_ms()));
        }
    } else if !system {
        let offset = if message.reply.is_some() { 20.0 } else { 2.0 };
        let avatar = Rect::from_min_size(pos2(left + 16.0, top + offset), Vec2::splat(AVATAR));
        // An animated avatar moves under the pointer, as Discord's.
        let url = if ui.rect_contains_pointer(avatar) { message.author.avatar_moving(64) } else { message.author.avatar_url() };
        app.images.circle(ui, avatar, Some(&url), &initials(&name), AVATAR / 2.0);
        if let Some(decoration) = message.author.decoration_url() {
            crate::icons::decoration(ui, &mut app.images, avatar, &decoration);
        }
        let face = ui.interact(avatar, ui.id().with(("avatar", message.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if face.clicked() {
            app.actions.push(Action::OpenProfile { user: message.author.clone(), guild, at: avatar.right_top() });
        }
        face.context_menu(|ui| {
            super::menu::begin(ui);
            super::menu::user(app, ui, &message.author, guild);
        });
    }

    // Hover toolbar, as Discord's: three quick reactions, then react,
    // reply (or edit, on yours), forward and "…" for the whole menu; with
    // Shift held, mark unread, copy the ID and delete instead of "…".
    let more_id = egui::Id::new(("message-more", message.id));
    let more_open = egui::Popup::is_id_open(ui.ctx(), more_id);
    let toolbar_top = (top - 14.0).max(ui.clip_rect().top() + 4.0);
    if (hovered || more_open) && !message.pending && !message.deleted && !system {
        let shift = ui.input(|i| i.modifiers.shift);
        let actions = MessageActions::of(app, message, guild);
        let bar = egui::Area::new(bar_id)
            .pivot(Align2::RIGHT_TOP)
            .fixed_pos(pos2(left + width - 16.0, toolbar_top))
            .order(egui::Order::Foreground)
            // At once, as Discord's; a fade restarts on every show.
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                Frame::new().fill(p.chat).stroke(Stroke::new(1.0, p.selected)).corner_radius(8).inner_margin(Margin::same(2)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for emoji in QUICK_REACTIONS.iter().take(3) {
                            if ui.add(egui::Button::new(*emoji).frame(false).min_size(vec2(32.0, 30.0))).on_hover_text(t!("Click to react")).clicked() {
                                app.actions.push(Action::React { channel: message.channel_id, message: message.id, emoji: (*emoji).into(), add: true });
                            }
                        }
                        let (line, _) = ui.allocate_exact_size(vec2(9.0, 30.0), Sense::hover());
                        ui.painter().vline(line.center().x, line.y_range().shrink(7.0), Stroke::new(1.0, p.selected));
                        let icon = |ui: &mut egui::Ui, icon: crate::icons::Icon, tip: &str| crate::icons::button(ui, icon, 30.0, tip).clicked();
                        if icon(ui, crate::icons::Icon::Reaction, t!("Add Reaction")) {
                            app.toggle_expressions(crate::app::ExpressionTab::Emoji, crate::app::PickerTarget::Reaction { channel: message.channel_id, message: message.id }, 0.0);
                        }
                        if actions.edit {
                            if icon(ui, crate::icons::Icon::Pencil, t!("Edit")) {
                                app.actions.push(Action::StartEdit { id: message.id, content: message.content.clone() });
                            }
                        } else if icon(ui, crate::icons::Icon::Reply, t!("Reply")) {
                            app.actions.push(Action::Reply { id: message.id, author: name.clone() });
                        }
                        if icon(ui, crate::icons::Icon::Forward, t!("Forward")) {
                            app.modal = Some(forward_modal(message, guild));
                        }
                        if shift {
                            if actions.edit && icon(ui, crate::icons::Icon::Reply, t!("Reply")) {
                                app.actions.push(Action::Reply { id: message.id, author: name.clone() });
                            }
                            if icon(ui, crate::icons::Icon::Chat, t!("Mark Unread")) {
                                app.actions.push(Action::MarkUnread { channel: message.channel_id, message: message.id });
                            }
                            if actions.delete {
                                let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                                if response.hovered() {
                                    ui.painter().rect_filled(rect, 4.0, p.red.linear_multiply(0.2));
                                }
                                crate::icons::paint(ui, rect.shrink(6.0), crate::icons::Icon::Trash, p.red);
                                if response.on_hover_text(t!("Delete")).clicked() {
                                    app.actions.push(Action::Delete { channel: message.channel_id, id: message.id });
                                }
                            }
                        } else {
                            let more = crate::icons::button(ui, crate::icons::Icon::More, 30.0, t!("More"));
                            egui::Popup::menu(&more).id(more_id).align(egui::RectAlign::LEFT_START).show(|ui| {
                                message_menu(app, ui, message, guild, &name, &actions);
                            });
                        }
                    });
                });
            });
        ui.data_mut(|d| d.insert_temp(bar_id, bar.response.rect));
    } else if bar_rect.is_some() {
        ui.data_mut(|d| d.remove::<Rect>(bar_id));
    }

    let mine = message.author.id == app.me_id();
    if response.double_clicked() && app.plugin(Plugin::MessageClickActions) && !message.pending {
        if mine {
            app.actions.push(Action::StartEdit { id: message.id, content: message.content.clone() });
        } else {
            app.actions.push(Action::Reply { id: message.id, author: name.clone() });
        }
    }
    if !message.pending && !system {
        let actions = MessageActions::of(app, message, guild);
        response.context_menu(|ui| message_menu(app, ui, message, guild, &name, &actions));
    }
}

/// A call whose answer only matters when it fails.
fn call(method: &'static str, path: String, body: Option<serde_json::Value>) -> crate::backend::Command {
    crate::backend::Command::Api { key: "pin".into(), method, path, body }
}

/// What you may do with a message here.
struct MessageActions {
    edit: bool,
    delete: bool,
    pin: bool,
    /// Manage Messages: other people's reactions and embeds.
    manage: bool,
}

impl MessageActions {
    fn of(app: &App, message: &Message, guild: Option<Id>) -> Self {
        const MANAGE_MESSAGES: u64 = 1 << 13;
        // Split from Manage Messages in 2025, as the bundle's pin button checks.
        const PIN_MESSAGES: u64 = 1 << 51;
        let mine = message.author.id == app.me_id();
        let perms = guild.and_then(|g| app.guild(g)).map(|g| {
            let perms = app.channel(message.channel_id).map_or_else(|| g.my_permissions(app.me_id()), |c| g.channel_permissions(app.me_id(), c));
            perms
        });
        let manage = perms.is_some_and(|perms| perms & MANAGE_MESSAGES != 0);
        Self {
            edit: mine && matches!(message.kind, 0 | 19) && !message.deleted,
            delete: (mine || manage) && !message.deleted,
            pin: guild.is_none() || perms.is_some_and(|perms| perms & PIN_MESSAGES != 0),
            manage,
        }
    }
}

/// Discord's "Forward" for a message.
fn forward_modal(message: &Message, guild: Option<Id>) -> crate::app::Modal {
    crate::app::Modal::Forward { channel: message.channel_id, guild, message: message.id, picked: Vec::new(), filter: String::new(), note: String::new() }
}

/// The message's menu, on right click and behind "…", as Discord's.
fn message_menu(app: &mut App, ui: &mut egui::Ui, message: &Message, guild: Option<Id>, name: &str, actions: &MessageActions) {
    let p = theme::p();
    super::menu::begin(ui);
    let mine = message.author.id == app.me_id();
    // The quick reactions in a row at the top.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for emoji in QUICK_REACTIONS.iter().take(4) {
            let (rect, response) = ui.allocate_exact_size(vec2(44.0, 40.0), Sense::click());
            ui.painter().rect_filled(rect, 8.0, if response.hovered() { p.hover } else { p.sidebar });
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, *emoji, FontId::proportional(20.0), p.text);
            if response.clicked() {
                app.actions.push(Action::React { channel: message.channel_id, message: message.id, emoji: (*emoji).into(), add: true });
                ui.close();
            }
        }
    });
    ui.add_space(4.0);
    super::menu::submenu(ui, t!("Add Reaction"), |ui| {
        for emoji in QUICK_REACTIONS {
            let label = emojis::get(emoji).and_then(emojis::Emoji::shortcode).map_or_else(|| (*emoji).to_owned(), |code| format!("{emoji}  :{code}:"));
            if super::menu::item(ui, &label, super::menu::Tail::None).clicked() {
                app.actions.push(Action::React { channel: message.channel_id, message: message.id, emoji: (*emoji).into(), add: true });
                ui.close();
            }
        }
        super::menu::separator(ui);
        if super::menu::item(ui, t!("View More"), super::menu::Tail::Icon(crate::icons::Icon::Reaction)).clicked() {
            app.toggle_expressions(crate::app::ExpressionTab::Emoji, crate::app::PickerTarget::Reaction { channel: message.channel_id, message: message.id }, 0.0);
            ui.close();
        }
    });
    super::menu::separator(ui);
    use super::menu::{Tail, item};
    use crate::icons::Icon;
    if actions.edit && item(ui, t!("Edit Message"), Tail::Icon(Icon::Pencil)).clicked() {
        app.actions.push(Action::StartEdit { id: message.id, content: message.content.clone() });
        ui.close();
    }
    if item(ui, t!("Reply"), Tail::Icon(Icon::Reply)).clicked() {
        app.actions.push(Action::Reply { id: message.id, author: name.to_owned() });
        ui.close();
    }
    if item(ui, t!("Forward"), Tail::Icon(Icon::Forward)).clicked() {
        app.modal = Some(forward_modal(message, guild));
        ui.close();
    }
    if !message.reactions.is_empty() && item(ui, t!("View Reactions"), Tail::Icon(Icon::Reaction)).clicked() {
        let emojis: Vec<_> = message.reactions.iter().map(|r| r.emoji.clone()).collect();
        ask_reactors(app, message.channel_id, message.id, &emojis[0]);
        app.modal = Some(crate::app::Modal::Reactions { channel: message.channel_id, message: message.id, emojis, picked: 0, manage: actions.manage });
        ui.close();
    }
    if guild.is_some() && item(ui, t!("Create Thread"), Tail::Icon(Icon::Thread)).clicked() {
        let name: String = message.content.chars().take(40).collect();
        app.modal = Some(crate::app::Modal::NewThread {
            channel: message.channel_id,
            message: message.id,
            name: if name.trim().is_empty() { t!("New Thread").into() } else { name },
        });
        ui.close();
    }
    let saved_path = format!("/users/@me/saved-messages/{}/{}", message.channel_id, message.id);
    let bookmarked = app.saved.contains(&message.id);
    if item(ui, if bookmarked { t!("Remove Bookmark") } else { t!("Bookmark Message") }, Tail::None).clicked() {
        if bookmarked {
            app.saved.remove(&message.id);
            app.commands.send(call("DELETE", saved_path.clone(), None));
        } else {
            app.saved.insert(message.id);
            app.commands.send(call("PUT", saved_path.clone(), Some(serde_json::json!({ "due_at": null }))));
        }
        ui.close();
    }
    super::menu::submenu(ui, t!("Remind Me"), |ui| {
        for (label, hours) in [(t!("In 1 hour"), 1), (t!("In 3 hours"), 3), (t!("Tomorrow"), 24)] {
            if super::menu::item(ui, label, Tail::None).clicked() {
                let due = jiff::Timestamp::now().checked_add(jiff::SignedDuration::from_hours(hours)).map(|at| at.to_string()).unwrap_or_default();
                app.commands.send(call("PUT", saved_path.clone(), Some(serde_json::json!({ "due_at": due }))));
                app.notify(t!("Reminder added."));
                ui.close();
            }
        }
    });
    if !message.content.is_empty() && item(ui, t!("Copy Text"), Tail::None).clicked() {
        ui.ctx().copy_text(message.content.clone());
        ui.close();
    }
    if actions.pin {
        let pin_label = if message.pinned { t!("Unpin Message") } else { t!("Pin Message") };
        if item(ui, pin_label, Tail::Icon(Icon::Pin)).clicked() {
            let method = if message.pinned { "DELETE" } else { "PUT" };
            app.commands.send(crate::backend::Command::Api {
                key: "pin".into(),
                method,
                path: format!("/channels/{}/messages/pins/{}", message.channel_id, message.id),
                body: None,
            });
            ui.close();
        }
    }
    // Announcements go out to the channels that follow them.
    let announcing = app.channel(message.channel_id).is_some_and(|c| c.kind == crate::discord::model::ChannelKind::Announcement);
    if announcing && message.flags & 1 == 0 && (mine || actions.manage) && item(ui, t!("Publish"), Tail::None).clicked() {
        app.commands.send(call("POST", format!("/channels/{}/messages/{}/crosspost", message.channel_id, message.id), None));
        ui.close();
    }
    if !message.embeds.is_empty() && message.flags & 4 == 0 && (mine || actions.manage) && item(ui, t!("Remove Embeds"), Tail::None).clicked() {
        app.commands.send(call("PATCH", format!("/channels/{}/messages/{}", message.channel_id, message.id), Some(serde_json::json!({ "flags": message.flags | 4 }))));
        ui.close();
    }
    if !message.reactions.is_empty() && actions.manage && item(ui, t!("Remove All Reactions"), Tail::None).clicked() {
        app.modal = Some(crate::app::Modal::Confirm {
            title: t!("Remove All Reactions").into(),
            body: t!("Are you sure you want to remove all the reactions from this message?").into(),
            button: t!("Remove All Reactions").into(),
            then: crate::app::Confirmed::RemoveAllReactions { channel: message.channel_id, message: message.id },
        });
        ui.close();
    }
    super::menu::separator(ui);
    if !mine && item(ui, t!("Mark Unread"), Tail::None).clicked() {
        app.actions.push(Action::MarkUnread { channel: message.channel_id, message: message.id });
        ui.close();
    }
    if item(ui, t!("Copy Message Link"), Tail::None).clicked() {
        let guild = guild.map_or_else(|| "@me".to_owned(), |g| g.to_string());
        ui.ctx().copy_text(format!("https://discord.com/channels/{guild}/{}/{}", message.channel_id, message.id));
        ui.close();
    }
    if app.plugin(Plugin::ViewRaw) && !message.content.is_empty() && item(ui, t!("View Raw"), Tail::None).clicked() {
        app.notify(message.content.clone());
        ui.ctx().copy_text(message.content.clone());
        ui.close();
    }
    if app.plugin(Plugin::ReverseImageSearch)
        && let Some(image) = message.attachments.iter().find(|a| a.is_image())
        && item(ui, t!("Search Image (Google Lens)"), Tail::Icon(Icon::Search)).clicked()
    {
        let _ = open::that_detached(format!("https://lens.google.com/uploadbyurl?url={}", image.url.replace('&', "%26")));
        ui.close();
    }
    if actions.delete {
        super::menu::separator(ui);
        let shift = ui.input(|i| i.modifiers.shift);
        if super::menu::danger(ui, t!("Delete Message"), Tail::Icon(Icon::Trash)).clicked() {
            if shift {
                app.actions.push(Action::Delete { channel: message.channel_id, id: message.id });
            } else {
                app.modal = Some(delete_confirmation(message));
            }
            ui.close();
        }
    }
    super::menu::separator(ui);
    super::menu::copy_id(ui, t!("Copy Message ID"), message.id);
}

/// Asks who reacted with `emoji`, for the open dialog.
fn ask_reactors(app: &mut App, channel: Id, message: Id, emoji: &crate::discord::model::Emoji) {
    app.reactors = None;
    let name = crate::discord::rest::percent_encode(&emoji.api_name());
    app.commands.send(crate::admin::get("reactors", format!("/channels/{channel}/messages/{message}/reactions/{name}?limit=100&type=0")));
}

/// Discord's "Delete Message" question (Shift skips it).
fn delete_confirmation(message: &Message) -> crate::app::Modal {
    let preview: String = message.content.chars().take(300).collect();
    let body = if preview.is_empty() {
        t!("Are you sure you want to delete this message?").to_owned()
    } else {
        format!("{}\n\n“{preview}”", t!("Are you sure you want to delete this message?"))
    };
    crate::app::Modal::Confirm {
        title: t!("Delete Message").into(),
        body: format!("{body}\n\n{}", t!("PROTIP: You can hold down shift when clicking delete message to bypass this confirmation entirely.")),
        button: t!("Delete").into(),
        then: crate::app::Confirmed::DeleteMessage { channel: message.channel_id, id: message.id },
    }
}

/// Discord's welcomes for someone joining, one picked by the message's
/// time as Discord picks it, so every client shows the same.
const WELCOMES: [&str; 13] = [
    n!("{name} joined the party."),
    n!("{name} is here."),
    n!("Welcome, {name}. We hope you brought pizza."),
    n!("A wild {name} appeared."),
    n!("{name} just landed."),
    n!("{name} just slid into the server."),
    n!("{name} just showed up!"),
    n!("Welcome {name}. Say hi!"),
    n!("{name} hopped into the server."),
    n!("Everyone welcome {name}!"),
    n!("Glad you're here, {name}."),
    n!("Good to see you, {name}."),
    n!("Yay you made it, {name}!"),
];

/// What Discord writes for a system message (types from its MessageTypes).
fn system_text(message: &Message, name: &str, me: Id, peer: &str) -> String {
    let other = message.mentions.first().map(|u| u.display_name().to_owned()).unwrap_or_else(|| t!("someone").into());
    let content = &message.content;
    match message.kind {
        1 => t!("{name} added {other} to the group.", name = name, other = other),
        2 => t!("{name} removed {other} from the group.", name = name, other = other),
        3 => t!("{name} started a call.", name = name),
        4 => t!("{name} changed the channel name: {channel}", name = name, channel = content),
        5 => t!("{name} changed the channel icon.", name = name),
        6 => t!("{name} pinned a message to this channel.", name = name),
        7 => {
            let welcome = WELCOMES[(crate::discord::model::snowflake_ms(message.id) % WELCOMES.len() as u64) as usize];
            crate::i18n::tr(welcome).replace("{name}", name)
        }
        8 => t!("{name} just boosted the server!", name = name),
        9..=11 => t!("{name} just boosted the server! The server has achieved Level {level}!", name = name, level = message.kind - 8),
        12 => t!("{name} has added {channel} to this channel. Its most important updates will show up here.", name = name, channel = content),
        18 => t!("{name} started a thread: {thread}", name = name, thread = content),
        24 => t!("AutoMod has blocked a message.").to_owned(),
        27 => t!("{name} started {stage}", name = name, stage = content),
        28 => t!("{name} ended {stage}", name = name, stage = content),
        29 => t!("{name} is now a speaker.", name = name),
        31 => t!("{name} changed the Stage topic: {topic}", name = name, topic = content),
        46 => t!("{name}'s poll has closed.", name = name),
        63 => t!("{name} added a new emoji.", name = name),
        67 => t!("You and {name} are now friends.", name = if message.author.id == me { peer } else { name }),
        _ => message.content.clone(),
    }
}

fn reactions(app: &mut App, ui: &mut egui::Ui, message: &Message) {
    if message.reactions.is_empty() {
        return;
    }
    let p = theme::p();
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for reaction in &message.reactions {
            let fill = if reaction.me { p.accent.linear_multiply(0.25) } else { p.sidebar };
            let stroke = if reaction.me { Stroke::new(1.0, p.accent) } else { Stroke::new(1.0, p.sidebar) };
            let response = Frame::new()
                .fill(fill)
                .stroke(stroke)
                .corner_radius(8)
                .inner_margin(Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        match reaction.emoji.url() {
                            Some(url) => {
                                let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                                if let Some((texture, _)) = app.images.get(&url, 40) {
                                    egui::Image::from_texture((texture, rect.size())).paint_at(ui, rect);
                                }
                            }
                            None => {
                                ui.label(RichText::new(&reaction.emoji.name).size(15.0));
                            }
                        }
                        ui.label(RichText::new(reaction.count.to_string()).size(13.0).color(if reaction.me { p.strong } else { p.muted }));
                    });
                })
                .response
                .interact(Sense::click());
            if response.on_hover_text(format!(":{}:", reaction.emoji.name)).clicked() {
                app.actions.push(Action::React {
                    channel: message.channel_id,
                    message: message.id,
                    emoji: reaction.emoji.api_name(),
                    add: !reaction.me,
                });
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
/// The files waiting to be sent, as Discord's upload cards over the box:
/// the picture (or a file icon) and the name; on hover a small bar marks it
/// a spoiler, opens its name and description, or removes it.
fn upload_cards(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    const CARD: f32 = 216.0;
    let (mut remove, mut modify, mut spoil) = (None, None, None);
    Frame::new().fill(p.composer).corner_radius(CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }).inner_margin(Margin { left: 10, right: 10, top: 20, bottom: 10 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ScrollArea::horizontal().id_salt("upload-cards").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                for (i, upload) in app.files.iter().enumerate() {
                    let (card, response) = ui.allocate_exact_size(Vec2::splat(CARD), Sense::hover());
                    ui.painter().rect(card, 6.0, p.sidebar, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
                    let area = Rect::from_min_max(card.min + vec2(8.0, 8.0), pos2(card.right() - 8.0, card.bottom() - 34.0));
                    let ppp = ui.ctx().pixels_per_point();
                    let picture = upload.is_image().then(|| app.images.get(&format!("file://{}", upload.path.display()), (area.width() * ppp) as u32)).flatten();
                    match picture {
                        Some((texture, size)) => {
                            let scale = (area.width() / size.x).min(area.height() / size.y);
                            let shown = Rect::from_center_size(area.center(), size * scale);
                            let tint = if upload.spoiler { Color32::from_gray(40) } else { Color32::WHITE };
                            egui::Image::from_texture((texture, shown.size())).tint(tint).corner_radius(4.0).paint_at(ui, shown);
                        }
                        None => crate::icons::paint(ui, Rect::from_center_size(area.center(), Vec2::splat(64.0)), crate::icons::Icon::File, p.muted),
                    }
                    if upload.spoiler {
                        let label = ui.painter().layout_no_wrap(t!("Spoiler").to_uppercase(), theme::strong(12.0), Color32::WHITE);
                        let pill = Rect::from_center_size(area.center(), label.size() + vec2(20.0, 10.0));
                        ui.painter().rect_filled(pill, pill.height() / 2.0, Color32::from_black_alpha(200));
                        ui.painter().galley(pill.center() - label.size() / 2.0, label, Color32::WHITE);
                    }
                    super::paint_truncated(ui, Rect::from_min_max(pos2(card.left() + 8.0, card.bottom() - 30.0), pos2(card.right() - 8.0, card.bottom() - 8.0)), &upload.name, FontId::proportional(14.0), p.text);
                    // The bar over the card's corner while it is pointed at.
                    let bar = Rect::from_min_size(pos2(card.right() - 3.0 * 32.0 - 2.0, card.top() - 12.0), vec2(3.0 * 32.0 + 4.0, 36.0));
                    let pointer = ui.ctx().pointer_hover_pos();
                    if response.hovered() || pointer.is_some_and(|at| bar.contains(at)) {
                        ui.painter().rect(bar, 6.0, p.rail, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
                        let tools = [
                            (if upload.spoiler { crate::icons::Icon::EyeSlash } else { crate::icons::Icon::Eye }, t!("Spoiler Attachment"), p.text),
                            (crate::icons::Icon::Pencil, t!("Modify Attachment"), p.text),
                            (crate::icons::Icon::Trash, t!("Remove Attachment"), p.red),
                        ];
                        for (n, (icon, tip, color)) in tools.into_iter().enumerate() {
                            let cell = Rect::from_min_size(pos2(bar.left() + 2.0 + n as f32 * 32.0, bar.top() + 2.0), Vec2::splat(32.0));
                            let tool = ui.interact(cell, ui.id().with(("upload-tool", i, n)), Sense::click());
                            if tool.hovered() {
                                ui.painter().rect_filled(cell.shrink(2.0), 4.0, p.hover);
                            }
                            crate::icons::paint(ui, Rect::from_center_size(cell.center(), Vec2::splat(20.0)), icon, color);
                            if tool.on_hover_text(tip).clicked() {
                                match n {
                                    0 => spoil = Some(i),
                                    1 => modify = Some(i),
                                    _ => remove = Some(i),
                                }
                            }
                        }
                    }
                }
            });
        });
    });
    if let Some(i) = spoil {
        app.files[i].spoiler = !app.files[i].spoiler;
    }
    if let Some(i) = modify {
        app.modal = Some(crate::app::Modal::Attachment { index: i, upload: app.files[i].clone() });
    }
    if let Some(i) = remove {
        app.files.remove(i);
    }
}

/// A sticker in a message, as Discord's: 160 points with no frame behind
/// it (Lottie ones drawn here), its name on hover.
fn sticker(app: &mut App, ui: &mut egui::Ui, id: Id, name: &str, format: u64) {
    let mut url = crate::discord::model::sticker_url(id, format);
    // "Never animate": the other frames are not even decoded.
    if app.preloaded.number(crate::discord::preloaded::ANIMATE_STICKERS) == 2 {
        url.push_str(crate::backend::STILL);
    }
    let px = (160.0 * ui.ctx().pixels_per_point()).round() as u32;
    ui.add_space(4.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(160.0), Sense::hover());
    // Settings → Accessibility: always, under the pointer, or never.
    let play = match app.preloaded.number(crate::discord::preloaded::ANIMATE_STICKERS) {
        0 => true,
        1 => response.hovered(),
        _ => false,
    };
    if let Some((texture, size)) = app.images.get_playing(&url, px, play) {
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        egui::Image::from_texture((texture, size * scale)).paint_at(ui, Rect::from_min_size(rect.min, size * scale));
    }
    response.on_hover_text(name);
}

fn picture(app: &mut App, ui: &mut egui::Ui, proxy: &str, original: &str, w: u32, h: u32, max_w: f32, max_h: f32, from: Option<&Message>) {
    let p = theme::p();
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let scale = (max_w / w).min(max_h / h).min(1.0);
    let size = vec2((w * scale).round(), (h * scale).round());
    let ppp = ui.ctx().pixels_per_point();
    let (pw, ph) = ((size.x * ppp).round() as u32, (size.y * ppp).round() as u32);
    let separator = if proxy.contains('?') { '&' } else { '?' };
    // A GIF from the file itself: the proxy's resized copies may not move.
    let gif = original.split('?').next().is_some_and(|path| path.to_ascii_lowercase().ends_with(".gif"));
    let url = if gif {
        original.to_owned()
    } else if proxy.contains("media.discordapp.net") || proxy.contains("images-ext") {
        format!("{proxy}{separator}width={pw}&height={ph}")
    } else {
        proxy.to_owned()
    };
    ui.add_space(4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    // "Play GIFs when Discord is focused": off, they move under the pointer.
    let play = response.hovered() || app.preloaded.flag(crate::discord::preloaded::GIF_AUTO_PLAY);
    match app.images.get_playing(&url, pw.max(ph), play) {
        Some((texture, _)) => {
            egui::Image::from_texture((texture, size)).corner_radius(6.0).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 6.0, p.sidebar);
        }
    }
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        // The viewer asks for a larger copy than the message shows.
        let big = if gif {
            original.to_owned()
        } else if proxy.contains("media.discordapp.net") {
            let s = (1600.0 / w.max(h)).min(1.0);
            format!("{proxy}{separator}width={}&height={}", (w * s) as u32, (h * s) as u32)
        } else {
            proxy.to_owned()
        };
        let from = from.map(|message| {
            let (name, _) = app.display(message.guild_id, &message.author);
            crate::app::ViewerSource { author: message.author.clone(), name, at: message.timestamp_ms(), channel: message.channel_id, guild: message.guild_id, message: message.id }
        });
        app.viewer = Some(Viewer {
            url: big,
            original: original.to_owned(),
            size: vec2(w, h),
            zoom: 1.0,
            offset: Vec2::ZERO,
            from,
        });
    }
}

/// Discord's media viewer: the picture over a dark backdrop, fitted to the
/// window. A click zooms in where it lands and another zooms back out; the
/// wheel zooms about the pointer and a drag moves a close-up. Who posted it
/// sits at the top left, the tools at the top right; Esc or a click on the
/// backdrop closes it.
pub fn viewer(app: &mut App, ctx: &egui::Context) {
    let Some(mut view) = app.viewer.take() else { return };
    let mut keep = !ctx.input(|i| i.key_pressed(Key::Escape));
    let screen = ctx.content_rect();
    egui::Area::new("viewer".into()).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
        let (rect, backdrop) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(230));
        // The room between the bars, and the close-up a click goes to: the
        // picture's own size, at least twice the fitted one.
        let room = Rect::from_min_max(rect.min + vec2(40.0, 72.0), rect.max - vec2(40.0, 40.0));
        let fit = (room.width() / view.size.x).min(room.height() / view.size.y).min(1.0);
        let close_up = (1.0 / fit).clamp(2.0, 8.0);
        // The picture's centre after zooming about `at` from one zoom to another.
        let about = |at: egui::Pos2, center: egui::Pos2, from: f32, to: f32| at - (at - center) * (to / from);
        let shown = Rect::from_center_size(room.center() + view.offset, view.size * fit * view.zoom);
        let picture = ui.interact(shown.intersect(rect), ui.id().with("viewer-picture"), Sense::click_and_drag());
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(at) = ctx.pointer_hover_pos()
        {
            let zoom = (view.zoom * (1.0 + scroll / 400.0)).clamp(1.0, 8.0);
            view.offset = about(at, room.center() + view.offset, view.zoom, zoom) - room.center();
            view.zoom = zoom;
        }
        if picture.clicked() {
            if view.zoom > 1.01 {
                view.zoom = 1.0;
                view.offset = Vec2::ZERO;
            } else if let Some(at) = picture.interact_pointer_pos() {
                view.offset = about(at, room.center() + view.offset, view.zoom, close_up) - room.center();
                view.zoom = close_up;
            }
        }
        if picture.dragged() && view.zoom > 1.01 {
            view.offset += picture.drag_delta();
        }
        // A close-up never shows an empty edge where it is larger than the room.
        let size = view.size * fit * view.zoom;
        let slack = ((size - room.size()) / 2.0).max(Vec2::ZERO);
        view.offset = view.offset.clamp(-slack, slack);
        let shown = Rect::from_center_size(room.center() + view.offset, size);
        match app.images.get(&view.url, 1600) {
            Some((texture, _)) => {
                egui::Image::from_texture((texture, size)).corner_radius(if view.zoom > 1.01 { 0.0 } else { 4.0 }).paint_at(&ui.new_child(egui::UiBuilder::new().max_rect(rect)), shown);
            }
            None => {
                ui.put(Rect::from_center_size(room.center(), Vec2::splat(32.0)), egui::Spinner::new().size(32.0).color(Color32::WHITE));
            }
        }
        picture.on_hover_cursor(if view.zoom > 1.01 { egui::CursorIcon::ZoomOut } else { egui::CursorIcon::ZoomIn });
        if viewer_bar(app, ui, rect, &mut view) {
            keep = false;
        }
        if backdrop.clicked() {
            keep = false;
        }
    });
    if keep {
        app.viewer = Some(view);
    }
}

/// The viewer's top bar: who posted the picture and when, then Discord's
/// tools. Returns whether it asked to close.
fn viewer_bar(app: &mut App, ui: &mut egui::Ui, rect: Rect, view: &mut crate::app::Viewer) -> bool {
    use crate::icons::Icon;
    let mut close = false;
    if let Some(from) = &view.from {
        let avatar = Rect::from_min_size(rect.min + vec2(24.0, 16.0), Vec2::splat(40.0));
        app.images.circle(ui, avatar, Some(&from.author.avatar_url()), &super::initials(&from.name), 20.0);
        ui.painter().text(pos2(avatar.right() + 12.0, avatar.top() + 1.0), Align2::LEFT_TOP, &from.name, theme::strong(16.0), Color32::WHITE);
        ui.painter().text(pos2(avatar.right() + 12.0, avatar.bottom() - 1.0), Align2::LEFT_BOTTOM, text::time(from.at), FontId::proportional(13.0), Color32::from_gray(185));
    }
    // Discord's order (the bundle's media viewer): zoom, forward, save,
    // open in the browser, more; then close.
    let tools: [(Icon, &str); 6] = [
        if view.zoom > 1.01 { (Icon::ZoomOut, t!("Zoom Out")) } else { (Icon::ZoomIn, t!("Zoom In")) },
        (Icon::Forward, t!("Forward")),
        (Icon::Download, t!("Save Image")),
        (Icon::Launch, t!("Open in Browser")),
        (Icon::More, t!("More")),
        (Icon::Close, t!("Close")),
    ];
    let mut x = rect.right() - 24.0;
    for (n, (icon, tip)) in tools.into_iter().enumerate().rev() {
        // Forward needs the message the picture came in.
        if n == 1 && view.from.is_none() {
            continue;
        }
        let cell = Rect::from_min_size(pos2(x - 40.0, rect.top() + 16.0), Vec2::splat(40.0));
        x -= 44.0;
        let response = ui.interact(cell, ui.id().with(("viewer-tool", n)), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(cell, 8.0, Color32::from_white_alpha(24));
        }
        crate::icons::paint(ui, Rect::from_center_size(cell.center(), Vec2::splat(24.0)), icon, Color32::WHITE);
        let response = response.on_hover_text(tip);
        if n == 4 {
            egui::Popup::menu(&response).show(|ui| {
                super::menu::begin(ui);
                if super::menu::item(ui, t!("Copy Image"), super::menu::Tail::None).clicked() {
                    app.commands.send(crate::backend::Command::CopyImage { url: view.url.clone() });
                    ui.close();
                }
                if super::menu::item(ui, t!("Copy Media Link"), super::menu::Tail::None).clicked() {
                    ui.ctx().copy_text(view.original.clone());
                    app.notify(t!("Link Copied!"));
                    ui.close();
                }
            });
        }
        if !response.clicked() {
            continue;
        }
        match n {
            0 => {
                if view.zoom > 1.01 {
                    view.zoom = 1.0;
                    view.offset = Vec2::ZERO;
                } else {
                    view.zoom = 2.0;
                }
            }
            1 => {
                if let Some(from) = &view.from {
                    app.modal = Some(crate::app::Modal::Forward { channel: from.channel, guild: from.guild, message: from.message, picked: Vec::new(), filter: String::new(), note: String::new() });
                    close = true;
                }
            }
            2 => {
                let name = view.original.split('?').next().and_then(|u| u.rsplit('/').next()).filter(|n| !n.is_empty()).unwrap_or("image.png").to_owned();
                app.commands.send(crate::backend::Command::SaveImage { url: view.original.clone(), name });
            }
            3 => {
                let _ = open::that_detached(&view.original);
            }
            5 => close = true,
            _ => {}
        }
    }
    close
}

fn embed_view(app: &mut App, ui: &mut egui::Ui, embed: &crate::discord::model::Embed, message: &Message) {
    if embed.title.is_none()
        && embed.description.is_none()
        && embed.image.is_none()
        && embed.fields.is_empty()
        && embed.author.is_none()
        && embed.footer.is_none()
    {
        return;
    }
    let p = theme::p();
    ui.add_space(4.0);
    let bar = embed.color.and_then(theme::role_color).unwrap_or(p.selected);
    let response = Frame::new()
        .fill(p.sidebar)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 12 })
        .show(ui, |ui| {
            ui.set_max_width(516.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(if embed.thumbnail.is_some() { 420.0 } else { 484.0 });
                    if let Some(provider) = &embed.provider {
                        ui.label(RichText::new(provider).size(12.0).color(p.muted));
                    }
                    // The author with their picture, a link when they have one.
                    if let Some(author) = &embed.author {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if let Some(icon) = &embed.author_icon {
                                let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                                app.images.circle(ui, rect, Some(icon), "", 12.0);
                            }
                            let name = RichText::new(author).font(theme::strong(14.0)).color(p.strong);
                            match &embed.author_url {
                                Some(url) => {
                                    ui.hyperlink_to(name, url);
                                }
                                None => {
                                    ui.label(name);
                                }
                            }
                        });
                    }
                    if let Some(title) = &embed.title {
                        match &embed.url {
                            Some(url) => {
                                ui.hyperlink_to(RichText::new(title).font(theme::strong(15.0)).color(p.link), url);
                            }
                            None => {
                                ui.label(RichText::new(title).font(theme::strong(15.0)).color(p.strong));
                            }
                        }
                    }
                    if let Some(description) = &embed.description {
                        let short: String = description.chars().take(1200).collect();
                        let blocks = markdown::parse(&short);
                        markdown::show(app, ui, &blocks, p.text, 0);
                    }
                    // Fields: inline ones side by side, up to three a row.
                    let mut row: Vec<&(String, String, bool)> = Vec::new();
                    let flush = |app: &mut App, ui: &mut egui::Ui, row: &mut Vec<&(String, String, bool)>| {
                        if row.is_empty() {
                            return;
                        }
                        ui.add_space(4.0);
                        ui.columns(row.len(), |columns| {
                            for (column, (name, value, _)) in columns.iter_mut().zip(row.iter()) {
                                column.label(RichText::new(name).font(theme::strong(13.5)).color(p.strong));
                                let blocks = markdown::parse(value);
                                markdown::show(app, column, &blocks, p.text, 0);
                            }
                        });
                        row.clear();
                    };
                    for field in &embed.fields {
                        if !field.2 || row.len() == 3 {
                            flush(app, ui, &mut row);
                        }
                        row.push(field);
                        if !field.2 {
                            flush(app, ui, &mut row);
                        }
                    }
                    flush(app, ui, &mut row);
                });
                if let Some((url, w, h)) = &embed.thumbnail {
                    let original = url.clone();
                    picture(app, ui, url, &original, *w, *h, 80.0, 80.0, Some(message));
                }
            });
            if let Some((url, w, h)) = &embed.image {
                let original = embed.url.clone().unwrap_or_else(|| url.clone());
                picture(app, ui, url, &original, *w, *h, 400.0, 300.0, Some(message));
            }
            // The footer's picture and text, and when it happened: "Today
            // at 7:30 PM", as message times read.
            if embed.footer.is_some() || embed.timestamp.is_some() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if let Some(icon) = &embed.footer_icon {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        app.images.circle(ui, rect, Some(icon), "", 10.0);
                    }
                    let line = match (&embed.footer, embed.timestamp) {
                        (Some(footer), Some(ms)) => format!("{footer} • {}", text::time(ms)),
                        (Some(footer), None) => footer.clone(),
                        (None, Some(ms)) => text::time(ms),
                        (None, None) => String::new(),
                    };
                    ui.label(RichText::new(line).font(theme::strong(12.0)).color(p.text));
                });
            }
        });
    let r = response.response.rect;
    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(4.0, r.height())), CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, bar);
}

/// A poll, as Discord draws it: each answer a bar filled to its share.
fn poll_view(app: &mut App, ui: &mut egui::Ui, message: &Message, poll: &crate::discord::model::Poll) {
    let p = theme::p();
    let total: u64 = poll.answers.iter().map(|a| a.2).sum();
    let voted = poll.answers.iter().any(|a| a.3);
    let show_results = voted || poll.finalized;
    Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(14)).show(ui, |ui| {
        ui.set_max_width(440.0);
        ui.label(RichText::new(&poll.question).font(theme::strong(16.0)).color(p.strong));
        let hint = if poll.finalized {
            t!("Poll closed")
        } else if poll.multiselect {
            t!("Select one or more answers")
        } else {
            t!("Select one answer")
        };
        ui.label(RichText::new(hint).size(12.5).color(p.muted));
        ui.add_space(6.0);
        for (id, text, votes, mine) in &poll.answers {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
            let border = if *mine { p.accent } else if response.hovered() && !poll.finalized { p.muted } else { p.selected };
            ui.painter().rect(rect, 8.0, p.chat, Stroke::new(1.5, border), egui::StrokeKind::Inside);
            if show_results && total > 0 {
                let share = *votes as f32 / total as f32;
                let fill = Rect::from_min_size(rect.min, vec2(rect.width() * share, rect.height()));
                ui.painter().rect_filled(fill, 8.0, if *mine { p.accent.linear_multiply(0.35) } else { p.selected });
            }
            ui.painter().text(rect.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, text, theme::strong(14.0), p.strong);
            if show_results {
                let percent = if total > 0 { votes * 100 / total } else { 0 };
                ui.painter().text(rect.right_center() - vec2(14.0, 0.0), Align2::RIGHT_CENTER, format!("{}  {percent}%", tn!(*votes, "{count} vote", "{count} votes")), FontId::proportional(13.0), p.muted);
            }
            if response.clicked() && !poll.finalized {
                let mut chosen: Vec<u64> = if poll.multiselect {
                    poll.answers.iter().filter(|a| a.3).map(|a| a.0).collect()
                } else {
                    Vec::new()
                };
                if *mine {
                    chosen.retain(|a| a != id);
                } else {
                    chosen.push(*id);
                }
                app.commands.send(crate::backend::Command::Api {
                    key: "vote".into(),
                    method: "PUT",
                    path: format!("/channels/{}/polls/{}/answers/@me", message.channel_id, message.id),
                    body: Some(serde_json::json!({ "answer_ids": chosen.iter().map(ToString::to_string).collect::<Vec<_>>() })),
                });
            }
            ui.add_space(4.0);
        }
        let mut footer = tn!(total, "{count} vote", "{count} votes");
        if !poll.finalized
            && let Some(end) = poll.expiry.as_deref().and_then(|e| e.parse::<jiff::Timestamp>().ok())
        {
            footer.push_str(&format!("  ·  {}", t!("ends {when}", when = text::time(end.as_millisecond().max(0) as u64).to_lowercase())));
        }
        ui.label(RichText::new(footer).size(12.0).color(p.muted));
    });
}

fn forum(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let p = theme::p();
    let posts = app.forums.get(&channel.id).cloned();
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_space(12.0);
        Frame::new().inner_margin(Margin::symmetric(16, 4)).show(ui, |ui| {
            if ui.add(egui::Button::new(RichText::new(t!("New Post")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                app.modal = Some(crate::app::Modal::NewPost { forum: channel.id, title: String::new(), body: String::new() });
            }
        });
        let Some(Some(posts)) = posts else {
            ui.vertical_centered(|ui| ui.spinner());
            return;
        };
        if posts.is_empty() {
            ui.vertical_centered(|ui| ui.label(RichText::new(t!("No active posts.")).color(p.muted)));
        }
        for (thread, first) in posts {
            Frame::new().inner_margin(Margin::symmetric(16, 4)).show(ui, |ui| {
                let response = Frame::new()
                    .fill(p.sidebar)
                    .corner_radius(8)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&thread.name).font(theme::strong(16.0)).color(p.strong));
                        if let Some(first) = &first {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(first.author.display_name()).font(theme::strong(13.0)).color(p.muted));
                                let preview: String = first.content.chars().take(160).collect();
                                ui.add(Label::new(RichText::new(preview).size(13.0).color(p.muted)).truncate());
                            });
                        }
                        if let Some(last) = thread.last_message_id {
                            ui.label(RichText::new(t!("Last activity: {when}", when = text::time(crate::discord::model::snowflake_ms(last)))).size(11.5).color(p.faint));
                        }
                    })
                    .response
                    .interact(Sense::click());
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::SelectChannel(thread.id));
                }
            });
        }
    });
}

fn composer(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let p = theme::p();
    // The server's rules come first for someone new.
    if let Some(guild) = channel.guild_id
        && app.members.get(&(guild, app.me_id())).is_some_and(|member| member.pending)
    {
        Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if ui.add(egui::Button::new(RichText::new(t!("Complete")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                super::gates::open(app, super::gates::Gate::Verification { guild, answers: Default::default(), agreed: false });
            }
        });
        return;
    }
    // An announcement channel we cannot write in: Discord's bar to follow
    // it instead of the box.
    const SEND_MESSAGES: u64 = 1 << 11;
    if channel.kind == ChannelKind::Announcement
        && let Some(guild) = channel.guild_id.and_then(|g| app.guild(g))
        && guild.channel_permissions(app.me_id(), channel) & SEND_MESSAGES == 0
    {
        Frame::new().fill(p.composer).corner_radius(8).inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("Follow to get this channel's updates in your own server.")).font(theme::strong(15.0)).color(p.strong));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if follow_button(ui, 32.0) {
                        app.modal = Some(crate::app::Modal::Follow { channel: channel.id, guild: None, target: None });
                    }
                });
            });
        });
        return;
    }
    // Someone who is no friend wrote: nothing goes out until it is accepted.
    if channel.message_request {
        Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(t!("Message Requests")).font(theme::strong(15.0)).color(p.strong));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let path = format!("/channels/{}/recipients/@me", channel.id);
                if ui.add(egui::Button::new(RichText::new(t!("Accept")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                    app.commands.send(call("PUT", path.clone(), Some(serde_json::json!({ "consent_status": 2 }))));
                }
                if ui.button(t!("Ignore")).clicked() {
                    app.commands.send(call("DELETE", path, None));
                    app.actions.push(Action::CloseDm(channel.id));
                }
            });
        });
        return;
    }
    let typing: Vec<String> = app
        .typing
        .get(&channel.id)
        .map(|list| list.iter().filter_map(|(id, _)| app.users.get(id).map(|u| u.display_name().to_owned())).collect())
        .unwrap_or_default();

    if let Some((_, author)) = &app.replying {
        let author = author.clone();
        bar(app, ui, &t!("Replying to {author}", author = author));
    }
    if !app.files.is_empty() {
        upload_cards(app, ui);
    }

    let id = egui::Id::new("composer");
    let mut send = false;
    // A slash command picked here and left for another channel is dropped.
    if app.slash.as_ref().is_some_and(|d| d.channel != channel.id) {
        app.slash = None;
    }
    let in_command = app.slash.is_some();
    // The box gives way to the command's fields and its focus goes with it:
    // kept, its Enter sent the empty box instead of running a command
    // without options.
    if in_command {
        ui.ctx().memory_mut(|m| {
            if m.has_focus(id) {
                m.surrender_focus(id);
            }
        });
    }
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    if in_command {
        super::commands::suggestions(app, ui, channel);
    } else {
        super::commands::picker(app, ui, channel, focused);
    }
    // A command picked just now takes the box this very frame.
    let in_command = app.slash.is_some();
    let (start, suggestions) = super::emoji::suggestions(app, channel.id);
    if !suggestions.is_empty() {
        if let Some(i) = super::emoji::suggestion_list(app, ui, &suggestions) {
            super::emoji::pick(app, start, &suggestions[i]);
            app.focus_composer = true;
        } else if focused
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter))
        {
            super::emoji::pick(app, start, &suggestions[0]);
            app.focus_composer = true;
        }
        ui.add_space(4.0);
    }
    if focused {
        send = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            // As Discord: Esc drops a reply or the attachments, or else
            // marks the channel read; Shift+Esc the whole server.
            if app.replying.is_some() || !app.files.is_empty() {
                app.actions.push(Action::CancelCompose);
            } else {
                app.actions.push(Action::MarkRead(vec![channel.id]));
            }
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Escape))
            && let Some(guild) = channel.guild_id.and_then(|g| app.guild(g))
        {
            let all = guild.channels.iter().filter(|c| c.kind.is_text()).map(|c| c.id).collect();
            app.actions.push(Action::MarkRead(all));
        }
        // A screenshot on the clipboard goes in as a PNG attachment.
        if crate::keybinds::paste_pressed()
            && let Some(path) = pasted_image()
            && app.files.len() < 10
        {
            app.files.push(crate::backend::Upload::new(path));
        }
        if app.composer.is_empty()
            && app.editing.is_none()
            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp))
            && let Some(last) = app
                .histories
                .get(&channel.id)
                .and_then(|h| h.messages.iter().rev().find(|m| m.author.id == app.me_id() && !m.pending))
        {
            app.actions.push(Action::StartEdit { id: last.id, content: last.content.clone() });
        }
        // Ctrl+↑: reply to the last message.
        if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::ArrowUp))
            && let Some(last) = app.histories.get(&channel.id).and_then(|h| h.messages.iter().rev().find(|m| !m.pending))
        {
            app.actions.push(Action::Reply { id: last.id, author: last.author.display_name().to_owned() });
        }
    }
    let hint = if channel.guild_id.is_some() {
        t!("Message #{channel}", channel = channel.name)
    } else {
        t!("Message @{user}", user = channel.title())
    };
    // Discord's box: 44 points for one line, the text at 16 points centred
    // in it (11 points above and below), the buttons in 44-point cells
    // beside the first line.
    const BOX: f32 = 44.0;
    let font = FontId::proportional(16.0);
    let row = ui.fonts_mut(|f| f.row_height(&font));
    let pad = ((BOX - row) / 2.0).max(0.0);
    let mut run_command = false;
    let boxed = Frame::new().fill(p.composer).corner_radius(8).inner_margin(Margin { left: 6, right: 6, top: 0, bottom: 0 }).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (rect, response) = ui.allocate_exact_size(vec2(40.0, BOX), Sense::click());
            ui.painter().circle_filled(rect.center(), 11.0, if response.hovered() { p.strong } else { p.muted });
            crate::icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(14.0)), crate::icons::Icon::Plus, p.composer);
            let response = response.on_hover_text(t!("Upload a File (or drag it onto the window)"));
            let polls = channel.guild_id.and_then(|g| app.guild(g)).is_none_or(|g| g.channel_permissions(app.me_id(), channel) & (1 << 49) != 0);
            let thread_of = channel.id;
            egui::Popup::menu(&response).show(|ui| {
                super::menu::begin(ui);
                if super::menu::item(ui, t!("Upload a File"), super::menu::Tail::None).clicked() {
                    app.actions.push(Action::AttachFiles);
                    ui.close();
                }
                if polls && super::menu::item(ui, t!("Create Poll"), super::menu::Tail::None).clicked() {
                    app.modal = Some(crate::app::Modal::NewPoll { channel: thread_of, question: String::new(), answers: vec![String::new(), String::new()], hours: 24, multiple: false });
                    ui.close();
                }
            });
            ui.add_space(4.0);
            // Settings → Appearance → Chat Box hides any of the three.
            let buttons = [app.settings.sticker_button, app.settings.gif_button, app.settings.emoji_button].iter().filter(|b| **b).count();
            let width = ui.available_width() - buttons as f32 * 36.0 - 4.0;
            // A slash command picked: its options take the box's place.
            if in_command {
                run_command = super::commands::fields(app, ui, channel, width, BOX);
            } else {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut app.composer)
                        .id(id)
                        // This egui takes a custom frame's margin, not .margin().
                        .frame(Frame::NONE.inner_margin(Margin { left: 0, right: 0, top: pad.floor() as i8, bottom: pad.ceil() as i8 }))
                        .desired_rows(1)
                        .desired_width(width)
                        .font(font.clone())
                        .hint_text(RichText::new(hint).font(font.clone()).color(p.faint))
                        .text_color(p.text),
                );
                if response.changed() {
                    app.typed();
                }
                if std::mem::take(&mut app.focus_composer) {
                    response.request_focus();
                    // Puts the cursor at the end after an insertion.
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                        let end = egui::text::CCursor::new(app.composer.chars().count());
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                        state.store(ui.ctx(), id);
                    }
                }
            }
            ui.add_space(4.0);
            let cell = |ui: &mut egui::Ui| ui.allocate_exact_size(vec2(36.0, BOX), Sense::click());
            if app.settings.sticker_button {
                let (rect, response) = cell(ui);
                crate::icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)), crate::icons::Icon::Sticker, if response.hovered() { p.strong } else { p.muted });
                if response.on_hover_text(t!("Open sticker picker")).clicked() {
                    app.toggle_expressions(crate::app::ExpressionTab::Stickers, crate::app::PickerTarget::Composer, ui.input(|i| i.time));
                }
            }
            if app.settings.gif_button {
                let (rect, response) = cell(ui);
                crate::icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)), crate::icons::Icon::Gif, if response.hovered() { p.strong } else { p.muted });
                if response.on_hover_text(t!("Open GIF picker")).clicked() {
                    app.toggle_expressions(crate::app::ExpressionTab::Gifs, crate::app::PickerTarget::Composer, ui.input(|i| i.time));
                }
            }
            if app.settings.emoji_button {
                let (rect, response) = cell(ui);
                emoji_button(ui, &response, Rect::from_center_size(rect.center(), Vec2::splat(28.0)));
                if response.on_hover_text(t!("Select emoji")).clicked() {
                    app.toggle_expressions(crate::app::ExpressionTab::Emoji, crate::app::PickerTarget::Composer, ui.input(|i| i.time));
                }
            }
        });
    });
    // Discord's count near the limit (2000, or 4000 with Nitro), red past
    // it, in the box's corner.
    let limit = message_limit(app);
    let used = app.composer.chars().count();
    if used + 200 > limit {
        let left = limit as i64 - used as i64;
        let rect = boxed.response.rect;
        ui.painter().text(pos2(rect.right() - 12.0, rect.bottom() - 4.0), Align2::RIGHT_BOTTOM, left.to_string(), theme::strong(12.0), if left < 0 { p.red } else { p.muted });
    }
    if run_command {
        super::commands::run(app, channel);
    }
    if send {
        if used > limit {
            app.notify(t!("Your message is too long: the limit is {limit} characters.", limit = limit));
        } else {
            app.actions.push(Action::Send);
        }
    }
    typing_line(ui, &typing);
}

/// Discord's line under the box: three dots that pulse in turn, then the
/// names in bold and the rest plain, at the left. Painted, not a label, so
/// a click cannot select it.
fn typing_line(ui: &mut egui::Ui, typing: &[String]) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    if typing.is_empty() {
        return;
    }
    let time = ui.input(|i| i.time) as f32;
    for i in 0..3 {
        // Each dot brightens and grows in turn, a third of a cycle apart.
        let phase = ((time * 2.4 - i as f32 * 0.33).rem_euclid(1.0) * std::f32::consts::TAU).sin().max(0.0);
        let center = pos2(rect.left() + 10.0 + i as f32 * 7.0, rect.center().y);
        ui.painter().circle_filled(center, 2.5 + phase * 0.5, p.strong.gamma_multiply(0.4 + phase * 0.6));
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
    let bold = egui::TextFormat::simple(theme::strong(13.5), p.strong);
    let plain = egui::TextFormat::simple(FontId::proportional(13.5), p.text);
    let mut job = egui::text::LayoutJob::default();
    match typing {
        [a] => super::rich_template(&mut job, t!("{a} is typing…"), &plain, &bold, &[("a", a)]),
        [a, b] => super::rich_template(&mut job, t!("{a} and {b} are typing…"), &plain, &bold, &[("a", a), ("b", b)]),
        [a, b, c] => super::rich_template(&mut job, t!("{a}, {b}, and {c} are typing…"), &plain, &bold, &[("a", a), ("b", b), ("c", c)]),
        _ => job.append(t!("Several people are typing…"), 0.0, plain),
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 40.0);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
}

/// The faces Discord's emoji button cycles through.
const BUTTON_FACES: [&str; 24] = [
    "😀", "😃", "😄", "😁", "😆", "😅", "🤣", "😂", "🙂", "🙃", "😉", "😊",
    "😇", "🥰", "😍", "🤩", "😘", "😋", "😛", "😜", "🤪", "😝", "🤗", "😎",
];

/// Discord's emoji button: a face in grey, which on hover takes its colour,
/// grows a little and, each time the pointer comes in, becomes another face.
fn emoji_button(ui: &egui::Ui, response: &egui::Response, rect: Rect) {
    let id = ui.id().with("emoji-button");
    let (mut face, was_hovered) = ui.data(|d| d.get_temp::<(usize, bool)>(id)).unwrap_or((0, false));
    let hovered = response.hovered();
    if hovered && !was_hovered {
        // A different face each time, as Discord's.
        let tick = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        face = (face + 1 + tick % (BUTTON_FACES.len() - 1)) % BUTTON_FACES.len();
    }
    ui.data_mut(|d| d.insert_temp(id, (face, hovered)));
    let side = if hovered { 24.0 * 1.14 } else { 24.0 };
    let px = (24.0 * 1.14 * ui.ctx().pixels_per_point()).round() as u32;
    if let Some(texture) = face_texture(ui.ctx(), BUTTON_FACES[face], px, !hovered) {
        let size = texture.size_vec2();
        let shown = vec2(side * size.x / size.y.max(1.0), side);
        ui.painter().image(texture.id(), Rect::from_center_size(rect.center(), shown), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// An emoji's picture, in colour or in grey, drawn once and kept.
fn face_texture(ctx: &egui::Context, face: &str, px: u32, grey: bool) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("emoji-button-face", face, px, grey));
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(texture);
    }
    let picture = fastframe_emoji::get().render(face, px)?;
    let mut rgba = picture.rgba;
    if grey {
        // Discord's filter: grayscale, keeping the face's lightness.
        for pixel in rgba.chunks_mut(4) {
            let luma = (0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2])).round() as u8;
            pixel[..3].fill(luma);
        }
    }
    let image = egui::ColorImage::from_rgba_premultiplied(picture.size, &rgba);
    let texture = ctx.load_texture(format!("emoji-button-{face}-{px}-{grey}"), image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// The longest message you may send: 4000 characters with Nitro.
fn message_limit(app: &App) -> usize {
    if app.me.as_ref().is_some_and(|m| m.premium == 2) { 4000 } else { 2000 }
}

/// The clipboard's picture saved as a PNG to attach, when it holds one.
fn pasted_image() -> Option<std::path::PathBuf> {
    let image = arboard::Clipboard::new().ok()?.get_image().ok()?;
    let picture = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())?;
    let stamp = jiff::Timestamp::now().as_millisecond();
    let path = std::env::temp_dir().join(format!("image-{stamp}.png"));
    picture.save(&path).ok()?;
    Some(path)
}

fn bar(app: &mut App, ui: &mut egui::Ui, label: &str) {
    let p = theme::p();
    Frame::new()
        .fill(p.sidebar)
        .corner_radius(CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 })
        .inner_margin(Margin::symmetric(14, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(13.0).color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if crate::icons::button(ui, crate::icons::Icon::Close, 22.0, t!("Cancel")).clicked() {
                        app.actions.push(Action::CancelCompose);
                    }
                });
            });
        });
}

/// The member list: sections by hoisted role, then online and offline.
fn member_list(app: &mut App, ui: &mut egui::Ui) {
    use crate::discord::gateway::ListItem;
    let p = theme::p();
    let Some(state) = &mut app.member_list else { return };
    let (guild_id, channel_id, requested) = (state.guild, state.channel, state.requested);
    let counts = state.groups.clone();
    let Some(guild) = app.guild(guild_id).cloned() else { return };
    // Borrowed for the frame, put back below.
    let Some(rows) = app.member_list.as_mut().map(|m| std::mem::take(&mut m.rows)) else { return };
    const ROW: f32 = 44.0;
    egui::ScrollArea::vertical().id_salt(("members", channel_id)).auto_shrink(false).show_rows(ui, ROW, rows.len(), |ui, range| {
        // Asks for the next hundred rows near the end.
        if range.end + 20 >= rows.len() && (rows.len() as u64) + 1 >= requested {
            app.watch_members(guild_id, channel_id, requested + 100);
        }
        for row in &rows[range] {
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            match row {
                None => {}
                Some(ListItem::Group { id, count }) => {
                    // As Discord's list now writes them: "Mods — 1", the
                    // role's name as written.
                    let name = match id.as_str() {
                        "online" => t!("Online").to_owned(),
                        "offline" => t!("Offline").to_owned(),
                        role => role
                            .parse::<Id>()
                            .ok()
                            .and_then(|r| guild.roles.iter().find(|x| x.id == r))
                            .map(|r| r.name.clone())
                            .unwrap_or_default(),
                    };
                    let count = counts.get(id).copied().unwrap_or(*count);
                    ui.painter().text(rect.left_bottom() + vec2(16.0, -8.0), Align2::LEFT_BOTTOM, format!("{name} — {count}"), theme::strong(14.0), p.muted);
                }
                Some(ListItem::Member { user, member, presence }) => {
                    // Discord's row: 42 points inside a 1-point padding.
                    let inner = rect.shrink2(vec2(8.0, 1.0));
                    if response.hovered() {
                        ui.painter().rect_filled(inner, 4.0, p.hover);
                    }
                    let plate = member.nameplate.as_ref().or(user.nameplate.as_ref());
                    let slot = ui.painter().add(egui::Shape::Noop);
                    let mut content = 0.0f32;
                    let offline = presence.as_ref().is_none_or(|p| p.status == "offline");
                    let avatar = Rect::from_min_size(inner.left_center() + vec2(8.0, -16.0), Vec2::splat(32.0));
                    let face = if response.hovered() { user.avatar_moving(64) } else { user.avatar_url() };
                    app.images.circle(ui, avatar, Some(&face), &initials(user.display_name()), 16.0);
                    if let Some(decoration) = user.decoration_url() {
                        crate::icons::decoration(ui, &mut app.images, avatar, &decoration);
                    }
                    if let Some(presence) = presence.as_ref().filter(|_| !offline) {
                        super::friends::dot(ui, avatar, &presence.status, if response.hovered() { p.hover } else { p.sidebar });
                    }
                    let name = member.nick.clone().unwrap_or_else(|| user.display_name().to_owned());
                    let color = if app.settings.role_colors == 0 { theme::role_color(guild.role_color(&member.roles)) } else { None }.unwrap_or(p.muted);
                    let color = if offline { color.linear_multiply(0.5) } else { color };
                    let voice = super::friends::shows_voice(app, user.id);
                    let activity = presence.as_ref().and_then(member_activity);
                    // In voice, the green speaker: with a custom status its
                    // text gives way, as Discord's line.
                    let activity = match activity {
                        Some((custom, bold, id)) if voice => Some((format!("• {custom}"), bold, id)),
                        None if voice => Some((t!("In voice").to_owned(), String::new(), None)),
                        other => other,
                    };
                    let x = avatar.right() + 10.0;
                    // The game's icon at the row's end, as Discord's list.
                    let icon = activity.as_ref().and_then(|(_, _, app_id)| *app_id).and_then(|a| app.app_icon(a));
                    let right = inner.right() - if icon.is_some() { 30.0 } else { 4.0 };
                    if let Some((texture, _)) = icon.as_deref().and_then(|url| app.images.get(url, 40)) {
                        let at = Rect::from_center_size(egui::pos2(inner.right() - 14.0, inner.center().y), Vec2::splat(20.0));
                        egui::Image::from_texture((texture, at.size())).corner_radius(4.0).paint_at(ui, at);
                    }
                    let name_y = if activity.is_some() { inner.center().y - 8.0 } else { inner.center().y };
                    content = content.max(super::name_with_tag(app, ui, Rect::from_min_max(egui::pos2(x, name_y - 9.0), egui::pos2(right, name_y + 9.0)), user, &name, FontId::proportional(15.0), color));
                    if let Some((template, bold, _)) = activity {
                        // "Playing " and the game in bold, as Discord writes it.
                        let mut job = egui::text::LayoutJob::default();
                        super::rich_template(&mut job, &template, &egui::TextFormat::simple(FontId::proportional(12.0), p.muted), &egui::TextFormat::simple(theme::strong(12.0), p.muted), &[("name", &bold)]);
                        let text_x = if voice {
                            crate::icons::paint(ui, Rect::from_center_size(egui::pos2(x + 6.0, inner.center().y + 10.0), Vec2::splat(12.0)), crate::icons::Icon::Voice, p.green);
                            x + 16.0
                        } else {
                            x
                        };
                        job.wrap = egui::text::TextWrapping::truncate_at_width(right - text_x);
                        let galley = ui.fonts_mut(|f| f.layout_job(job));
                        content = content.max(text_x - x + galley.size().x);
                        ui.painter().galley(egui::pos2(text_x, inner.center().y + 10.0 - galley.size().y / 2.0), galley, p.muted);
                    }
                    if let Some(plate) = plate {
                        super::nameplate(app, ui, slot, inner, plate, x - inner.left() + content, response.hovered(), false, super::Plate::Members);
                    }
                    if response.clicked() {
                        app.actions.push(Action::OpenProfile { user: user.clone(), guild: Some(guild_id), at: rect.left_top() - vec2(350.0, 0.0) });
                    }
                    response.context_menu(|ui| {
                        super::menu::begin(ui);
                        super::menu::user(app, ui, &user, Some(guild_id));
                    });
                }
            }
        }
    });
    if let Some(state) = &mut app.member_list
        && state.channel == channel_id
        && state.rows.is_empty()
    {
        state.rows = rows;
    }
}

/// Pinned messages or search results, in the member list's place.
fn side(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let p = theme::p();
    let mut close = false;
    let mut search_now = None;
    Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let title = match &app.side {
                Some(crate::app::Side::Pins(_)) => t!("Pinned Messages"),
                _ => t!("Search"),
            };
            ui.label(RichText::new(title).font(theme::strong(15.0)).color(p.strong));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if crate::icons::button(ui, crate::icons::Icon::Close, 24.0, t!("Close")).clicked() {
                    close = true;
                }
            });
        });
        if let Some(crate::app::Side::Search { query, .. }) = &mut app.side {
            ui.add_space(6.0);
            let response = ui.add(egui::TextEdit::singleline(query).hint_text(t!("Search Messages")).desired_width(f32::INFINITY));
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !query.trim().is_empty() {
                search_now = Some(0);
            }
        }
        if let Some(crate::app::Side::Search { query, tab, oldest, offset, results }) = &mut app.side {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for (i, label) in [t!("Messages"), t!("Media"), t!("Links"), t!("Files")].into_iter().enumerate() {
                    if ui.selectable_label(*tab == i as u8, label).clicked() && *tab != i as u8 {
                        *tab = i as u8;
                        search_now = Some(0);
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.selectable_label(!*oldest, t!("Newest")).clicked() && *oldest {
                    *oldest = false;
                    search_now = Some(0);
                }
                if ui.selectable_label(*oldest, t!("Oldest")).clicked() && !*oldest {
                    *oldest = true;
                    search_now = Some(0);
                }
                let total = results.as_ref().map_or(0, |(total, _)| *total);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if *offset + 25 < total && ui.small_button("›").clicked() {
                        search_now = Some(*offset + 25);
                    }
                    if *offset > 0 && ui.small_button("‹").clicked() {
                        search_now = Some(offset.saturating_sub(25));
                    }
                });
            });
            if query.trim().is_empty() && *tab == 0 {
                search_now = None;
            }
        }
    });
    if let Some(page) = search_now {
        let Some(crate::app::Side::Search { query, tab, oldest, .. }) = &app.side else { return };
        // "from:name" names someone known here; the rest is the text.
        let mut words = Vec::new();
        let mut filters = String::new();
        for word in query.split_whitespace() {
            match word.strip_prefix("from:").map(|name| app.users.values().find(|u| u.username.eq_ignore_ascii_case(name) || u.display_name().eq_ignore_ascii_case(name))) {
                Some(Some(user)) => filters.push_str(&format!("&author_id={}", user.id)),
                _ => words.push(word),
            }
        }
        let encoded = crate::discord::rest::percent_encode(&words.join(" "));
        filters.push_str(match tab {
            1 => "&has=image&has=video",
            2 => "&has=link",
            3 => "&has=file",
            _ => "",
        });
        if *oldest {
            filters.push_str("&sort_by=timestamp&sort_order=asc");
        }
        filters.push_str(&format!("&offset={page}"));
        let path = match channel.guild_id {
            Some(guild) => format!("/guilds/{guild}/messages/search?content={encoded}{filters}"),
            None => format!("/channels/{}/messages/search?content={encoded}{filters}", channel.id),
        };
        if let Some(crate::app::Side::Search { results, offset, .. }) = &mut app.side {
            *results = None;
            *offset = page;
        }
        app.commands.send(crate::admin::get("search", path));
    }
    let (list, header) = match &app.side {
        Some(crate::app::Side::Pins(Some(list))) => (Some(list.clone()), tn!(list.len(), "{count} Pinned Message", "{count} Pinned Messages")),
        Some(crate::app::Side::Search { results: Some((total, list)), .. }) => (Some(list.clone()), tn!(*total, "{count} Result", "{count} Results")),
        Some(crate::app::Side::Search { results: None, query, .. }) if query.is_empty() => (Some(Vec::new()), String::new()),
        _ => (None, String::new()),
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        Frame::new().inner_margin(Margin::symmetric(12, 0)).show(ui, |ui| {
            let Some(list) = list else {
                ui.spinner();
                return;
            };
            if !header.is_empty() {
                ui.label(RichText::new(header).size(12.0).color(p.muted));
            }
            for message in list {
                let response = Frame::new()
                    .fill(p.chat)
                    .corner_radius(6)
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(message.author.display_name()).font(theme::strong(14.0)).color(p.strong));
                            ui.label(RichText::new(text::time(message.timestamp_ms())).size(11.5).color(p.faint));
                        });
                        let blocks = markdown::parse(&message.content);
                        markdown::show(app, ui, &blocks, p.text, message.id);
                        if !message.attachments.is_empty() {
                            ui.label(RichText::new(tn!(message.attachments.len(), "{count} attachment", "{count} attachments")).size(12.0).color(p.muted));
                        }
                    })
                    .response
                    .interact(Sense::click());
                if response.on_hover_text(t!("Jump")).clicked() {
                    let guild = message.guild_id.or_else(|| app.channel(message.channel_id).and_then(|c| c.guild_id));
                    app.actions.push(Action::SelectGuild(guild));
                    app.actions.push(Action::SelectChannel(message.channel_id));
                    app.actions.push(Action::JumpTo { channel: message.channel_id, message: message.id });
                }
                ui.add_space(6.0);
            }
        });
    });
    if close {
        app.side = None;
    }
}

/// Dialogs: add a server (invite or new), name a thread.
pub fn modal(app: &mut App, ctx: &egui::Context) {
    let p = theme::p();
    let Some(mut modal) = app.modal.take() else { return };
    let mut keep = true;
    // Discord's footer: "Cancelar" and the action at the right.
    let footer = |ui: &mut egui::Ui, label: &str, fill: Color32, enabled: bool| -> (bool, bool) {
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let go = ui.add_enabled(enabled, egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(fill).min_size(vec2(96.0, 36.0))).clicked();
            let cancel = ui.add(egui::Button::new(RichText::new(t!("Cancel")).color(p.text)).frame(false).min_size(vec2(80.0, 36.0))).clicked();
            (go, cancel)
        })
        .inner
    };
    let mut own_footer = false;
    let response = egui::Modal::new("cordfast-modal".into()).show(ctx, |ui| {
        ui.set_width(420.0);
        // Enter confirms whichever pop-up is open, as Discord's do: taken
        // before the fields, so a note's box gets no new line from it.
        let enter = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        match &mut modal {
            crate::app::Modal::Confirm { title, body, button, then } => {
                own_footer = true;
                ui.label(RichText::new(title.as_str()).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(8.0);
                for (i, paragraph) in body.split("\n\n").enumerate() {
                    if i > 0 {
                        ui.add_space(8.0);
                    }
                    let tip = paragraph.starts_with(t!("PROTIP:"));
                    ui.label(RichText::new(paragraph).size(if tip { 12.5 } else { 15.0 }).color(if tip { p.muted } else { p.text }));
                }
                if matches!(then, crate::app::Confirmed::SwitchVoice { .. }) {
                    // Discord turns the warning off as soon as it is ticked.
                    ui.add_space(12.0);
                    let mut skip = !app.settings.switch_voice_warning;
                    if ui.checkbox(&mut skip, t!("Don't ask again")).changed() {
                        app.settings.switch_voice_warning = !skip;
                        app.actions.push(crate::app::Action::SettingsChanged);
                    }
                }
                let (go, cancel) = footer(ui, button, if then.danger() { p.red } else { p.accent }, true);
                if go || enter {
                    then.run(app);
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            }
            crate::app::Modal::Forward { channel, guild, message, picked, filter, note } => {
                own_footer = true;
                ui.label(RichText::new(t!("Forward To")).font(theme::strong(20.0)).color(p.strong));
                ui.label(RichText::new(t!("Choose up to 5 conversations.")).size(13.0).color(p.muted));
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(filter).hint_text(t!("Search")).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                let needle = filter.to_lowercase();
                // DMs and the text channels you can write in, the most
                // recently active first, as Discord lists them.
                let me = app.me_id();
                let mut places: Vec<(Id, String, String, Option<String>)> = Vec::new();
                for dm in &app.dms {
                    let avatar = (dm.kind == ChannelKind::Dm).then(|| dm.recipients.first().map(|u| u.avatar_url())).flatten();
                    places.push((dm.id, dm.title(), String::new(), avatar));
                }
                for g in &app.guilds {
                    for c in &g.channels {
                        if matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement) && g.channel_permissions(me, c) & ((1 << 10) | (1 << 11)) == (1 << 10) | (1 << 11) {
                            places.push((c.id, format!("#{}", c.name), g.name.clone(), None));
                        }
                    }
                }
                places.retain(|(_, title, server, _)| needle.is_empty() || title.to_lowercase().contains(&needle) || server.to_lowercase().contains(&needle));
                places.sort_by_key(|(id, ..)| std::cmp::Reverse(app.latest.get(id).copied().unwrap_or(0)));
                places.truncate(60);
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (id, title, server, avatar) in places {
                        let on = picked.contains(&id);
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 4.0, p.hover);
                        }
                        let icon = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 14.0), Vec2::splat(28.0));
                        if server.is_empty() {
                            app.images.circle(ui, icon, avatar.as_deref(), &initials(&title), 14.0);
                        } else {
                            crate::icons::paint(ui, icon.shrink(4.0), crate::icons::Icon::Text, p.muted);
                        }
                        let text = Rect::from_min_max(pos2(icon.right() + 10.0, rect.top()), pos2(rect.right() - 40.0, rect.bottom()));
                        if server.is_empty() {
                            super::paint_truncated(ui, text, &title, FontId::proportional(15.0), p.strong);
                        } else {
                            super::paint_truncated(ui, Rect::from_min_max(text.min, pos2(text.right(), text.center().y + 2.0)), &title, FontId::proportional(15.0), p.strong);
                            super::paint_truncated(ui, Rect::from_min_max(pos2(text.left(), text.center().y), text.max), &server, FontId::proportional(12.0), p.muted);
                        }
                        let check = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(20.0));
                        if on {
                            ui.painter().rect_filled(check, 4.0, p.accent);
                            let tick = [check.left_center() + vec2(4.0, 0.0), check.center_bottom() + vec2(-2.0, -5.0), check.right_top() + vec2(-4.0, 5.0)];
                            ui.painter().add(Shape::line(tick.to_vec(), Stroke::new(2.0, Color32::WHITE)));
                        } else {
                            ui.painter().rect_stroke(check, 4.0, Stroke::new(2.0, p.muted), egui::StrokeKind::Inside);
                        }
                        if response.clicked() {
                            if on {
                                picked.retain(|c| *c != id);
                            } else if picked.len() < 5 {
                                picked.push(id);
                            }
                        }
                    }
                });
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(note).hint_text(t!("Add an optional message…")).desired_width(f32::INFINITY));
                let (go, cancel) = footer(ui, t!("Send"), p.accent, !picked.is_empty());
                if go || (enter && !picked.is_empty()) {
                    for target in picked.iter() {
                        let mut reference = serde_json::json!({ "type": 1, "message_id": message.to_string(), "channel_id": channel.to_string() });
                        if let Some(guild) = guild {
                            reference["guild_id"] = guild.to_string().into();
                        }
                        app.commands.send(crate::backend::Command::Api {
                            key: "forward".into(),
                            method: "POST",
                            path: format!("/channels/{target}/messages"),
                            body: Some(serde_json::json!({ "message_reference": reference, "flags": 0, "nonce": crate::discord::model::now_snowflake().to_string() })),
                        });
                        if !note.trim().is_empty() {
                            app.commands.send(crate::backend::Command::Send {
                                channel: *target,
                                content: note.trim().to_owned(),
                                nonce: crate::discord::model::now_snowflake().to_string(),
                                reply_to: None,
                                ping_reply: false,
                                silent: false,
                                files: Vec::new(),
                            });
                        }
                    }
                    app.notify(if picked.len() == 1 { t!("Message forwarded.") } else { t!("Message forwarded to the conversations.") });
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            }
            crate::app::Modal::Nickname { guild, user, nick } => {
                own_footer = true;
                ui.label(RichText::new(t!("Change Nickname")).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(10.0);
                ui.label(RichText::new(t!("Nickname").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                ui.add(egui::TextEdit::singleline(nick).char_limit(32).desired_width(f32::INFINITY));
                if ui.add(egui::Label::new(RichText::new(t!("Reset Nickname")).size(13.0).color(p.link)).sense(Sense::click())).clicked() {
                    nick.clear();
                }
                let (go, cancel) = footer(ui, t!("Save"), p.accent, true);
                if go || enter {
                    let target = if *user == app.me_id() { "@me".to_owned() } else { user.to_string() };
                    let value = nick.trim();
                    app.commands.send(crate::backend::Command::Api {
                        key: "nick".into(),
                        method: "PATCH",
                        path: format!("/guilds/{guild}/members/{target}"),
                        body: Some(serde_json::json!({ "nick": if value.is_empty() { serde_json::Value::Null } else { value.into() } })),
                    });
                    if let Some(member) = app.members.get_mut(&(*guild, *user)) {
                        member.nick = (!value.is_empty()).then(|| value.to_owned());
                    }
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            }
            crate::app::Modal::Note { user, name, note } => {
                own_footer = true;
                ui.label(RichText::new(t!("Note")).font(theme::strong(20.0)).color(p.strong));
                ui.label(RichText::new(t!("About @{name}, only visible to you", name = name)).size(13.0).color(p.muted));
                ui.add_space(8.0);
                ui.add(egui::TextEdit::multiline(note).char_limit(256).desired_rows(3).hint_text(t!("Click to add a note")).desired_width(f32::INFINITY));
                let (go, cancel) = footer(ui, t!("Save"), p.accent, true);
                if go || enter {
                    app.commands.send(crate::backend::Command::Api {
                        key: "note".into(),
                        method: "PUT",
                        path: format!("/users/@me/notes/{user}"),
                        body: Some(serde_json::json!({ "note": note.trim() })),
                    });
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            }
            crate::app::Modal::Attachment { index, upload } => {
                own_footer = true;
                ui.label(RichText::new(t!("Modify Attachment")).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(10.0);
                ui.label(RichText::new(t!("Filename").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                ui.add(egui::TextEdit::singleline(&mut upload.name).desired_width(f32::INFINITY));
                ui.add_space(8.0);
                ui.label(RichText::new(t!("Description (Alt Text)").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                ui.add(egui::TextEdit::singleline(&mut upload.description).char_limit(1024).hint_text(t!("Add a description")).desired_width(f32::INFINITY));
                ui.add_space(8.0);
                ui.checkbox(&mut upload.spoiler, t!("Mark as spoiler"));
                let (go, cancel) = footer(ui, t!("Save"), p.accent, !upload.name.trim().is_empty());
                if (go || enter) && !upload.name.trim().is_empty() {
                    upload.name = upload.name.trim().to_owned();
                    if let Some(slot) = app.files.get_mut(*index) {
                        *slot = upload.clone();
                    }
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            }
            crate::app::Modal::VoiceStatus { channel, status } => {
                own_footer = true;
                ui.label(RichText::new(t!("Set a channel status")).font(theme::strong(20.0)).color(p.strong));
                ui.label(RichText::new(t!("Everyone in the channel sees it under its name.")).size(13.0).color(p.muted));
                ui.add_space(8.0);
                ui.add(egui::TextEdit::singleline(status).char_limit(500).hint_text(t!("What's going on?")).desired_width(f32::INFINITY)).request_focus();
                let (go, cancel) = footer(ui, t!("Save"), p.accent, true);
                if go || enter {
                    app.commands.send(crate::backend::Command::Api {
                        key: "voice-status".into(),
                        method: "PUT",
                        path: format!("/channels/{channel}/voice-status"),
                        body: Some(serde_json::json!({ "status": status.trim() })),
                    });
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            }
            crate::app::Modal::AddServer { invite, name } => {
                ui.label(RichText::new(t!("Join a Server")).font(theme::strong(18.0)).color(p.strong));
                ui.label(RichText::new(t!("Paste an invite, such as discord.gg/abc123")).color(p.muted));
                ui.add(egui::TextEdit::singleline(invite).hint_text("https://discord.gg/...").desired_width(f32::INFINITY));
                // Enter joins with an invite written, or else creates.
                let joining = enter && !invite.trim().is_empty();
                if (ui.add(egui::Button::new(RichText::new(t!("Join")).color(Color32::WHITE)).fill(p.accent)).clicked() || joining) && !invite.trim().is_empty() {
                    let code = crate::app::invite_code(invite);
                    app.commands.send(crate::backend::Command::Api {
                        key: "join".into(),
                        method: "POST",
                        path: format!("/invites/{code}"),
                        body: Some(serde_json::json!({})),
                    });
                }
                ui.add_space(14.0);
                ui.separator();
                ui.label(RichText::new(t!("Create Your Server")).font(theme::strong(18.0)).color(p.strong));
                ui.add(egui::TextEdit::singleline(name).hint_text(t!("Server Name")).desired_width(f32::INFINITY));
                if (ui.add(egui::Button::new(RichText::new(t!("Create")).color(Color32::WHITE)).fill(p.green)).clicked() || (enter && !joining)) && !name.trim().is_empty() {
                    app.commands.send(crate::backend::Command::Api {
                        key: "create".into(),
                        method: "POST",
                        path: "/guilds".into(),
                        body: Some(serde_json::json!({ "name": name.trim() })),
                    });
                }
            }
            crate::app::Modal::CustomStatus { text, minutes } => {
                ui.label(RichText::new(t!("Set a custom status")).font(theme::strong(18.0)).color(p.strong));
                ui.add_space(6.0);
                let me = app.me.as_ref().map(|m| m.display_name().to_uppercase()).unwrap_or_default();
                ui.label(RichText::new(t!("What's cookin', {name}?", name = me).to_uppercase()).font(theme::strong(11.5)).color(p.muted));
                ui.add(egui::TextEdit::singleline(text).hint_text(t!("Support has arrived!")).char_limit(128).desired_width(f32::INFINITY));
                ui.add_space(8.0);
                ui.label(RichText::new(t!("Clear after").to_uppercase()).size(11.5).font(theme::strong(11.5)).color(p.muted));
                let options: [(&str, Option<u64>); 5] = [
                    (n!("Today"), Some(1440)),
                    (n!("4 hours"), Some(240)),
                    (n!("1 hour"), Some(60)),
                    (n!("30 minutes"), Some(30)),
                    (n!("Don't clear"), None),
                ];
                let current = crate::i18n::tr(options.iter().find(|(_, m)| m == minutes).map_or(options[0].0, |(l, _)| *l));
                egui::ComboBox::from_id_salt("custom-status-clear")
                    .selected_text(current)
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (label, value) in options {
                            ui.selectable_value(minutes, value, crate::i18n::tr(label));
                        }
                    });
                ui.add_space(10.0);
                if ui.add(egui::Button::new(RichText::new(t!("Save")).color(Color32::WHITE)).fill(p.accent)).clicked() || enter {
                    app.actions.push(Action::SetCustomStatus { text: Some(text.clone()), minutes: *minutes });
                }
            }
            crate::app::Modal::NewDm { picked, filter } => {
                ui.label(RichText::new(t!("Select Friends")).font(theme::strong(18.0)).color(p.strong));
                ui.label(RichText::new(tn!(9usize.saturating_sub(picked.len()), "You can add {count} more friend.", "You can add {count} more friends.")).size(12.5).color(p.muted));
                ui.add(egui::TextEdit::singleline(filter).hint_text(t!("Type the username of a friend")).desired_width(f32::INFINITY));
                let needle = filter.to_lowercase();
                let mut friends: Vec<crate::discord::model::User> = app
                    .relationships
                    .values()
                    .filter(|r| r.kind == 1)
                    .map(|r| r.user.clone())
                    .filter(|u| needle.is_empty() || u.display_name().to_lowercase().contains(&needle) || u.username.to_lowercase().contains(&needle))
                    .collect();
                friends.sort_by_key(|u| u.display_name().to_lowercase());
                ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for user in friends {
                        let mut on = picked.contains(&user.id);
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                            app.images.circle(ui, rect, Some(&user.avatar_url()), &initials(user.display_name()), 14.0);
                            if ui.checkbox(&mut on, format!("{}  ({})", user.display_name(), user.username)).changed() {
                                if on && picked.len() < 9 {
                                    picked.push(user.id);
                                } else {
                                    picked.retain(|u| *u != user.id);
                                }
                            }
                        });
                    }
                });
                let label = if picked.len() > 1 { t!("Create Group") } else { t!("Create DM") };
                if ui.add_enabled(!picked.is_empty(), egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(p.accent)).clicked() || (enter && !picked.is_empty()) {
                    app.actions.push(Action::NewDm(picked.clone()));
                }
            }
            crate::app::Modal::Mfa { ticket, password, code, key, method, path, body } => {
                ui.label(RichText::new(t!("Multi-Factor Authentication")).font(theme::strong(18.0)).color(p.strong));
                let (note, hint) = if *password { (t!("Enter your password to continue."), t!("Password")) } else { (t!("Open your authentication app and enter the code below."), t!("Enter Discord Auth/Backup Code")) };
                ui.label(RichText::new(note).size(13.0).color(p.muted));
                ui.add_space(6.0);
                let field = ui.add(egui::TextEdit::singleline(code).password(*password).hint_text(hint).desired_width(f32::INFINITY));
                field.request_focus();
                ui.add_space(6.0);
                let verify = ui.add(egui::Button::new(RichText::new(t!("Verify")).color(Color32::WHITE)).fill(p.accent)).clicked();
                if (verify || enter) && !code.trim().is_empty() {
                    // Backup codes are eight characters, the app's are six.
                    let mfa_type = if *password { "password" } else if code.trim().chars().filter(char::is_ascii_alphanumeric).count() > 6 { "backup" } else { "totp" };
                    app.commands.send(crate::backend::Command::MfaFinish { ticket: ticket.clone(), mfa_type, code: code.trim().to_owned(), key: key.clone(), method, path: path.clone(), body: body.clone() });
                    keep = false;
                }
            }
            crate::app::Modal::Gate(gate) => {
                own_footer = true;
                if !super::gates::show(app, ui, gate) {
                    keep = false;
                }
            }
            crate::app::Modal::NewPost { forum, title, body } => {
                own_footer = true;
                ui.label(RichText::new(t!("New Post")).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(8.0);
                ui.add(egui::TextEdit::singleline(title).hint_text(t!("Post Title")).char_limit(100).desired_width(f32::INFINITY));
                ui.add_space(6.0);
                ui.add(egui::TextEdit::multiline(body).hint_text(t!("Message")).desired_rows(5).desired_width(f32::INFINITY));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("Cancel")).clicked() {
                        keep = false;
                    }
                    if ui.add_enabled(!title.trim().is_empty() && !body.trim().is_empty(), egui::Button::new(RichText::new(t!("Post")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                        app.commands.send(crate::backend::Command::Api { key: "thread-made".into(), method: "POST", path: format!("/channels/{forum}/threads"), body: Some(serde_json::json!({ "name": title.trim(), "auto_archive_duration": 1440, "message": { "content": body.trim() } })) });
                        keep = false;
                    }
                });
            }
            crate::app::Modal::Threads { channel, name } => {
                ui.label(RichText::new(t!("Threads")).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(name).hint_text(t!("New Thread")).desired_width(280.0));
                    if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(RichText::new(t!("Create")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                        // 11, a public thread, with no message under it.
                        app.commands.send(crate::backend::Command::Api { key: "thread-made".into(), method: "POST", path: format!("/channels/{channel}/threads"), body: Some(serde_json::json!({ "name": name.trim(), "type": 11, "auto_archive_duration": 1440 })) });
                        keep = false;
                    }
                });
                ui.add_space(8.0);
                let active: Vec<(Id, String)> = app.guilds.iter().flat_map(|g| g.channels.iter()).filter(|c| c.kind == crate::discord::model::ChannelKind::Thread && c.parent_id == Some(*channel) && !c.archived).map(|c| (c.id, c.title())).collect();
                let archived: Option<Vec<(Id, String)>> = app.archived_threads.as_ref().map(|list| list.iter().map(|c| (c.id, c.title())).collect());
                ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    ui.label(RichText::new(t!("Active Threads").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                    for (id, title) in &active {
                        if ui.selectable_label(false, title).clicked() {
                            app.actions.push(Action::SelectChannel(*id));
                            keep = false;
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(t!("Archived Threads").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                    match &archived {
                        Some(list) => {
                            for (id, title) in list {
                                if ui.selectable_label(false, title).clicked() {
                                    app.actions.push(Action::SelectChannel(*id));
                                    keep = false;
                                }
                            }
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                });
            }
            crate::app::Modal::Reactions { channel, message, emojis, picked, manage } => {
                ui.label(RichText::new(t!("Reactions")).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    let mut choose = None;
                    for (i, emoji) in emojis.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
                        ui.painter().rect_filled(rect, 6.0, if i == *picked { p.selected } else if response.hovered() { p.hover } else { p.sidebar });
                        let shown = Rect::from_center_size(rect.center(), Vec2::splat(20.0));
                        match emoji.url() {
                            Some(url) => {
                                if let Some((texture, _)) = app.images.get(&url, 40) {
                                    egui::Image::from_texture((texture, shown.size())).paint_at(ui, shown);
                                }
                            }
                            None => super::markdown::paint_twemoji(app, ui, &emoji.name, shown),
                        }
                        if response.clicked() && i != *picked {
                            choose = Some(i);
                        }
                    }
                    if let Some(i) = choose {
                        *picked = i;
                        ask_reactors(app, *channel, *message, &emojis[i]);
                    }
                });
                ui.add_space(8.0);
                ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    let Some(users) = app.reactors.clone() else {
                        ui.spinner();
                        return;
                    };
                    let me = app.me_id();
                    for user in users {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                            app.images.circle(ui, rect, Some(&user.avatar_url()), &initials(user.display_name()), 14.0);
                            ui.label(RichText::new(user.display_name()).color(p.strong));
                            ui.label(RichText::new(&user.username).size(12.0).color(p.muted));
                            if (*manage || user.id == me) && ui.small_button("×").on_hover_text(t!("Remove")).clicked() {
                                let name = crate::discord::rest::percent_encode(&emojis[*picked].api_name());
                                app.commands.send(call("DELETE", format!("/channels/{channel}/messages/{message}/reactions/{name}/0/{}", user.id), None));
                                if let Some(list) = &mut app.reactors {
                                    list.retain(|u| u.id != user.id);
                                }
                            }
                        });
                    }
                });
            }
            crate::app::Modal::NewPoll { channel, question, answers, hours, multiple } => {
                own_footer = true;
                ui.label(RichText::new(t!("Create Poll")).font(theme::strong(20.0)).color(p.strong));
                ui.add_space(10.0);
                ui.label(RichText::new(t!("Question").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                ui.add(egui::TextEdit::singleline(question).char_limit(300).desired_width(f32::INFINITY));
                ui.add_space(10.0);
                ui.label(RichText::new(t!("Answers").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                let mut remove = None;
                for (i, answer) in answers.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(answer).char_limit(55).desired_width(370.0));
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove.filter(|_| answers.len() > 1) {
                    answers.remove(i);
                }
                if answers.len() < 10 && super::form::button(ui, t!("Add another answer"), super::form::Kind::Secondary).clicked() {
                    answers.push(String::new());
                }
                ui.add_space(10.0);
                // The durations of the bundle's picker, in hours.
                const HOURS: [u64; 7] = [1, 4, 8, 24, 72, 168, 336];
                ui.label(RichText::new(t!("Duration").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                let label = |hours: u64| if hours < 24 { tn!(hours, "{count} hour", "{count} hours") } else { tn!(hours / 24, "{count} day", "{count} days") };
                let options: Vec<String> = HOURS.iter().map(|h| label(*h)).collect();
                if let Some(i) = super::form::select(ui, "poll-duration", &label(*hours), &options, 200.0) {
                    *hours = HOURS[i];
                }
                ui.add_space(10.0);
                super::form::switch_row(ui, multiple, t!("Allow Multiple Answers"), "");
                ui.add_space(12.0);
                let filled: Vec<&String> = answers.iter().filter(|a| !a.trim().is_empty()).collect();
                let ready = !question.trim().is_empty() && !filled.is_empty();
                ui.horizontal(|ui| {
                    if super::form::button(ui, t!("Cancel"), super::form::Kind::Secondary).clicked() {
                        keep = false;
                    }
                    if ui.add_enabled(ready, egui::Button::new(RichText::new(t!("Post")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                        let list: Vec<serde_json::Value> = filled.iter().map(|text| serde_json::json!({ "poll_media": { "text": text.trim() } })).collect();
                        app.commands.send(call("POST", format!("/channels/{channel}/messages"), Some(serde_json::json!({ "content": "", "poll": { "question": { "text": question.trim() }, "answers": list, "duration": *hours, "allow_multiselect": *multiple, "layout_type": 1 } }))));
                        keep = false;
                    }
                });
            }
            // Discord's "Follow #info": a server and a text channel where we
            // may manage webhooks get the announcements
            // (POST /channels/{id}/followers).
            crate::app::Modal::Follow { channel, guild, target } => {
                ui.label(RichText::new(t!("Follow Updates")).font(theme::strong(18.0)).color(p.strong));
                ui.label(RichText::new(t!("Follow to get this channel's updates in your own server.")).color(p.muted));
                ui.add_space(8.0);
                let me = app.me_id();
                const MANAGE_WEBHOOKS: u64 = 1 << 29;
                let servers: Vec<(Id, String)> = app.guilds.iter().filter(|g| g.channels.iter().any(|c| c.kind == ChannelKind::Text && g.can_view(me, c) && g.channel_permissions(me, c) & MANAGE_WEBHOOKS != 0)).map(|g| (g.id, g.name.clone())).collect();
                ui.label(RichText::new(t!("Select a server")).font(theme::strong(13.0)).color(p.strong));
                let shown = guild.and_then(|g| servers.iter().find(|(id, _)| *id == g)).map_or_else(|| t!("Select a server").to_owned(), |(_, name)| name.clone());
                egui::ComboBox::from_id_salt("follow-server").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                    for (id, name) in &servers {
                        if ui.selectable_label(*guild == Some(*id), name).clicked() {
                            *guild = Some(*id);
                            *target = None;
                        }
                    }
                });
                let channels: Vec<(Id, String)> = guild.and_then(|g| app.guild(g)).map(|g| g.channels.iter().filter(|c| c.kind == ChannelKind::Text && g.can_view(me, c) && g.channel_permissions(me, c) & MANAGE_WEBHOOKS != 0).map(|c| (c.id, c.title())).collect()).unwrap_or_default();
                ui.add_space(6.0);
                ui.label(RichText::new(t!("Select a channel")).font(theme::strong(13.0)).color(p.strong));
                let shown = target.and_then(|t| channels.iter().find(|(id, _)| *id == t)).map_or_else(|| t!("Select a channel").to_owned(), |(_, name)| format!("# {name}"));
                egui::ComboBox::from_id_salt("follow-channel").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                    for (id, name) in &channels {
                        if ui.selectable_label(*target == Some(*id), format!("# {name}")).clicked() {
                            *target = Some(*id);
                        }
                    }
                });
                ui.add_space(10.0);
                if ui.add_enabled(target.is_some(), egui::Button::new(RichText::new(t!("Follow")).color(Color32::WHITE)).fill(p.accent)).clicked()
                    && let Some(to) = target
                {
                    app.commands.send(call("POST", format!("/channels/{channel}/followers"), Some(serde_json::json!({ "webhook_channel_id": to.to_string() }))));
                    keep = false;
                }
            }
            crate::app::Modal::GroupName { channel, name } => {
                ui.label(RichText::new(t!("Edit Group")).font(theme::strong(18.0)).color(p.strong));
                ui.label(RichText::new(t!("Group Name").to_uppercase()).font(theme::strong(12.0)).color(p.muted));
                ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                if ui.add(egui::Button::new(RichText::new(t!("Save")).color(Color32::WHITE)).fill(p.accent)).clicked() || enter {
                    app.commands.send(call("PATCH", format!("/channels/{channel}"), Some(serde_json::json!({ "name": name.trim() }))));
                    keep = false;
                }
            }
            crate::app::Modal::GroupAdd { channel, picked, filter } => {
                ui.label(RichText::new(t!("Add Friends to DM")).font(theme::strong(18.0)).color(p.strong));
                ui.add(egui::TextEdit::singleline(filter).hint_text(t!("Search")).desired_width(f32::INFINITY));
                ui.add_space(6.0);
                let already: Vec<Id> = app.channel(*channel).map(|c| c.recipients.iter().map(|u| u.id).collect()).unwrap_or_default();
                let needle = filter.to_lowercase();
                let mut friends: Vec<crate::discord::model::User> = app.relationships.values().filter(|r| r.kind == 1).map(|r| r.user.clone()).filter(|u| !already.contains(&u.id) && u.display_name().to_lowercase().contains(&needle)).collect();
                friends.sort_by_key(|u| u.display_name().to_lowercase());
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for user in friends {
                        let mut on = picked.contains(&user.id);
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                            app.images.circle(ui, rect, Some(&user.avatar_url()), &initials(user.display_name()), 14.0);
                            if ui.checkbox(&mut on, user.display_name()).changed() {
                                if on {
                                    picked.push(user.id);
                                } else {
                                    picked.retain(|u| *u != user.id);
                                }
                            }
                        });
                    }
                });
                if ui.add_enabled(!picked.is_empty(), egui::Button::new(RichText::new(t!("Add")).color(Color32::WHITE)).fill(p.accent)).clicked() {
                    for user in picked.iter() {
                        app.commands.send(call("PUT", format!("/channels/{channel}/recipients/{user}"), None));
                    }
                    keep = false;
                }
            }
            crate::app::Modal::NewThread { channel, message, name } => {
                ui.label(RichText::new(t!("Create Thread")).font(theme::strong(18.0)).color(p.strong));
                ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                if (ui.add(egui::Button::new(RichText::new(t!("Create")).color(Color32::WHITE)).fill(p.accent)).clicked() || enter) && !name.trim().is_empty() {
                    app.commands.send(crate::backend::Command::Api {
                        key: "thread".into(),
                        method: "POST",
                        path: format!("/channels/{channel}/messages/{message}/threads"),
                        body: Some(serde_json::json!({ "name": name.trim(), "auto_archive_duration": 1440 })),
                    });
                }
            }
        }
        if !own_footer {
            ui.add_space(8.0);
            if ui.button(t!("Cancel")).clicked() {
                keep = false;
            }
        }
    });
    if response.should_close() {
        keep = false;
    }
    if keep && app.modal.is_none() {
        app.modal = Some(modal);
    }
}

/// A video: its first frame from Discord's media proxy with a play button;
/// a click plays it in the system's player.
fn video_view(app: &mut App, ui: &mut egui::Ui, proxy: &str, original: &str, w: u32, h: u32) {
    let p = theme::p();
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let scale = (400.0 / w).min(300.0 / h).min(1.0);
    let size = vec2((w * scale).round(), (h * scale).round());
    let ppp = ui.ctx().pixels_per_point();
    let separator = if proxy.contains('?') { '&' } else { '?' };
    let frame = format!("{proxy}{separator}format=jpeg&width={}&height={}", (size.x * ppp) as u32, (size.y * ppp) as u32);
    ui.add_space(4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    #[cfg(windows)]
    if app.video.as_ref().is_some_and(|video| video.url == original) {
        video_playing(app, ui, rect, &response);
        return;
    }
    match app.images.get(&frame, (size.x.max(size.y) * ppp) as u32) {
        Some((texture, _)) => {
            egui::Image::from_texture((texture, size)).corner_radius(6.0).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 6.0, p.rail);
        }
    }
    let hovered = response.hovered();
    ui.painter().circle_filled(rect.center(), 24.0, Color32::from_black_alpha(if hovered { 220 } else { 160 }));
    let c = rect.center();
    ui.painter().add(egui::Shape::convex_polygon(
        vec![egui::pos2(c.x - 7.0, c.y - 11.0), egui::pos2(c.x - 7.0, c.y + 11.0), egui::pos2(c.x + 12.0, c.y)],
        Color32::WHITE,
        Stroke::NONE,
    ));
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t!("Play")).clicked() {
        #[cfg(windows)]
        {
            let volume = app.settings.output_volume.min(1.0);
            app.video = Some(crate::video::Player::start(original.to_owned(), app.wake.clone(), app.settings.output_device.clone(), volume));
        }
        #[cfg(not(windows))]
        let _ = open::that_detached(original);
    }
}

/// The video playing in its message: the picture, a click to pause, and the
/// time over a bar along the bottom.
#[cfg(windows)]
fn video_playing(app: &mut App, ui: &mut egui::Ui, rect: Rect, response: &egui::Response) {
    let p = theme::p();
    let Some(video) = app.video.as_mut() else { return };
    if let Some(error) = video.error() {
        // What Windows cannot read still opens in the system's player.
        log::warn!("video: {error}");
        let url = video.url.clone();
        app.video = None;
        let _ = open::that_detached(url);
        return;
    }
    let ctx = ui.ctx().clone();
    match video.frame(&ctx) {
        Some(texture) => {
            egui::Image::from_texture((texture.id(), rect.size())).corner_radius(6.0).paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 6.0, p.rail);
            ui.put(Rect::from_center_size(rect.center(), Vec2::splat(24.0)), egui::Spinner::new());
        }
    }
    let (at, length) = video.times();
    let clock = |ms: u64| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60);
    let bar = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 26.0), rect.right_bottom());
    if response.hovered() || video.paused() || video.finished() {
        ui.painter().rect_filled(bar, 0.0, Color32::from_black_alpha(150));
        ui.painter().text(egui::pos2(bar.left() + 8.0, bar.center().y), Align2::LEFT_CENTER, format!("{} / {}", clock(at), clock(length)), FontId::proportional(12.0), Color32::WHITE);
        if length > 0 {
            let full = bar.width() - 90.0;
            let track = Rect::from_min_size(egui::pos2(bar.left() + 82.0, bar.center().y - 2.0), vec2(full, 4.0));
            ui.painter().rect_filled(track, 2.0, Color32::from_white_alpha(70));
            ui.painter().rect_filled(Rect::from_min_size(track.min, vec2(full * at as f32 / length as f32, 4.0)), 2.0, p.accent);
        }
    }
    if response.clicked() {
        if video.finished() {
            app.video = None;
        } else {
            video.toggle_pause();
        }
    }
}

/// Editing a message in place: Enter saves, Esc cancels, as in Discord.
fn edit_box(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let id = egui::Id::new("edit-box");
    if ui.ctx().memory(|m| m.has_focus(id)) {
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            app.actions.push(Action::SaveEdit);
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            app.actions.push(Action::CancelEdit);
        }
    }
    Frame::new().fill(p.composer).corner_radius(8).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
        let response = ui.add(
            egui::TextEdit::multiline(&mut app.edit_text)
                .id(id)
                .frame(Frame::NONE)
                .desired_rows(1)
                .desired_width(f32::INFINITY)
                .text_color(p.text),
        );
        if std::mem::take(&mut app.focus_edit) {
            response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let end = egui::text::CCursor::new(app.edit_text.chars().count());
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ui.ctx(), id);
            }
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.label(RichText::new(t!("escape to")).size(12.0).color(p.text));
        if ui.add(egui::Label::new(RichText::new(t!("cancel")).size(12.0).color(p.link)).sense(Sense::click())).clicked() {
            app.actions.push(Action::CancelEdit);
        }
        ui.label(RichText::new(" • ").size(12.0).color(p.text));
        ui.label(RichText::new(t!("enter to")).size(12.0).color(p.text));
        if ui.add(egui::Label::new(RichText::new(t!("save")).size(12.0).color(p.link)).sense(Sense::click())).clicked() {
            app.actions.push(Action::SaveEdit);
        }
    });
}

/// The profile beside a DM, as Discord shows it.
fn dm_profile(app: &mut App, ui: &mut egui::Ui, user: &crate::discord::model::User) {
    let p = theme::p();
    if !app.profiles.contains_key(&user.id) && app.requested_profiles.insert(user.id) {
        app.commands.send(crate::backend::Command::Profile { user: user.id, guild: None });
    }
    let profile = app.profiles.get(&user.id).cloned();
    let banner_color = profile
        .as_ref()
        .and_then(|pr| pr.theme_colors.map(|c| c.0).or(pr.accent))
        .and_then(theme::role_color)
        .unwrap_or(p.accent);
    let (banner, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    ui.painter().rect_filled(banner, 0.0, banner_color);
    if let Some(url) = profile.as_ref().and_then(|pr| pr.banner.clone())
        && let Some((texture, _)) = app.images.get(&url, 680)
    {
        egui::Image::from_texture((texture, banner.size())).paint_at(ui, banner);
    }
    let avatar = Rect::from_min_size(banner.left_bottom() + vec2(16.0, -44.0), Vec2::splat(80.0));
    ui.painter().circle_filled(avatar.center(), 46.0, p.sidebar);
    app.images.circle(ui, avatar, Some(&user.avatar_moving(128)), &initials(user.display_name()), 40.0);
    if let Some(presence) = app.presences.get(&user.id).filter(|pr| pr.status != "offline") {
        super::friends::dot(ui, avatar, &presence.status, p.sidebar);
    }
    ui.add_space(48.0);
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        Frame::new().inner_margin(Margin::symmetric(16, 0)).show(ui, |ui| {
            ui.label(RichText::new(user.display_name()).font(theme::strong(20.0)).color(p.strong));
            let mut handle = user.username.clone();
            if let Some(pronouns) = profile.as_ref().map(|pr| pr.pronouns.clone()).filter(|x| !x.is_empty()) {
                handle.push_str(&format!(" · {pronouns}"));
            }
            ui.label(RichText::new(handle).color(p.muted));
            if let Some(activity) = app.presences.get(&user.id).and_then(|pr| pr.activity()) {
                ui.add_space(4.0);
                ui.label(RichText::new(activity).size(13.0).color(p.text));
            }
            let activities = app.presences.get(&user.id).map(|pr| pr.activities.clone()).unwrap_or_default();
            for activity in &activities {
                ui.add_space(8.0);
                super::activity::card(app, ui, activity);
            }
            Frame::new().fill(p.rail).corner_radius(8).inner_margin(Margin::same(12)).outer_margin(Margin::symmetric(0, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(bio) = profile.as_ref().map(|pr| pr.bio.clone()).filter(|b| !b.is_empty()) {
                    ui.label(RichText::new(t!("About Me")).font(theme::strong(12.5)).color(p.strong));
                    let blocks = markdown::parse(&bio);
                    markdown::show(app, ui, &blocks, p.text, user.id);
                    ui.add_space(8.0);
                }
                ui.label(RichText::new(t!("Member Since")).font(theme::strong(12.5)).color(p.strong));
                ui.label(RichText::new(text::day(crate::discord::model::snowflake_ms(user.id))).size(13.0).color(p.text));
            });
            voice_card(app, ui, user.id);
            if let Some(profile) = &profile {
                let mutual: Vec<(Option<String>, String)> = profile
                    .mutual_guilds
                    .iter()
                    .filter_map(|g| app.guild(*g).map(|g| (g.icon_url(), g.name.clone())))
                    .collect();
                if !mutual.is_empty() {
                    ui.label(RichText::new(tn!(mutual.len(), "{count} Mutual Server", "{count} Mutual Servers")).font(theme::strong(13.0)).color(p.muted));
                    for (icon, name) in mutual.iter().take(8) {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                            app.images.circle(ui, rect, icon.as_deref(), &initials(name), 8.0);
                            ui.label(RichText::new(name).color(p.text));
                        });
                    }
                }
                if let Some(friends) = profile.mutual_friends.filter(|n| *n > 0) {
                    ui.add_space(6.0);
                    ui.label(RichText::new(tn!(friends, "{count} Mutual Friend", "{count} Mutual Friends")).font(theme::strong(13.0)).color(p.muted));
                }
            }
        });
    });
}

/// A voice channel's own view, as Discord's: a tile for each person, in
/// rows as wide as fit, the picture on its average colour and a green edge
/// while they speak. The name shows on hover.
fn voice_view(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    const GAP: f32 = 8.0;
    let p = theme::p();
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, p.chat);
    let inside = app.voice.channel.is_some_and(|(_, c)| c == channel.id);
    let members = app.voice_members(channel);
    let chat = Rect::from_min_size(egui::pos2(full.right() - 44.0, full.top() + 8.0), Vec2::splat(32.0));
    let response = ui.interact(chat, ui.id().with("voice-show-chat"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(chat, 8.0, p.hover);
    }
    crate::icons::paint(ui, Rect::from_center_size(chat.center(), Vec2::splat(20.0)), crate::icons::Icon::Chat, if response.hovered() { p.strong } else { p.muted });
    if response.on_hover_text(t!("Show Chat")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.voice_chat = true;
    }
    if !inside {
        let button = Rect::from_center_size(egui::pos2(full.center().x, if members.is_empty() { full.center().y + 24.0 } else { full.bottom() - 36.0 }), vec2(160.0, 40.0));
        let response = ui.interact(button, ui.id().with("voice-join"), Sense::click());
        ui.painter().rect_filled(button, 8.0, if response.hovered() { p.green.linear_multiply(0.85) } else { p.green });
        ui.painter().text(button.center(), Align2::CENTER_CENTER, t!("Join Voice"), theme::strong(14.0), Color32::WHITE);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.actions.push(Action::JoinVoice { guild: channel.guild_id, channel: channel.id, ring: false });
        }
    }
    if members.is_empty() {
        ui.painter().text(full.center() - vec2(0.0, 24.0), Align2::CENTER_CENTER, channel.title(), theme::strong(24.0), p.strong);
        return;
    }
    let area = Rect::from_min_max(full.min + vec2(GAP, 48.0), full.max - vec2(GAP, if inside { 16.0 } else { 72.0 }));
    // The column count that makes the tiles largest, each 16:9.
    let n = members.len();
    let (mut columns, mut width) = (1, 0.0f32);
    for c in 1..=n {
        let rows = n.div_ceil(c) as f32;
        let across = (area.width() - GAP * (c as f32 - 1.0)) / c as f32;
        let down = (area.height() - GAP * (rows - 1.0)) / rows * 16.0 / 9.0;
        if across.min(down) > width {
            (columns, width) = (c, across.min(down));
        }
    }
    let height = width * 9.0 / 16.0;
    let rows = n.div_ceil(columns);
    let mut y = area.center().y - (rows as f32 * height + (rows - 1) as f32 * GAP) / 2.0;
    for row in members.chunks(columns) {
        let mut x = area.center().x - (row.len() as f32 * width + (row.len() - 1) as f32 * GAP) / 2.0;
        for id in row {
            voice_tile(app, ui, channel, *id, Rect::from_min_size(egui::pos2(x, y), vec2(width, height)));
            x += width + GAP;
        }
        y += height + GAP;
    }
}

fn voice_tile(app: &mut App, ui: &mut egui::Ui, channel: &Channel, id: Id, rect: Rect) {
    let p = theme::p();
    let Some(user) = app.users.get(&id).cloned() else {
        app.images.circle(ui, Rect::from_center_size(rect.center(), Vec2::splat(80.0)), None, "", 40.0);
        if let Some(guild) = channel.guild_id {
            app.request_member(guild, id);
        }
        return;
    };
    let response = ui.interact(rect, ui.id().with(("voice-tile", id)), Sense::click());
    let url = user.avatar_url().replace("size=64", "size=128");
    let side = 80.0f32.min(rect.height() * 0.5);
    let avatar = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let fill = app.images.average(&url).unwrap_or(p.sidebar);
    ui.painter().rect_filled(rect, 8.0, fill);
    app.images.circle(ui, avatar, Some(&url), &initials(user.display_name()), side / 2.0);
    if let Some(decoration) = user.decoration_url() {
        crate::icons::decoration(ui, &mut app.images, avatar, &decoration);
    }
    if app.voice.speaking.contains(&id) {
        ui.painter().rect_stroke(rect, 8.0, Stroke::new(2.0, p.green), egui::StrokeKind::Inside);
    }
    // Muted or deafened, in the corner.
    let (muted, deaf) = app.voice_flags.get(&id).copied().unwrap_or_default();
    if muted || deaf {
        let badge = Rect::from_min_size(rect.right_bottom() - vec2(36.0, 36.0), Vec2::splat(28.0));
        ui.painter().circle_filled(badge.center(), 14.0, Color32::from_black_alpha(180));
        crate::icons::paint(ui, badge.shrink(5.0), if deaf { crate::icons::Icon::HeadphonesSlash } else { crate::icons::Icon::MicrophoneSlash }, Color32::WHITE);
    }
    if response.hovered() {
        let (name, _) = app.display(channel.guild_id, &user);
        let galley = ui.painter().layout_no_wrap(name, theme::strong(14.0), Color32::WHITE);
        let pill = Rect::from_min_size(rect.left_bottom() + vec2(8.0, -36.0), galley.size() + vec2(16.0, 8.0));
        ui.painter().rect_filled(pill, 6.0, Color32::from_black_alpha(180));
        ui.painter().galley(pill.min + vec2(8.0, 4.0), galley, Color32::WHITE);
    }
    let at = rect.right_top() + vec2(8.0, 0.0);
    if response.clicked() {
        app.actions.push(Action::OpenProfile { user: user.clone(), guild: channel.guild_id, at });
    }
    response.context_menu(|ui| {
        super::menu::begin(ui);
        super::menu::user(app, ui, &user, channel.guild_id);
    });
}

/// Discord's "In voice" card on a profile: up to four of the people there,
/// the channel and its server, and the button to join when we may connect.
fn voice_card(app: &mut App, ui: &mut egui::Ui, user: Id) {
    /// Discord's CONNECT permission.
    const CONNECT: u64 = 1 << 20;
    let p = theme::p();
    let Some(channel) = app.user_voice_channel(user).cloned() else { return };
    let Some(guild_id) = channel.guild_id else { return };
    let (guild_name, can_connect) = match app.guild(guild_id) {
        Some(guild) => (guild.name.clone(), guild.channel_permissions(app.me_id(), &channel) & CONNECT != 0),
        None => return,
    };
    let inside = app.voice.channel.is_some_and(|(_, c)| c == channel.id);
    let people: Vec<crate::discord::model::User> = app.voice_members(&channel).into_iter().filter_map(|id| app.users.get(&id).cloned()).take(4).collect();
    Frame::new().fill(p.rail).corner_radius(8).inner_margin(Margin::same(12)).outer_margin(Margin::symmetric(0, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(t!("In voice")).font(theme::strong(12.5)).color(p.strong));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            // The people, two by two.
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
            for (i, person) in people.iter().enumerate() {
                let at = tile.min + vec2((i % 2) as f32 * 28.0, (i / 2) as f32 * 28.0);
                app.images.circle(ui, Rect::from_min_size(at, Vec2::splat(24.0)), Some(&person.avatar_url()), &initials(person.display_name()), 12.0);
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    crate::icons::paint(ui, icon, crate::icons::Icon::Voice, p.muted);
                    ui.add(Label::new(RichText::new(&channel.name).font(theme::strong(14.0)).color(p.strong)).truncate());
                });
                ui.add(Label::new(RichText::new(t!("in {name}", name = guild_name)).size(12.5).color(p.muted)).truncate());
            });
        });
        if can_connect || inside {
            ui.add_space(10.0);
            let label = if inside { t!("Open Voice") } else { t!("Join Voice") };
            if super::form::button_wide(ui, label).clicked() {
                app.actions.push(if inside { Action::SelectChannel(channel.id) } else { Action::JoinVoice { guild: Some(guild_id), channel: channel.id, ring: false } });
            }
        }
    });
}

/// What a member is doing, as Discord's member list writes it: "Playing "
/// and the game in bold wherever the language puts `{name}` (with its
/// application, for the icon), or the custom status with its emoji.
fn member_activity(presence: &crate::discord::gateway::Presence) -> Option<(String, String, Option<Id>)> {
    if let Some(activity) = presence.activities.first() {
        let template = match activity.kind {
            1 => t!("Streaming {name}"),
            2 => t!("Listening to {name}"),
            3 => t!("Watching {name}"),
            5 => t!("Competing in {name}"),
            _ => t!("Playing {name}"),
        };
        return Some((template.to_owned(), activity.name.clone(), activity.application));
    }
    presence.custom.clone().map(|custom| (custom, String::new(), None))
}
