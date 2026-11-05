//! What a member goes through and takes part in: the verification form, the
//! onboarding questions ("Channels & Roles"), the server's events, finding new
//! servers and starting a Stage. Dialogs over the app, with their answers
//! kept in `App::gate_data` by the key of the call that asked.

use std::collections::{BTreeMap, HashSet};

use egui::{Color32, Frame, Margin, RichText, ScrollArea, Sense, Vec2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::backend::Command;
use crate::discord::model::Id;
use crate::theme;

const MANAGE_EVENTS: u64 = 1 << 33;
const CREATE_EVENTS: u64 = 1 << 44;

#[derive(Clone, Default)]
pub struct EventDraft {
    pub name: String,
    pub description: String,
    /// "2026-12-31 20:00", local time.
    pub when: String,
    pub location: String,
    /// A voice channel, or somewhere else (`location`).
    pub channel: Option<Id>,
}

pub enum Gate {
    Verification { guild: Id, answers: BTreeMap<usize, Value>, agreed: bool },
    Onboarding { guild: Id, picked: Option<HashSet<String>>, page: usize },
    Events { guild: Id, draft: Option<EventDraft> },
    Discover { query: String, asked: String },
    Stage { channel: Id, topic: String },
}

fn call(key: &str, method: &'static str, path: String, body: Option<Value>) -> Command {
    Command::Api { key: key.into(), method, path, body }
}

/// Opens a dialog, forgetting what an earlier one fetched.
pub fn open(app: &mut App, gate: Gate) {
    app.gate_data.clear();
    app.modal = Some(crate::app::Modal::Gate(gate));
}

/// What `key` answered, asking the first time.
fn fetch<'a>(app: &'a mut App, key: &str, path: impl FnOnce() -> String) -> Option<&'a Result<Value, String>> {
    if !app.gate_asked.contains(key) {
        app.gate_asked.insert(key.to_owned());
        app.gate_data.remove(key);
        let path = path();
        app.commands.send(crate::admin::get(key, path));
    }
    app.gate_data.get(key)
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::strong(20.0)).color(theme::p().strong));
    ui.add_space(8.0);
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(theme::strong(12.0)).color(theme::p().muted));
}

fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(theme::p().accent)).clicked()
}

/// Waits for an answer: the spinner, or what went wrong.
fn answered(ui: &mut egui::Ui, answer: Option<Result<Value, String>>) -> Option<Value> {
    match answer {
        Some(Ok(value)) => Some(value),
        Some(Err(error)) => {
            ui.label(RichText::new(error).color(theme::p().red));
            None
        }
        None => {
            ui.spinner();
            None
        }
    }
}

/// Draws the dialog; false when it closes.
pub fn show(app: &mut App, ui: &mut egui::Ui, gate: &mut Gate) -> bool {
    match gate {
        Gate::Verification { guild, answers, agreed } => verification(app, ui, *guild, answers, agreed),
        Gate::Onboarding { guild, picked, page } => onboarding(app, ui, *guild, picked, page),
        Gate::Events { guild, draft } => events(app, ui, *guild, draft),
        Gate::Discover { query, asked } => discover(app, ui, query, asked),
        Gate::Stage { channel, topic } => stage(app, ui, *channel, topic),
    }
}

