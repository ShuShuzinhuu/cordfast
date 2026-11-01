//! Slash commands in the composer, as Discord's: typing "/" lists the
//! commands of the place's apps and Discord's built-in ones; a command
//! picked turns the box into its options, each a field after its name,
//! with the choices, people, roles, channels or the app's own suggestions
//! over it; Enter runs it.

use egui::{FontId, Frame, Key, Margin, Modifiers, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};
use serde_json::Value;

use crate::app::{Action, App, SlashDraft, SlashField};
use crate::discord::commands::{BUILT_IN, CommandOption, Index, built_in_text, typed_value};
use crate::discord::model::{Channel, Id};
use crate::icons::{self, Icon};
use crate::theme;

/// What is typed after "/" while a command is being chosen: "pl" in
/// "/pl", "queue s" in "/queue s" (subcommands follow their command's
/// name); none with no slash, a space right after it or a new line.
fn query(app: &App) -> Option<&str> {
    let query = app.composer.strip_prefix('/')?;
    (!query.starts_with(char::is_whitespace) && !query.contains('\n')).then_some(query)
}

/// Where a channel's commands are kept: its server, or the conversation.
fn key(channel: &Channel) -> Id {
    channel.guild_id.unwrap_or(channel.id)
}

/// Runs `f` with a place's loaded commands, lent out of the app for the
/// time (so the app stays free to draw): no copy each frame.
fn with_index<R>(app: &mut App, key: Id, f: impl FnOnce(&mut App, &Index) -> R) -> Option<R> {
    let index = app.command_index.get_mut(&key)?.take()?;
    let out = f(app, &index);
    app.command_index.insert(key, Some(index));
    Some(out)
}

/// The picker over the composer while "/…" is typed. Returns whether it
/// took Enter, which then picks instead of sending.
pub fn picker(app: &mut App, ui: &mut egui::Ui, channel: &Channel, focused: bool) -> bool {
    if app.slash.is_some() {
        return false;
    }
    let Some(query) = query(app).map(str::to_owned) else {
        app.slash_pick = 0;
        return false;
    };
    if app.command_index(channel).is_none() {
        // Loading: Discord shows the panel with its spinner meanwhile.
        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(t!("Loading…")).size(13.0).color(theme::p().muted));
            });
        });
        return false;
    }
    with_index(app, key(channel), |app, index| list(app, ui, channel, index, &query, focused)).unwrap_or(false)
}

fn list(app: &mut App, ui: &mut egui::Ui, channel: &Channel, index: &Index, query: &str, focused: bool) -> bool {
    // Spaces count once, as in the names ("queue  show" is "queue show").
    let typed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let spaced = query.ends_with(char::is_whitespace);
    // A space after a whole name picks that command, as Discord does:
    // "/skip " opens it, "/queue " goes on listing its subcommands.
    if spaced && let [entry] = index.exact(&typed)[..] {
        start(app, channel, index, entry);
        return false;
    }
    let found = index.search(&if spaced { format!("{typed} ") } else { typed });
    if found.is_empty() {
        return false;
    }
    app.slash_pick = app.slash_pick.min(found.len() - 1);
    let mut picked = None;
    let mut took = false;
    if focused {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                app.slash_pick = (app.slash_pick + 1) % found.len();
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                app.slash_pick = (app.slash_pick + found.len() - 1) % found.len();
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter) {
                picked = Some(found[app.slash_pick]);
                took = true;
            }
        });
    }
    let p = theme::p();
    panel(ui, |ui| {
        ui.label(RichText::new(t!("Commands Matching /{query}", query = query).to_uppercase()).font(theme::strong(12.0)).color(p.muted));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            for (row, &entry_index) in found.iter().enumerate() {
                let entry = &index.entries[entry_index];
                let command = &index.commands[entry.command];
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
                let chosen = row == app.slash_pick;
                if chosen || response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, if chosen { p.selected } else { p.hover });
                }
                if chosen && focused {
                    response.scroll_to_me(None);
                }
                let icon = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), Vec2::splat(28.0));
                app_icon(app, ui, index, command.application, icon);
                // "/play query volume", the description under it, the
                // app's name at the end.
                let app_name = if command.application == BUILT_IN { t!("Built-In").to_owned() } else { index.app(command.application).map(|a| a.name.clone()).unwrap_or_default() };
                let right = ui.painter().layout_no_wrap(app_name, FontId::proportional(12.0), p.muted);
                let text_width = (rect.width() - 52.0 - right.size().x - 16.0).max(40.0);
                let mut job = egui::text::LayoutJob::default();
                job.append(&index.title(entry), 0.0, egui::TextFormat::simple(theme::strong(15.0), p.strong));
                for option in &entry.options {
                    job.append(&format!("  {}", option.name), 0.0, egui::TextFormat::simple(FontId::proportional(13.0), if option.required { p.text } else { p.faint }));
                }
                job.wrap = egui::text::TextWrapping::truncate_at_width(text_width);
                let title = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(pos2(rect.left() + 44.0, rect.top() + 6.0), title, p.strong);
                super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 44.0, rect.top() + 26.0), pos2(rect.left() + 44.0 + text_width, rect.bottom() - 4.0)), &entry.description, FontId::proportional(13.0), p.muted);
                ui.painter().galley(pos2(rect.right() - right.size().x - 10.0, rect.center().y - right.size().y / 2.0), right, p.muted);
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(entry_index);
                }
            }
        });
    });
    if let Some(entry) = picked {
        start(app, channel, index, entry);
    }
    took
}

