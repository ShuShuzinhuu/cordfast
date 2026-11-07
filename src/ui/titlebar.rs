//! Discord's title bar: 32 points across the top of the window, in the
//! rail's colour. On the left the arrows back and forward through where you
//! have been; in the middle where you are (the server, the conversation or
//! Amigos); on the right the inbox and help and, since the window has no
//! system frame, minimize, maximize and close. Dragging it moves the window
//! and a double click maximizes it (fastframe-titlebar). Sizes and colours
//! measured on the owner's screenshot of Discord.

use egui::{Color32, FontId, Frame, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Shape, Stroke, Vec2, pos2, vec2};

use super::{initials, markdown};
use crate::app::{Action, App, AuthView, InboxTab};
use crate::discord::model::{ChannelKind, Id};
use crate::icons::{self, Icon};
use crate::{text, theme};

pub const HEIGHT: f32 = 32.0;
/// A window button: Discord's are as tall as the bar and 36 apart.
const BUTTON: f32 = 36.0;
/// Where Discord keeps the inbox and help from the window buttons.
const HELP_GAP: f32 = 28.5;
const ICON_STEP: f32 = 36.0;
const INBOX_WIDTH: f32 = 480.0;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    egui::Panel::top("titlebar")
        .exact_size(HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(p.rail))
        .show(ui, |ui| bar(app, ui));
}

fn bar(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let rect = ui.max_rect();
    // Before the bar's own buttons, so they keep their clicks.
    fastframe_titlebar::drag(ui, rect);
    let logged_in = matches!(app.auth, AuthView::LoggedIn);
    if logged_in {
        for (back, x) in [(true, 19.5), (false, 43.5)] {
            let center = pos2(rect.left() + x, rect.center().y);
            let enabled = if back { !app.back.is_empty() } else { !app.forward.is_empty() };
            let sense = if enabled { Sense::click() } else { Sense::hover() };
            let response = ui.interact(Rect::from_center_size(center, Vec2::splat(24.0)), ui.id().with(("history", back)), sense);
            // Discord leaves the arrows there, barely visible, when there
            // is nowhere to go.
            let color = match (enabled, response.hovered()) {
                (false, _) => p.faint.gamma_multiply(0.45),
                (true, true) => p.strong,
                (true, false) => p.muted,
            };
            arrow(ui, center, back, color);
            if enabled && response.on_hover_text(if back { tp!("navigation", "Back") } else { tp!("navigation", "Forward") }).clicked() {
                app.actions.push(if back { Action::Back } else { Action::Forward });
            }
        }
        location(app, ui, rect);
    }
    let mut right = rect.right();
    if fastframe_titlebar::visible(ui.ctx()) {
        right = window_buttons(ui, rect);
        ui.painter().vline(right - 7.5, egui::Rangef::new(rect.top() + 6.0, rect.bottom() - 6.0), Stroke::new(1.0, p.border));
        right -= 7.5;
    }
    if !logged_in {
        return;
    }
    // Help, then the inbox to its left.
    let help = Rect::from_center_size(pos2(right - HELP_GAP, rect.center().y), Vec2::splat(28.0));
    let response = ui.interact(help, ui.id().with("help"), Sense::click());
    icons::paint(ui, Rect::from_center_size(help.center(), Vec2::splat(16.0)), Icon::Help, if response.hovered() { p.strong } else { p.muted });
    if response.on_hover_text(t!("Help")).clicked() {
        let _ = open::that_detached(crate::i18n::current().help_url());
    }
    let button = help.translate(vec2(-ICON_STEP, 0.0));
    let response = ui.interact(button, ui.id().with("inbox"), Sense::click());
    let open = app.inbox.is_some();
    icons::paint(ui, Rect::from_center_size(button.center(), Vec2::splat(16.0)), Icon::Inbox, if open || response.hovered() { p.strong } else { p.muted });
    if response.on_hover_text(t!("Inbox")).clicked() {
        app.toggle_inbox();
    }
    ui.data_mut(|d| d.insert_temp(egui::Id::new("inbox-button"), button));
}

/// Discord's thin arrow: a 13-point shaft and a small head.
fn arrow(ui: &egui::Ui, center: Pos2, back: bool, color: Color32) {
    let d = if back { -1.0 } else { 1.0 };
    let tip = center + vec2(6.5 * d, 0.0);
    let stroke = Stroke::new(1.8, color);
    ui.painter().line_segment([center - vec2(6.5 * d, 0.0), tip], stroke);
    ui.painter().add(Shape::line(vec![tip + vec2(-4.5 * d, -4.5), tip, tip + vec2(-4.5 * d, 4.5)], stroke));
}