/// The form a server with "Rules Screening" asks before letting someone talk:
/// the rules to agree to and, where set, questions.
fn verification(app: &mut App, ui: &mut egui::Ui, guild: Id, answers: &mut BTreeMap<usize, Value>, agreed: &mut bool) -> bool {
    title(ui, t!("Complete"));
    let key = format!("gate:verification:{guild}");
    let answer = fetch(app, &key, || format!("/guilds/{guild}/member-verification?with_guild=false")).cloned();
    let Some(form) = answered(ui, answer) else { return !ui.button(t!("Cancel")).clicked() };
    let fields = form.get("form_fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut ready = true;
    ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for (i, field) in fields.iter().enumerate() {
            let label = field.get("label").and_then(Value::as_str).unwrap_or_default();
            let required = field.get("required").and_then(Value::as_bool).unwrap_or(false);
            ui.add_space(6.0);
            match field.get("field_type").and_then(Value::as_str).unwrap_or_default() {
                "TERMS" => {
                    for (n, rule) in field.get("values").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).enumerate() {
                        ui.label(format!("{}. {rule}", n + 1));
                    }
                    ui.checkbox(agreed, label);
                    ready &= *agreed || !required;
                }
                "MULTIPLE_CHOICE" => {
                    heading(ui, label);
                    let picked = answers.get(&i).and_then(Value::as_u64);
                    for (n, choice) in field.get("choices").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).enumerate() {
                        if ui.radio(picked == Some(n as u64), choice).clicked() {
                            answers.insert(i, json!(n));
                        }
                    }
                    ready &= picked.is_some() || !required;
                }
                _ => {
                    heading(ui, label);
                    let mut text = answers.get(&i).and_then(Value::as_str).unwrap_or_default().to_owned();
                    if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(f32::INFINITY)).changed() {
                        answers.insert(i, json!(text));
                    }
                    ready &= !text.trim().is_empty() || !required;
                }
            }
        }
    });
    ui.add_space(10.0);
    let mut open = true;
    ui.horizontal(|ui| {
        if ui.button(t!("Cancel")).clicked() {
            open = false;
        }
        if primary(ui, t!("Submit"), ready) {
            let filled: Vec<Value> = fields
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let mut field = field.clone();
                    field["response"] = if field.get("field_type").and_then(Value::as_str) == Some("TERMS") { json!(*agreed) } else { answers.get(&i).cloned().unwrap_or(Value::Null) };
                    field
                })
                .collect();
            app.commands.send(call("pin", "PUT", format!("/guilds/{guild}/requests/@me"), Some(json!({ "version": form.get("version"), "form_fields": filled }))));
            open = false;
        }
    });
    open
}

/// The server's questions, a page each, whose answers open channels and give
/// roles. The same dialog changes them later, as "Channels & Roles".
fn onboarding(app: &mut App, ui: &mut egui::Ui, guild: Id, picked: &mut Option<HashSet<String>>, page: &mut usize) -> bool {
    title(ui, t!("Channels & Roles"));
    let key = format!("gate:onboarding:{guild}");
    let answer = fetch(app, &key, || format!("/guilds/{guild}/onboarding")).cloned();
    let Some(data) = answered(ui, answer) else { return !ui.button(t!("Cancel")).clicked() };
    let prompts = data.get("prompts").and_then(Value::as_array).cloned().unwrap_or_default();
    let picked = picked.get_or_insert_with(|| data.get("responses").and_then(Value::as_array).into_iter().flatten().filter_map(|id| id.as_str().map(str::to_owned)).collect());
    let Some(prompt) = prompts.get(*page) else {
        ui.label(RichText::new(t!("No results found")).color(theme::p().muted));
        return !ui.button(t!("Close")).clicked();
    };
    let single = prompt.get("single_select").and_then(Value::as_bool).unwrap_or(false);
    let required = prompt.get("required").and_then(Value::as_bool).unwrap_or(false);
    ui.label(RichText::new(prompt.get("title").and_then(Value::as_str).unwrap_or_default()).font(theme::strong(16.0)).color(theme::p().strong));
    ui.add_space(8.0);
    let options = prompt.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
    let ids: Vec<String> = options.iter().filter_map(|o| o.get("id")?.as_str().map(str::to_owned)).collect();
    ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        for option in &options {
            let Some(id) = option.get("id").and_then(Value::as_str) else { continue };
            let on = picked.contains(id);
            let label = option.get("title").and_then(Value::as_str).unwrap_or_default();
            let response = ui.selectable_label(on, label);
            if let Some(about) = option.get("description").and_then(Value::as_str).filter(|about| !about.is_empty()) {
                ui.label(RichText::new(about).size(12.5).color(theme::p().muted));
            }
            if response.clicked() {
                if on {
                    picked.remove(id);
                } else {
                    if single {
                        picked.retain(|other| !ids.contains(other));
                    }
                    picked.insert(id.to_owned());
                }
            }
            ui.add_space(4.0);
        }
    });
    ui.add_space(10.0);
    let answered_here = ids.iter().any(|id| picked.contains(id));
    let last = *page + 1 >= prompts.len();
    let mut open = true;
    ui.horizontal(|ui| {
        if *page > 0 {
            if ui.button(t!("Back")).clicked() {
                *page -= 1;
            }
        } else if ui.button(t!("Cancel")).clicked() {
            open = false;
        }
        if primary(ui, if last { t!("Finish") } else { t!("Next") }, answered_here || !required) {
            if last {
                let now = crate::app::now_ms();
                let seen_prompts: serde_json::Map<String, Value> = prompts.iter().filter_map(|p| Some((p.get("id")?.as_str()?.to_owned(), json!(now)))).collect();
                let seen_options: serde_json::Map<String, Value> = prompts.iter().flat_map(|p| p.get("options").and_then(Value::as_array).cloned().unwrap_or_default()).filter_map(|o| Some((o.get("id")?.as_str()?.to_owned(), json!(now)))).collect();
                let responses: Vec<&String> = picked.iter().collect();
                app.commands.send(call("pin", "POST", format!("/guilds/{guild}/onboarding-responses"), Some(json!({ "onboarding_responses": responses, "onboarding_prompts_seen": seen_prompts, "onboarding_responses_seen": seen_options }))));
                open = false;
            } else {
                *page += 1;
            }
        }
    });
    open
}