/// Discord's panel over the composer.
fn panel(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let p = theme::p();
    Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        content(ui);
    });
    ui.add_space(4.0);
}

/// An application's icon in a list: its picture, or Discord's mark for the
/// built-in commands.
fn app_icon(app: &mut App, ui: &egui::Ui, index: &Index, application: Id, rect: Rect) {
    let p = theme::p();
    let picture = index.app(application).and_then(|a| a.icon.as_deref());
    match picture.and_then(|url| app.images.get(url, 64)) {
        Some((texture, _)) => {
            egui::Image::from_texture((texture, rect.size())).corner_radius(rect.width() / 2.0).paint_at(ui, rect);
        }
        None => {
            ui.painter().circle_filled(rect.center(), rect.width() / 2.0, if application == BUILT_IN { p.accent } else { p.selected });
            icons::paint(ui, rect.shrink(rect.width() * 0.22), Icon::Discord, egui::Color32::WHITE);
        }
    }
}

/// Picks a command: the box becomes its options, required ones first.
fn start(app: &mut App, channel: &Channel, index: &Index, entry: usize) {
    let mut options = index.entries[entry].options.clone();
    options.sort_by_key(|o| !o.required);
    app.slash = Some(SlashDraft {
        channel: channel.id,
        key: key(channel),
        entry,
        fields: options.into_iter().map(|option| SlashField { option, text: String::new(), value: None }).collect(),
        focus: 0,
        refocus: true,
        suggestions: Vec::new(),
        asked: None,
        typed_at: None,
    });
    app.composer.clear();
    app.slash_pick = 0;
}

/// Leaves the command, its name back in the box to pick another.
fn cancel(app: &mut App) {
    let Some(draft) = app.slash.take() else { return };
    let title = app.command_index.get(&draft.key).and_then(Option::as_ref).and_then(|index| Some(index.title(index.entries.get(draft.entry)?)));
    app.composer = title.unwrap_or_default();
    app.focus_composer = true;
}

/// The command in the box: its name, then each option's name and field.
/// Returns whether Enter asked to run it.
pub fn fields(app: &mut App, ui: &mut egui::Ui, channel: &Channel, width: f32, height: f32) -> bool {
    let Some(key) = app.slash.as_ref().map(|d| d.key) else { return false };
    match with_index(app, key, |app, index| draw_fields(app, ui, channel, index, width, height)) {
        Some(Outcome::Run) => true,
        Some(Outcome::Stay) => false,
        // Left once the index is back, so the command's name returns too.
        Some(Outcome::Leave) => {
            cancel(app);
            false
        }
        // The place's commands are gone (logged out, another account).
        None => {
            app.slash = None;
            false
        }
    }
}

/// What a frame of the fields asked for.
enum Outcome {
    Stay,
    Run,
    Leave,
}