/// Where you are, centred on the window: the server's icon and name, the
/// person's avatar and name in a DM, or Amigos.
fn location(app: &mut App, ui: &mut egui::Ui, bar: Rect) {
    let p = theme::p();
    enum Lead {
        Icon(Icon),
        Image(Option<String>, String, f32),
    }
    let (lead, name) = match (app.selected_guild, app.selected_channel) {
        (Some(guild), _) => match app.guild(guild) {
            Some(g) => (Lead::Image(g.icon_url(), initials(&g.name), 5.0), g.name.clone()),
            None => return,
        },
        (None, Some(channel)) => match app.channel(channel) {
            Some(c) => {
                let avatar = c.recipients.first().map(|u| (u.avatar_url(), initials(u.display_name())));
                let lead = match avatar {
                    Some((url, letters)) if c.kind == ChannelKind::Dm || c.kind == ChannelKind::GroupDm => Lead::Image(Some(url), letters, 8.0),
                    _ => Lead::Icon(Icon::Friends),
                };
                (lead, c.title())
            }
            None => return,
        },
        (None, None) => match app.home {
            crate::app::Home::Friends => (Lead::Icon(Icon::Friends), t!("Friends").to_owned()),
            crate::app::Home::MessageRequests => (Lead::Icon(Icon::Envelope), t!("Message Requests").to_owned()),
        },
    };
    let font = FontId::proportional(14.0);
    let room = (bar.width() * 0.4).max(120.0);
    let mut job = text::script_job(&name, font, p.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let width = 16.0 + 9.0 + galley.size().x;
    let left = bar.center().x - width / 2.0;
    let icon = Rect::from_min_size(pos2(left, bar.center().y - 8.0), Vec2::splat(16.0));
    match lead {
        Lead::Icon(kind) => icons::paint(ui, icon, kind, p.muted),
        Lead::Image(url, letters, rounding) => app.images.circle(ui, icon, url.as_deref(), &letters, rounding),
    }
    ui.painter().galley(pos2(icon.right() + 9.0, bar.center().y - galley.size().y / 2.0), galley, p.text);
}

/// Minimize, maximize (restore when maximized) and close, drawn as Discord
/// draws them: thin 10-point glyphs, a wash on hover and red under close.
/// Returns where they start.
fn window_buttons(ui: &mut egui::Ui, bar: Rect) -> f32 {
    let p = theme::p();
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let mut right = bar.right() + 2.0;
    for which in 0..3 {
        let rect = Rect::from_min_max(pos2(right - BUTTON, bar.top()), pos2(right, bar.bottom())).intersect(bar);
        right -= BUTTON;
        let response = ui.interact(rect, ui.id().with(("window", which)), Sense::click());
        let close = which == 0;
        let color = if response.hovered() {
            ui.painter().rect_filled(rect, 0.0, if close { p.red } else { p.hover });
            if close { Color32::WHITE } else { p.strong }
        } else {
            p.muted
        };
        let c = pos2(rect.center().x.round() + 0.5, bar.center().y.round() - 0.5);
        let stroke = Stroke::new(1.0, color);
        let painter = ui.painter();
        match which {
            0 => {
                painter.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], stroke);
                painter.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], stroke);
            }
            1 if maximized => {
                // Two windows, one behind the other.
                painter.rect_stroke(Rect::from_min_size(c + vec2(-5.0, -3.0), Vec2::splat(8.0)), 0.0, stroke, egui::StrokeKind::Middle);
                painter.add(Shape::line(vec![c + vec2(-3.0, -3.0), c + vec2(-3.0, -5.0), c + vec2(5.0, -5.0), c + vec2(5.0, 3.0), c + vec2(3.0, 3.0)], stroke));
            }
            1 => {
                painter.rect_stroke(Rect::from_center_size(c, Vec2::splat(10.0)), 0.0, stroke, egui::StrokeKind::Middle);
            }
            _ => {
                painter.line_segment([c + vec2(-5.0, 0.5), c + vec2(5.0, 0.5)], stroke);
            }
        }
        if response.clicked() {
            let command = match which {
                0 => egui::ViewportCommand::Close,
                1 => egui::ViewportCommand::Maximized(!maximized),
                _ => egui::ViewportCommand::Minimized(true),
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
    right
}

/// Discord's inbox, under its button: "Unreads", the channels with
/// unread messages (a check marks one read), and "Mentions", recent
/// mentions everywhere. A click outside or Esc closes it.
pub fn inbox(app: &mut App, ctx: &egui::Context) {
    let Some(mut inbox) = app.inbox.take() else { return };
    let p = theme::p();
    let screen = ctx.content_rect();
    let button = ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("inbox-button"))).unwrap_or(Rect::from_min_size(pos2(screen.right() - 200.0, screen.top()), Vec2::splat(28.0)));
    let left = (button.right() + 12.0 - INBOX_WIDTH).clamp(screen.left() + 8.0, (screen.right() - INBOX_WIDTH - 8.0).max(screen.left() + 8.0));
    let height = (screen.height() * 0.8).min(640.0);
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if inbox.tab != InboxTab::Unread && inbox.mentions.is_none() && !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("inbox-asked"))).unwrap_or(false) {
        app.commands.send(match inbox.tab {
            InboxTab::Bookmarks => crate::admin::get("inbox-saved", "/users/@me/saved-messages/bookmarks?limit=25".into()),
            InboxTab::Reminders => crate::admin::get("inbox-saved", "/users/@me/saved-messages".into()),
            _ => crate::admin::get("inbox", "/users/@me/mentions?limit=25&roles=true&everyone=true".into()),
        });
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("inbox-asked"), true));
    }
    let area = egui::Area::new(egui::Id::new("inbox"))
        .fixed_pos(pos2(left, button.bottom() + 6.0))
        .order(egui::Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(p.sidebar)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(8)
                .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(INBOX_WIDTH);
                    ui.set_height(height);
                    Frame::new().inner_margin(Margin { left: 16, right: 16, top: 14, bottom: 10 }).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (icon, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                            icons::paint(ui, icon, Icon::Inbox, p.strong);
                            ui.label(RichText::new(t!("Inbox")).font(theme::strong(18.0)).color(p.strong));
                        });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for (tab, label) in [(InboxTab::Mentions, t!("Mentions")), (InboxTab::Unread, t!("Unreads")), (InboxTab::Bookmarks, t!("Bookmarks")), (InboxTab::Reminders, t!("Reminders"))] {
                                if tab_pill(ui, label, inbox.tab == tab).clicked() && inbox.tab != tab {
                                    inbox.tab = tab;
                                    if tab != InboxTab::Unread {
                                        inbox.mentions = None;
                                        ui.data_mut(|d| d.insert_temp(egui::Id::new("inbox-asked"), false));
                                    }
                                }
                            }
                        });
                    });
                    let line = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().x_range(), line, Stroke::new(1.0, p.border));
                    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                        Frame::new().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match inbox.tab {
                                InboxTab::Unread => {
                                    if unread(app, ui) {
                                        close = true;
                                    }
                                }
                                InboxTab::Mentions => {
                                    if mentions(app, ui, inbox.mentions.as_deref(), (t!("No recent mentions"), t!("When someone mentions you, it shows up here."))) {
                                        close = true;
                                    }
                                }
                                InboxTab::Bookmarks => {
                                    if mentions(app, ui, inbox.mentions.as_deref(), (t!("You have no Bookmarks!"), "")) {
                                        close = true;
                                    }
                                }
                                InboxTab::Reminders => {
                                    if mentions(app, ui, inbox.mentions.as_deref(), (t!("You have no Reminders!"), "")) {
                                        close = true;
                                    }
                                }
                            }
                        });
                    });
                });
        });
    if ctx.input(|i| i.pointer.any_pressed())
        && let Some(pointer) = ctx.pointer_interact_pos()
        && !area.response.rect.contains(pointer)
        && !button.contains(pointer)
    {
        close = true;
    }
    if !close && app.inbox.is_none() {
        app.inbox = Some(inbox);
    } else if close {
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::new("inbox-asked")));
    }
}