/// RFC 3339 from "2026-12-31 20:00" in the machine's time zone.
fn event_time(text: &str) -> Option<String> {
    let civil: jiff::civil::DateTime = format!("{}:00", text.trim().replacen(' ', "T", 1)).parse().ok()?;
    Some(civil.to_zoned(jiff::tz::TimeZone::system()).ok()?.timestamp().to_string())
}

fn events(app: &mut App, ui: &mut egui::Ui, guild: Id, draft: &mut Option<EventDraft>) -> bool {
    let p = theme::p();
    let me = app.me_id();
    let perms = app.guild(guild).map_or(0, |g| g.my_permissions(me));
    if let Some(event) = draft {
        title(ui, t!("Create Event"));
        heading(ui, t!("Event Topic"));
        ui.add(egui::TextEdit::singleline(&mut event.name).char_limit(100).desired_width(f32::INFINITY));
        ui.add_space(6.0);
        heading(ui, t!("Start Time"));
        ui.add(egui::TextEdit::singleline(&mut event.when).hint_text("2026-12-31 20:00").desired_width(f32::INFINITY));
        ui.add_space(6.0);
        heading(ui, t!("Location"));
        let voices: Vec<(Id, String)> = app.guild(guild).map_or_else(Vec::new, |g| g.channels.iter().filter(|c| c.kind == crate::discord::model::ChannelKind::Voice && g.can_view(me, c)).map(|c| (c.id, c.title())).collect());
        let shown = event.channel.and_then(|id| voices.iter().find(|(c, _)| *c == id)).map_or_else(|| t!("Somewhere Else").to_owned(), |(_, name)| name.clone());
        egui::ComboBox::from_id_salt("event-place").selected_text(shown).width(300.0).show_ui(ui, |ui| {
            if ui.selectable_label(event.channel.is_none(), t!("Somewhere Else")).clicked() {
                event.channel = None;
            }
            for (id, name) in &voices {
                if ui.selectable_label(event.channel == Some(*id), name).clicked() {
                    event.channel = Some(*id);
                }
            }
        });
        if event.channel.is_none() {
            ui.add(egui::TextEdit::singleline(&mut event.location).char_limit(100).desired_width(f32::INFINITY));
        }
        ui.add_space(6.0);
        heading(ui, t!("Description"));
        ui.add(egui::TextEdit::multiline(&mut event.description).desired_rows(3).desired_width(f32::INFINITY));
        ui.add_space(10.0);
        let start = event_time(&event.when);
        let ready = !event.name.trim().is_empty() && start.is_some() && (event.channel.is_some() || !event.location.trim().is_empty());
        let mut done = false;
        let mut back = false;
        ui.horizontal(|ui| {
            if ui.button(t!("Back")).clicked() {
                back = true;
            }
            if primary(ui, t!("Create Event"), ready) {
                let start = start.clone().unwrap_or_default();
                let mut body = json!({ "name": event.name.trim(), "description": event.description.trim(), "privacy_level": 2, "scheduled_start_time": start });
                match event.channel {
                    Some(channel) => {
                        body["entity_type"] = json!(2);
                        body["channel_id"] = json!(channel.to_string());
                    }
                    None => {
                        // Somewhere else needs an end: two hours later, which Discord's form proposes too.
                        let end = start.parse::<jiff::Timestamp>().ok().and_then(|at| at.checked_add(jiff::SignedDuration::from_hours(2)).ok()).map(|at| at.to_string());
                        body["entity_type"] = json!(3);
                        body["entity_metadata"] = json!({ "location": event.location.trim() });
                        body["scheduled_end_time"] = json!(end);
                    }
                }
                app.commands.send(call("gate:event-made", "POST", format!("/guilds/{guild}/scheduled-events"), Some(body)));
                app.gate_asked.remove(&format!("gate:events:{guild}"));
                done = true;
            }
        });
        if back || done {
            *draft = None;
        }
        return true;
    }
    title(ui, t!("Events"));
    let key = format!("gate:events:{guild}");
    let answer = fetch(app, &key, || format!("/guilds/{guild}/scheduled-events?with_user_count=true")).cloned();
    let list = answer.as_ref().and_then(|a| a.as_ref().ok()).and_then(Value::as_array).cloned();
    if list.is_none() {
        answered(ui, answer.clone());
    }
    ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        let list = list.unwrap_or_default();
        if answer.is_some() && list.is_empty() {
            ui.label(RichText::new(t!("No results found")).color(p.muted));
        }
        for event in &list {
            let id = event.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            let status = event.get("status").and_then(Value::as_u64).unwrap_or(1);
            Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let when = event.get("scheduled_start_time").and_then(Value::as_str).and_then(|at| at.parse::<jiff::Timestamp>().ok()).map(|at| crate::text::time(at.as_millisecond() as u64)).unwrap_or_default();
                ui.label(RichText::new(when).size(12.5).color(if status == 2 { p.green } else { p.muted }));
                ui.label(RichText::new(event.get("name").and_then(Value::as_str).unwrap_or_default()).font(theme::strong(16.0)).color(p.strong));
                if let Some(about) = event.get("description").and_then(Value::as_str).filter(|about| !about.is_empty()) {
                    ui.label(RichText::new(about).size(13.0).color(p.text));
                }
                if let Some(place) = event.pointer("/entity_metadata/location").and_then(Value::as_str) {
                    ui.label(RichText::new(place).size(12.5).color(p.muted));
                }
                let count = event.get("user_count").and_then(Value::as_u64).unwrap_or(0);
                ui.label(RichText::new(tn!(count, "{count} interested", "{count} interested")).size(12.0).color(p.muted));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let on = app.event_rsvp.contains(&id);
                    if ui.selectable_label(on, t!("Interested")).clicked() {
                        app.commands.send(call("pin", "PUT", format!("/guilds/{guild}/scheduled-events/{id}/users/@me"), Some(json!({ "response": u8::from(!on) }))));
                        if on {
                            app.event_rsvp.remove(&id);
                        } else {
                            app.event_rsvp.insert(id.clone());
                        }
                    }
                    if perms & MANAGE_EVENTS != 0 {
                        let path = format!("/guilds/{guild}/scheduled-events/{id}");
                        // Starting, ending and cancelling are status changes.
                        let change = match status {
                            1 => Some((t!("Start Event"), 2)),
                            2 => Some((t!("End Event"), 3)),
                            _ => None,
                        };
                        if let Some((label, next)) = change
                            && ui.button(label).clicked()
                        {
                            app.commands.send(call("gate:event-made", "PATCH", path.clone(), Some(json!({ "status": next }))));
                        }
                        if status == 1 && ui.button(t!("Cancel Event")).clicked() {
                            app.commands.send(call("gate:event-made", "PATCH", path, Some(json!({ "status": 4 }))));
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
    ui.add_space(8.0);
    let mut open = true;
    ui.horizontal(|ui| {
        if ui.button(t!("Close")).clicked() {
            open = false;
        }
        if perms & (MANAGE_EVENTS | CREATE_EVENTS) != 0 && primary(ui, t!("Create Event"), true) {
            *draft = Some(EventDraft::default());
        }
    });
    open
}

/// Discord's server discovery: what it features, or a search.
fn discover(app: &mut App, ui: &mut egui::Ui, query: &mut String, asked: &mut String) -> bool {
    let p = theme::p();
    title(ui, t!("Find your community on Discord"));
    let response = ui.add(egui::TextEdit::singleline(query).hint_text(t!("Explore Discoverable Servers")).desired_width(f32::INFINITY));
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        asked.clone_from(query);
        app.gate_asked.remove("gate:discover");
    }
    ui.add_space(8.0);
    let term = asked.trim().to_owned();
    let answer = fetch(app, "gate:discover", || {
        if term.is_empty() {
            "/discoverable-guilds?limit=24&offset=0".to_owned()
        } else {
            format!("/discoverable-guilds/search?query={}&limit=24&offset=0&with_counts=true", crate::discord::rest::percent_encode(&term))
        }
    })
    .cloned();
    let list = answer.as_ref().and_then(|a| a.as_ref().ok()).and_then(|value| value.get("guilds").or_else(|| value.get("results")).or(Some(value))).and_then(Value::as_array).cloned();
    if list.is_none() {
        answered(ui, answer);
    }
    ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
        for guild in list.iter().flatten() {
            let Some(id) = guild.get("id").and_then(Value::as_str) else { continue };
            let name = guild.get("name").and_then(Value::as_str).unwrap_or_default();
            Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                    let icon = guild.get("icon").and_then(Value::as_str).map(|hash| format!("https://cdn.discordapp.com/icons/{id}/{hash}.png?size=96"));
                    app.images.circle(ui, rect, icon.as_deref(), &super::initials(name), 12.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 80.0);
                        ui.label(RichText::new(name).font(theme::strong(15.0)).color(p.strong));
                        if let Some(about) = guild.get("description").and_then(Value::as_str) {
                            ui.label(RichText::new(about).size(12.5).color(p.muted));
                        }
                        if let Some(count) = guild.get("approximate_member_count").and_then(Value::as_u64) {
                            ui.label(RichText::new(tn!(count, "{count} member", "{count} members")).size(12.0).color(p.muted));
                        }
                    });
                    if primary(ui, t!("Join"), true) {
                        app.commands.send(call("join", "PUT", format!("/guilds/{id}/members/@me?lurker=false&location=Server%20Discovery"), Some(json!({}))));
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
    ui.add_space(8.0);
    !ui.button(t!("Close")).clicked()
}

fn stage(app: &mut App, ui: &mut egui::Ui, channel: Id, topic: &mut String) -> bool {
    title(ui, t!("Start Stage"));
    heading(ui, t!("Stage Topic"));
    ui.add(egui::TextEdit::singleline(topic).char_limit(120).desired_width(f32::INFINITY));
    ui.add_space(10.0);
    let mut open = true;
    ui.horizontal(|ui| {
        if ui.button(t!("Cancel")).clicked() {
            open = false;
        }
        if primary(ui, t!("Start Stage"), !topic.trim().is_empty()) {
            app.commands.send(call("pin", "POST", "/stage-instances".into(), Some(json!({ "channel_id": channel.to_string(), "topic": topic.trim(), "privacy_level": 2, "send_start_notification": false }))));
            open = false;
        }
    });
    open
}

/// The Stage's line in the call panel: asking to speak, or leaving the stage.
pub fn stage_row(app: &mut App, ui: &mut egui::Ui) {
    let Some((Some(guild), channel)) = app.voice.channel else { return };
    if app.channel(channel).is_none_or(|c| c.kind != crate::discord::model::ChannelKind::Stage) {
        return;
    }
    let me = app.me_id();
    let suppressed = app.voice_stage.get(&me).is_none_or(|(suppress, _)| *suppress);
    let hand = app.voice_stage.get(&me).is_some_and(|(_, hand)| *hand);
    let path = format!("/guilds/{guild}/voice-states/@me");
    let at_now = || jiff::Timestamp::now().to_string();
    ui.horizontal(|ui| {
        if suppressed {
            let label = if hand { t!("Lower Hand") } else { t!("Request to Speak") };
            if super::form::button(ui, label, super::form::Kind::Secondary).clicked() {
                let at = if hand { Value::Null } else { json!(at_now()) };
                app.commands.send(call("pin", "PATCH", path.clone(), Some(json!({ "channel_id": channel.to_string(), "request_to_speak_timestamp": at }))));
            }
        } else if super::form::button(ui, t!("Move to Audience"), super::form::Kind::Secondary).clicked() {
            app.commands.send(call("pin", "PATCH", path.clone(), Some(json!({ "channel_id": channel.to_string(), "suppress": true, "self_video": false, "self_stream": false }))));
        }
    });
    ui.add_space(vec2(0.0, 4.0).y);
}