fn draw_fields(app: &mut App, ui: &mut egui::Ui, channel: &Channel, index: &Index, width: f32, height: f32) -> Outcome {
    let p = theme::p();
    let Some(draft) = app.slash.as_mut() else { return Outcome::Stay };
    let Some(entry) = index.entries.get(draft.entry) else {
        app.slash = None;
        return Outcome::Stay;
    };
    let mut run = false;
    let mut leave = false;
    let refocus = std::mem::take(&mut draft.refocus);
    let focus_before = draft.focus;
    ui.allocate_ui_with_layout(vec2(width, height), egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
        ui.set_min_size(vec2(width, height));
        // The command's name, as Discord's chip.
        let galley = ui.painter().layout_no_wrap(index.title(entry), theme::strong(15.0), p.strong);
        let (chip, _) = ui.allocate_exact_size(galley.size() + vec2(12.0, 8.0), Sense::hover());
        ui.painter().rect_filled(chip, 4.0, p.selected);
        ui.painter().galley(chip.center() - galley.size() / 2.0, galley, p.strong);
        for (i, field) in draft.fields.iter_mut().enumerate() {
            let focused_field = i == draft.focus;
            let label = ui.painter().layout_no_wrap(field.option.name.clone(), FontId::proportional(13.0), if field.option.required { p.text } else { p.muted });
            let (label_rect, _) = ui.allocate_exact_size(label.size() + vec2(10.0, 6.0), Sense::hover());
            ui.painter().rect(label_rect, 4.0, if focused_field { p.hover } else { p.rail }, Stroke::new(1.0, if focused_field { p.accent } else { p.border }), egui::StrokeKind::Inside);
            ui.painter().galley(label_rect.center() - label.size() / 2.0, label, p.text);
            let id = egui::Id::new(("slash-field", i));
            let shown = ui.painter().layout_no_wrap(field.text.clone(), FontId::proportional(15.0), p.text).size().x;
            // The option's description stands in an empty field.
            let hint = if field.text.is_empty() { field.option.description.clone() } else { String::new() };
            let hint_width = ui.painter().layout_no_wrap(hint.clone(), FontId::proportional(15.0), p.faint).size().x;
            let response = ui.add(
                egui::TextEdit::singleline(&mut field.text)
                    .id(id)
                    .frame(Frame::NONE)
                    .font(FontId::proportional(15.0))
                    .text_color(p.text)
                    .desired_width(shown.max(hint_width.min(220.0)).max(40.0) + 8.0)
                    .hint_text(RichText::new(hint).color(p.faint)),
            );
            if response.changed() {
                // Typed over a pick: the pick no longer holds.
                field.value = None;
                draft.typed_at = Some(std::time::Instant::now());
                draft.suggestions.clear();
            }
            if response.has_focus() {
                draft.focus = i;
            }
            if refocus && i == draft.focus {
                response.request_focus();
                // The cursor after what was picked or typed.
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    let end = egui::text::CCursor::new(field.text.chars().count());
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ui.ctx(), id);
                }
            }
        }
    });
    if draft.focus != focus_before {
        draft.suggestions.clear();
        draft.asked = None;
    }
    let any_focused = (0..draft.fields.len()).any(|i| ui.ctx().memory(|m| m.has_focus(egui::Id::new(("slash-field", i)))));
    if any_focused {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::SHIFT, Key::Tab) {
                draft.focus = draft.focus.saturating_sub(1);
                draft.refocus = true;
            } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                draft.focus = (draft.focus + 1).min(draft.fields.len().saturating_sub(1));
                draft.refocus = true;
            }
            // Backspace in the first, empty field drops the command.
            if draft.focus == 0 && draft.fields.first().is_none_or(|f| f.text.is_empty()) && i.consume_key(Modifiers::NONE, Key::Backspace) {
                leave = true;
            }
        });
    }
    // Esc drops the command and Enter runs it, from its fields or, for a
    // command without options (no field to focus), while nothing else has
    // the keyboard. There Backspace drops it too, and typing turns it
    // back into text, so the box never holds the keys.
    let free = any_focused || ui.ctx().memory(|m| m.focused().is_none());
    let mut typed = String::new();
    if free {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                leave = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                run = true;
            }
            if draft.fields.is_empty() {
                if i.consume_key(Modifiers::NONE, Key::Backspace) {
                    leave = true;
                }
                for event in &i.events {
                    if let egui::Event::Text(text) | egui::Event::Paste(text) = event {
                        typed.push_str(text);
                    }
                }
            }
        });
    }
    if !typed.is_empty() {
        let text = format!("{} {typed}", index.title(entry));
        app.slash = None;
        app.composer = text;
        app.focus_composer = true;
        return Outcome::Stay;
    }
    if leave {
        return Outcome::Leave;
    }
    ask_suggestions(app, ui.ctx(), channel, index);
    if run { Outcome::Run } else { Outcome::Stay }
}