/// One of the inbox's tabs: a pill, filled while chosen.
fn tab_pill(ui: &mut egui::Ui, label: &str, chosen: bool) -> egui::Response {
    let p = theme::p();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::strong(14.0), p.text);
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 28.0), Sense::click());
    if chosen || response.hovered() {
        ui.painter().rect_filled(rect, 6.0, if chosen { p.selected } else { p.hover });
    }
    let color = if chosen || response.hovered() { p.strong } else { p.muted };
    ui.painter().galley(pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, color);
    response
}

/// The channels with unread messages, newest first: the server's icon (or
/// the person's avatar), the channel, the server, the last unread message
/// when it is loaded, and the check that marks it read. True when one was
/// opened.
fn unread(app: &mut App, ui: &mut egui::Ui) -> bool {
    let p = theme::p();
    let me = app.me_id();
    let mut list: Vec<(Id, Option<Id>, Id)> = Vec::new();
    for guild in &app.guilds {
        if app.mutes.guilds.contains(&guild.id) {
            continue;
        }
        for channel in &guild.channels {
            if channel.kind.is_text() && guild.can_view(me, channel) && !app.is_muted(channel) && app.is_unread(channel.id) {
                list.push((channel.id, Some(guild.id), app.latest.get(&channel.id).copied().unwrap_or(0)));
            }
        }
    }
    for dm in &app.dms {
        if app.is_unread(dm.id) && !app.is_muted(dm) {
            list.push((dm.id, None, app.latest.get(&dm.id).copied().unwrap_or(0)));
        }
    }
    list.sort_by_key(|(_, _, latest)| std::cmp::Reverse(*latest));
    list.truncate(40);
    if list.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("You're all caught up!")).font(theme::strong(16.0)).color(p.strong));
            ui.label(RichText::new(t!("You have no unread messages.")).size(13.5).color(p.muted));
        });
        return false;
    }
    let mut opened = false;
    for (channel_id, guild_id, _) in list {
        let Some(channel) = app.channel(channel_id).cloned() else { continue };
        let guild = guild_id.and_then(|g| app.guild(g)).map(|g| (g.icon_url(), g.name.clone()));
        let preview = app.histories.get(&channel_id).and_then(|h| h.messages.last()).map(|m| (m.author.display_name().to_owned(), m.content.clone()));
        let row_height = if preview.is_some() { 64.0 } else { 46.0 };
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_height), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 6.0, p.hover);
        }
        let icon = Rect::from_min_size(rect.left_top() + vec2(6.0, 7.0), Vec2::splat(32.0));
        match &guild {
            Some((url, name)) => app.images.circle(ui, icon, url.as_deref(), &initials(name), 10.0),
            None => {
                let user = channel.recipients.first();
                app.images.circle(ui, icon, user.map(|u| u.avatar_url()).as_deref(), &initials(&channel.title()), 16.0);
            }
        }
        let x = icon.right() + 10.0;
        let title = if guild.is_some() { format!("# {}", channel.title()) } else { channel.title() };
        super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.top() + 5.0), pos2(rect.right() - 44.0, rect.top() + 23.0)), &title, theme::strong(14.5), p.strong);
        if let Some((_, name)) = &guild {
            super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.top() + 23.0), pos2(rect.right() - 44.0, rect.top() + 40.0)), name, FontId::proportional(12.5), p.muted);
        }
        if let Some((author, content)) = &preview {
            let line = format!("{author}: {}", content.lines().next().unwrap_or_default());
            super::paint_truncated(ui, Rect::from_min_max(pos2(x, rect.top() + 40.0), pos2(rect.right() - 44.0, rect.top() + 58.0)), &line, FontId::proportional(13.0), p.text);
        }
        // Discord's round check: marks the channel read.
        let check = Rect::from_center_size(pos2(rect.right() - 22.0, rect.top() + 22.0), Vec2::splat(28.0));
        let check_response = ui.interact(check, ui.id().with(("inbox-read", channel_id)), Sense::click());
        ui.painter().circle_filled(check.center(), 14.0, if check_response.hovered() { p.selected } else { p.rail });
        icons::paint(ui, Rect::from_center_size(check.center(), Vec2::splat(18.0)), Icon::Check, if check_response.hovered() { p.strong } else { p.muted });
        if check_response.on_hover_text(t!("Mark as Read")).clicked() {
            app.actions.push(Action::MarkRead(vec![channel_id]));
        } else if response.clicked() {
            app.actions.push(Action::SelectGuild(guild_id));
            app.actions.push(Action::SelectChannel(channel_id));
            opened = true;
        }
    }
    opened
}

/// Recent mentions: where each was (channel and server), who and when, and
/// the message; a click goes to it. True when one was opened.
fn mentions(app: &mut App, ui: &mut egui::Ui, list: Option<&[crate::discord::model::Message]>, empty: (&str, &str)) -> bool {
    let p = theme::p();
    let Some(list) = list else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.spinner());
        return false;
    };
    if list.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(empty.0).font(theme::strong(16.0)).color(p.strong));
            ui.label(RichText::new(empty.1).size(13.5).color(p.muted));
        });
        return false;
    }
    let mut opened = false;
    for message in list {
        let guild = message.guild_id.or_else(|| app.channel(message.channel_id).and_then(|c| c.guild_id));
        let place = match (app.channel(message.channel_id), guild.and_then(|g| app.guild(g))) {
            (Some(c), Some(g)) => format!("# {} • {}", c.title(), g.name),
            (Some(c), None) => c.title(),
            _ => String::new(),
        };
        let response = Frame::new()
            .fill(p.chat)
            .corner_radius(8)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !place.is_empty() {
                    ui.label(RichText::new(&place).font(theme::strong(12.5)).color(p.muted));
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    let (avatar, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                    app.images.circle(ui, avatar, Some(&message.author.avatar_url()), &initials(message.author.display_name()), 12.0);
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
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.actions.push(Action::SelectGuild(guild));
            app.actions.push(Action::SelectChannel(message.channel_id));
            app.actions.push(Action::JumpTo { channel: message.channel_id, message: message.id });
            opened = true;
        }
        ui.add_space(8.0);
    }
    opened
}