/// What the focused field could be: its choices, the place's people, roles
/// or channels, or what the app suggested; filtered by what was typed.
fn candidates(app: &App, channel: &Channel) -> Vec<(String, Value)> {
    let Some(draft) = &app.slash else { return Vec::new() };
    let Some(field) = draft.fields.get(draft.focus) else { return Vec::new() };
    let typed = field.text.to_lowercase();
    let matches = |name: &str| typed.is_empty() || name.to_lowercase().contains(&typed);
    let mut out: Vec<(String, Value)> = Vec::new();
    if !field.option.choices.is_empty() {
        out.extend(field.option.choices.iter().filter(|(name, _)| matches(name)).cloned());
    } else if field.option.kind == 5 {
        out.extend([(t!("True"), true), (t!("False"), false)].into_iter().filter(|(name, _)| matches(name)).map(|(name, value)| (name.to_owned(), Value::Bool(value))));
    } else if field.option.autocomplete {
        out.extend(draft.suggestions.iter().cloned());
    } else if field.option.picks_entity() {
        let guild = channel.guild_id.and_then(|g| app.guild(g));
        if matches!(field.option.kind, 6 | 9) {
            let mut seen = std::collections::HashSet::new();
            let recent = app.histories.get(&channel.id).into_iter().flat_map(|h| h.messages.iter().rev().map(|m| &m.author));
            let members = channel.guild_id.into_iter().flat_map(|g| app.members.keys().filter(move |(mg, _)| *mg == g).filter_map(|(_, u)| app.users.get(u)));
            for user in channel.recipients.iter().chain(recent).chain(members) {
                if seen.insert(user.id) && (matches(user.display_name()) || matches(&user.username)) {
                    out.push((user.display_name().to_owned(), Value::String(user.id.to_string())));
                }
            }
        }
        if let Some(guild) = guild {
            if matches!(field.option.kind, 8 | 9) {
                out.extend(guild.roles.iter().filter(|r| matches(&r.name)).map(|r| (format!("@{}", r.name), Value::String(r.id.to_string()))));
            }
            if field.option.kind == 7 {
                out.extend(guild.channels.iter().filter(|c| c.kind != crate::discord::model::ChannelKind::Category && matches(&c.name)).map(|c| (format!("#{}", c.name), Value::String(c.id.to_string()))));
            }
        }
    }
    out.truncate(25);
    out
}

/// The list over the box for the focused field, or its description when
/// there is nothing to pick. Enter and Tab take the first while it shows.
pub fn suggestions(app: &mut App, ui: &mut egui::Ui, channel: &Channel) {
    let list = candidates(app, channel);
    let Some(draft) = app.slash.as_ref() else { return };
    let Some(field) = draft.fields.get(draft.focus) else { return };
    let p = theme::p();
    if list.is_empty() || field.value.is_some() {
        if !field.option.description.is_empty() {
            let note = format!("{}  —  {}", field.option.name, field.option.description);
            panel(ui, |ui| {
                ui.label(RichText::new(note).size(13.0).color(p.muted));
            });
        }
        return;
    }
    let mut picked = None;
    let focused = (0..draft.fields.len()).any(|i| ui.ctx().memory(|m| m.has_focus(egui::Id::new(("slash-field", i)))));
    if focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter)) {
        picked = Some(0);
    }
    let title = field.option.name.to_uppercase();
    panel(ui, |ui| {
        ui.label(RichText::new(title).font(theme::strong(12.0)).color(p.muted));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, (name, _)) in list.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                if i == 0 || response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, if i == 0 { p.selected } else { p.hover });
                }
                super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 10.0, rect.top()), pos2(rect.right() - 10.0, rect.bottom())), name, FontId::proportional(14.0), p.strong);
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(i);
                }
            }
        });
    });
    if let Some(i) = picked
        && let Some((name, value)) = list.get(i).cloned()
        && let Some(draft) = app.slash.as_mut()
    {
        let focus = draft.focus;
        if let Some(field) = draft.fields.get_mut(focus) {
            field.text = name;
            field.value = Some(value);
        }
        draft.suggestions.clear();
        draft.asked = None;
        if focus + 1 < draft.fields.len() {
            draft.focus += 1;
        }
        draft.refocus = true;
    }
}

/// The options filled so far, as the interaction carries them; `focused`
/// sends that field's text as it is, for its suggestions.
fn filled<'a>(draft: &'a SlashDraft, focused: Option<usize>) -> Vec<(&'a CommandOption, Value)> {
    let mut values = Vec::new();
    for (i, field) in draft.fields.iter().enumerate() {
        if Some(i) == focused {
            values.push((&field.option, Value::String(field.text.clone())));
        } else if let Some(value) = field.value.clone().or_else(|| typed_value(&field.option, &field.text)) {
            values.push((&field.option, value));
        }
    }
    values
}

/// Asks the app for the focused field's suggestions once typing pauses, as
/// Discord does (an interaction of type 4, answered over the gateway).
fn ask_suggestions(app: &mut App, ctx: &egui::Context, channel: &Channel, index: &Index) {
    let Some(draft) = app.slash.as_mut() else { return };
    let Some(field) = draft.fields.get(draft.focus) else { return };
    let entry = &index.entries[draft.entry];
    let command = &index.commands[entry.command];
    if !field.option.autocomplete || command.application == BUILT_IN || field.value.is_some() {
        return;
    }
    if draft.asked.as_ref().is_some_and(|(_, f, text)| *f == draft.focus && *text == field.text) {
        return;
    }
    let pause = std::time::Duration::from_millis(300);
    if let Some(at) = draft.typed_at
        && at.elapsed() < pause
    {
        ctx.request_repaint_after(pause - at.elapsed());
        return;
    }
    let focused = field.option.name.clone();
    let text = field.text.clone();
    let data = crate::discord::commands::data(command, entry, &filled(draft, Some(draft.focus)), Some(&focused));
    let nonce = crate::discord::model::now_snowflake().to_string();
    draft.asked = Some((nonce.clone(), draft.focus, text));
    app.commands.send(crate::backend::Command::SlashCommand { guild: channel.guild_id, channel: channel.id, application: command.application, kind: 4, data, nonce });
}

/// Runs the command: the built-in ones send their text, the apps' an
/// interaction. A required option left empty, or a value that does not
/// read as its type, stops it with a notice.
pub fn run(app: &mut App, channel: &Channel) {
    let Some(key) = app.slash.as_ref().map(|d| d.key) else { return };
    with_index(app, key, |app, index| run_with(app, channel, index));
}

fn run_with(app: &mut App, channel: &Channel, index: &Index) {
    let Some(draft) = app.slash.as_ref() else { return };
    let Some(entry) = index.entries.get(draft.entry) else { return };
    let command = &index.commands[entry.command];
    let mut values = Vec::new();
    let mut missing = None;
    for field in &draft.fields {
        let value = field.value.clone().or_else(|| {
            if !field.option.choices.is_empty() {
                // A choice typed out in full counts as picked.
                field.option.choices.iter().find(|(name, _)| name.eq_ignore_ascii_case(field.text.trim())).map(|(_, v)| v.clone())
            } else if field.option.picks_entity() {
                None
            } else {
                typed_value(&field.option, &field.text)
            }
        });
        match value {
            Some(value) => values.push((&field.option, value)),
            None if field.option.required || !field.text.trim().is_empty() => {
                missing = Some(field.option.name.clone());
                break;
            }
            None => {}
        }
    }
    if let Some(option) = missing {
        app.notify(t!("Fill in \"{option}\" to run the command.", option = option));
        return;
    }
    if command.application == BUILT_IN {
        let message = values.first().and_then(|(_, v)| v.as_str()).unwrap_or_default().to_owned();
        app.composer = built_in_text(&command.name, &message);
        app.slash = None;
        app.actions.push(Action::Send);
        return;
    }
    let data = crate::discord::commands::data(command, entry, &values, None);
    let nonce = crate::discord::model::now_snowflake().to_string();
    app.sent_commands.push(nonce.clone());
    if app.sent_commands.len() > 16 {
        app.sent_commands.remove(0);
    }
    app.commands.send(crate::backend::Command::SlashCommand { guild: channel.guild_id, channel: channel.id, application: command.application, kind: 2, data, nonce });
    app.slash = None;
    app.focus_composer = true;
}

/// Discord's line over an app's answer to a command: "@Ana used /play",
/// the name in its colour and the command in the link colour.
pub fn used_line(app: &mut App, ui: &mut egui::Ui, guild: Option<Id>, user: &crate::discord::model::User, command: &str) {
    let p = theme::p();
    let (name, color) = app.display(guild, user);
    ui.horizontal(|ui| {
        let start = ui.cursor().left_center();
        let spine = [pos2(start.x - 34.0, start.y + 10.0), pos2(start.x - 34.0, start.y + 2.0), pos2(start.x - 6.0, start.y + 2.0)];
        ui.painter().add(egui::Shape::line(spine.to_vec(), Stroke::new(2.0, p.selected)));
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        app.images.circle(ui, rect, Some(&user.avatar_url()), &super::initials(&name), 8.0);
        let mut job = egui::text::LayoutJob::default();
        super::rich_template(
            &mut job,
            t!("{name} used {command}"),
            &egui::TextFormat::simple(FontId::proportional(13.0), p.muted),
            &egui::TextFormat::simple(theme::strong(13.0), color.unwrap_or(p.muted)),
            &[("name", &format!("@{name}")), ("command", &format!("/{command}"))],
        );
        for section in &mut job.sections {
            if job.text.get(section.byte_range.start.0..section.byte_range.end.0).is_some_and(|text| text.starts_with('/')) {
                section.format.color = p.link;
            }
        }
        ui.label(job);
    });
}
