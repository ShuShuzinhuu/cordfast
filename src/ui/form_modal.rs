//! The forms apps open in answer to a button or a command (Discord's
//! interaction modal, the bundle's `InteractionModal` and its
//! `INTERACTION_MODAL_CREATE`): the app's icon and the form's title, the
//! warning that it goes to the app, its fields, and Cancel and Submit across
//! the bottom. Submitting sends a `MODAL_SUBMIT` (5) interaction with every
//! field's value in the form's own shape; the form closes on
//! `INTERACTION_SUCCESS` and says "Something went wrong" on a failure.
//!
//! Fields: text inputs (short and paragraph), labels with their
//! descriptions, text displays, string, role, channel and user selects,
//! radio groups, checkbox groups and checkboxes. File uploads are shown but
//! cannot be filled yet.

use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, Frame, Margin, RichText, ScrollArea, Sense, Stroke, Vec2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::discord::model::Id;
use crate::theme;

/// What one field holds.
#[derive(Clone, Debug)]
pub enum Field {
    Text(String),
    Values(Vec<String>),
    Flag(bool),
}

pub struct Form {
    /// The interaction that opened it, which the submit names.
    id: String,
    channel: Id,
    application: Id,
    app_name: String,
    app_icon: Option<String>,
    custom_id: String,
    title: String,
    components: Vec<Value>,
    values: HashMap<String, Field>,
    /// Required fields left empty at the last Submit.
    missing: HashSet<String>,
    /// The submit waiting for its answer.
    pub nonce: Option<String>,
    pub error: Option<String>,
}

fn sid(value: &Value, key: &str) -> Option<Id> {
    value.get(key).and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()))
}

impl Form {
    pub fn parse(d: &Value) -> Option<Self> {
        let application = d.get("application")?;
        let app_id = sid(application, "id")?;
        let app_name = application
            .pointer("/bot/username")
            .or_else(|| application.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let app_icon = application.get("icon").and_then(Value::as_str).map(|icon| format!("https://cdn.discordapp.com/app-icons/{app_id}/{icon}.png?size=64"));
        let components: Vec<Value> = d.get("components").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut values = HashMap::new();
        for component in &components {
            defaults(component, &mut values);
        }
        Some(Self {
            id: d.get("id").and_then(Value::as_str)?.to_owned(),
            channel: sid(d, "channel_id")?,
            application: app_id,
            app_name,
            app_icon,
            custom_id: d.get("custom_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
            title: d.get("title").and_then(Value::as_str).unwrap_or_default().to_owned(),
            components,
            values,
            missing: HashSet::new(),
            nonce: None,
            error: None,
        })
    }
}

/// The values a form starts with: a text input's `value`, the options
/// marked `default`.
fn defaults(component: &Value, values: &mut HashMap<String, Field>) {
    let kind = component.get("type").and_then(Value::as_u64).unwrap_or(0);
    let custom = component.get("custom_id").and_then(Value::as_str).map(str::to_owned);
    match kind {
        1 => component.get("components").and_then(Value::as_array).into_iter().flatten().for_each(|c| defaults(c, values)),
        18 => {
            if let Some(inner) = component.get("component") {
                defaults(inner, values);
            }
        }
        4 => {
            if let Some(custom) = custom {
                values.insert(custom, Field::Text(component.get("value").and_then(Value::as_str).unwrap_or_default().to_owned()));
            }
        }
        3 | 21 | 22 => {
            if let Some(custom) = custom {
                let chosen: Vec<String> = options(component).into_iter().filter(|o| o.default).map(|o| o.value).collect();
                values.insert(custom, if kind == 21 { Field::Text(chosen.first().cloned().unwrap_or_default()) } else { Field::Values(chosen) });
            }
        }
        23 => {
            if let Some(custom) = custom {
                values.insert(custom, Field::Flag(component.get("default").and_then(Value::as_bool).unwrap_or(false)));
            }
        }
        5..=8 | 19 => {
            if let Some(custom) = custom {
                let chosen = component.get("default_values").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned)).collect();
                values.insert(custom, Field::Values(chosen));
            }
        }
        _ => {}
    }
}

struct Choice {
    label: String,
    value: String,
    description: Option<String>,
    default: bool,
}

fn options(component: &Value) -> Vec<Choice> {
    component
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|o| Choice {
            label: o.get("label").and_then(Value::as_str).unwrap_or_default().to_owned(),
            value: o.get("value").and_then(Value::as_str).unwrap_or_default().to_owned(),
            description: o.get("description").and_then(Value::as_str).map(str::to_owned),
            default: o.get("default").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect()
}

/// The submit's components: the form's own tree with each field's value,
/// as Discord's `MODAL_SUBMIT` builds it.
fn answers(component: &Value, values: &HashMap<String, Field>) -> Value {
    let kind = component.get("type").and_then(Value::as_u64).unwrap_or(0);
    let custom = component.get("custom_id").and_then(Value::as_str).unwrap_or_default();
    let field = values.get(custom);
    match kind {
        1 => json!({ "type": 1, "components": component.get("components").and_then(Value::as_array).into_iter().flatten().map(|c| answers(c, values)).collect::<Vec<_>>() }),
        18 => json!({ "type": 18, "component": component.get("component").map(|c| answers(c, values)) }),
        10 => json!({ "type": 10 }),
        4 | 21 => json!({ "type": kind, "custom_id": custom, "value": match field { Some(Field::Text(text)) if !text.is_empty() || kind == 4 => Value::String(text.clone()), _ => Value::Null } }),
        23 => json!({ "type": kind, "custom_id": custom, "value": matches!(field, Some(Field::Flag(true))) }),
        _ => json!({ "type": kind, "custom_id": custom, "values": match field { Some(Field::Values(list)) => list.clone(), _ => Vec::new() } }),
    }
}

/// Required fields left empty, and text shorter than its minimum.
fn check(component: &Value, values: &HashMap<String, Field>, missing: &mut HashSet<String>) {
    let kind = component.get("type").and_then(Value::as_u64).unwrap_or(0);
    match kind {
        1 => component.get("components").and_then(Value::as_array).into_iter().flatten().for_each(|c| check(c, values, missing)),
        18 => {
            if let Some(inner) = component.get("component") {
                check(inner, values, missing);
            }
        }
        _ => {
            let Some(custom) = component.get("custom_id").and_then(Value::as_str) else { return };
            // Text inputs are required unless they say otherwise; the rest only when they say so.
            let required = component.get("required").and_then(Value::as_bool).unwrap_or(kind == 4);
            let empty = match values.get(custom) {
                Some(Field::Text(text)) => text.trim().is_empty(),
                Some(Field::Values(list)) => list.len() < component.get("min_values").and_then(Value::as_u64).unwrap_or(1) as usize,
                Some(Field::Flag(on)) => !on,
                None => true,
            };
            let short = match values.get(custom) {
                Some(Field::Text(text)) if !text.is_empty() => text.chars().count() < component.get("min_length").and_then(Value::as_u64).unwrap_or(0) as usize,
                _ => false,
            };
            if (required && empty) || short {
                missing.insert(custom.to_owned());
            }
        }
    }
}

/// Draws the open form, if any.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut form) = app.form.take() else { return };
    let p = theme::p();
    let screen = ctx.content_rect();
    let mut close = false;
    let mut submit = false;
    // The dimmed app behind it; a click there does not close a form.
    egui::Area::new(egui::Id::new("form-dim")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(178));
    });
    let width = 440.0_f32.min(screen.width() - 32.0);
    egui::Area::new(egui::Id::new("form-window")).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
        Frame::new().fill(p.chat).stroke(Stroke::new(1.0, p.border)).corner_radius(12).show(ui, |ui| {
            ui.set_width(width);
            ui.set_max_height(screen.height() - 80.0);
            // The app's icon and the title.
            Frame::new().inner_margin(Margin { left: 16, right: 16, top: 16, bottom: 8 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                    app.images.circle(ui, icon, form.app_icon.as_deref(), &super::initials(&form.app_name), 12.0);
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(&form.title).font(theme::strong(20.0)).color(p.strong)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (x, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                        crate::icons::paint(ui, x, crate::icons::Icon::Close, if response.hovered() { p.strong } else { p.muted });
                        if response.on_hover_text(t!("Close")).clicked() {
                            close = true;
                        }
                    });
                });
            });
            ScrollArea::vertical().max_height(screen.height() - 220.0).show(ui, |ui| {
                Frame::new().inner_margin(Margin { left: 16, right: 16, top: 0, bottom: 8 }).show(ui, |ui| {
                    ui.set_width(width - 32.0);
                    if let Some(error) = &form.error {
                        banner(ui, error, p.red);
                        ui.add_space(8.0);
                    }
                    // The app's name in bold, as Discord's notice has it.
                    let plain = egui::TextFormat::simple(egui::FontId::proportional(14.0), p.text);
                    let bold = egui::TextFormat::simple(theme::strong(14.0), p.strong);
                    let mut job = egui::text::LayoutJob::default();
                    super::rich_template(&mut job, t!("This form will be submitted to {applicationName}. Do not share passwords or other sensitive information."), &plain, &bold, &[("applicationName", &form.app_name)]);
                    notice(ui, job, p.warning);
                    ui.add_space(12.0);
                    let components = form.components.clone();
                    for component in &components {
                        field(app, ui, &mut form, component, None);
                        ui.add_space(12.0);
                    }
                });
            });
            // Cancel and Submit, the full width.
            Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let half = (ui.available_width() - 8.0) / 2.0;
                    if wide(ui, t!("Cancel"), half, false, false).clicked() {
                        close = true;
                    }
                    if wide(ui, t!("Submit"), half, true, form.nonce.is_some()).clicked() && form.nonce.is_none() {
                        submit = true;
                    }
                });
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if submit {
        form.missing.clear();
        let mut missing = HashSet::new();
        for component in &form.components {
            check(component, &form.values, &mut missing);
        }
        form.missing = missing;
        if form.missing.is_empty() {
            let nonce = crate::discord::model::now_snowflake().to_string();
            let components: Vec<Value> = form.components.iter().map(|c| answers(c, &form.values)).collect();
            let guild = app.channel(form.channel).and_then(|c| c.guild_id);
            form.error = None;
            form.nonce = Some(nonce.clone());
            app.commands.send(crate::backend::Command::SlashCommand {
                guild,
                channel: form.channel,
                application: form.application,
                kind: 5,
                data: json!({ "id": form.id, "custom_id": form.custom_id, "components": components }),
                nonce,
            });
        }
    }
    if !close {
        app.form = Some(form);
    }
}

/// Discord's notice: its colour on a tint, the text beside an "!".
fn banner(ui: &mut egui::Ui, text: &str, color: Color32) {
    let p = theme::p();
    Frame::new().fill(color.gamma_multiply(0.12)).stroke(Stroke::new(1.0, color.gamma_multiply(0.6))).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("!").font(theme::strong(14.0)).color(color));
            ui.add(egui::Label::new(RichText::new(text).size(14.0).color(p.text)).wrap());
        });
    });
}

/// `banner` for text with bold parts.
fn notice(ui: &mut egui::Ui, mut job: egui::text::LayoutJob, color: Color32) {
    Frame::new().fill(color.gamma_multiply(0.12)).stroke(Stroke::new(1.0, color.gamma_multiply(0.6))).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("!").font(theme::strong(14.0)).color(color));
            job.wrap.max_width = ui.available_width();
            ui.add(egui::Label::new(job).wrap());
        });
    });
}

/// A button across half the footer.
fn wide(ui: &mut egui::Ui, text: &str, width: f32, primary: bool, busy: bool) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 38.0), Sense::click());
    let fill = match (primary, response.hovered()) {
        (true, false) => p.accent,
        (true, true) => p.accent.gamma_multiply(0.85),
        (false, false) => p.selected,
        (false, true) => p.hover.gamma_multiply(1.6),
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let label = if busy { "…" } else { text };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::strong(14.0), if primary { p.on_accent } else { p.strong });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// One component of the form; `label` is the Label (18) around it.
fn field(app: &mut App, ui: &mut egui::Ui, form: &mut Form, component: &Value, label: Option<&Value>) {
    let p = theme::p();
    let kind = component.get("type").and_then(Value::as_u64).unwrap_or(0);
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    match kind {
        1 => {
            for inner in component.get("components").and_then(Value::as_array).into_iter().flatten() {
                field(app, ui, form, inner, None);
            }
            return;
        }
        18 => {
            if let Some(inner) = component.get("component") {
                field(app, ui, form, inner, Some(component));
            }
            return;
        }
        10 => {
            let content = text(component, "content").unwrap_or_default();
            ui.add(egui::Label::new(RichText::new(content).size(15.0).color(p.text)).wrap());
            return;
        }
        _ => {}
    }
    let custom = text(component, "custom_id").unwrap_or_default();
    // The label over the field (a Label's, or a text input's own), with
    // Discord's red star when it is required, and its description.
    let title = label.and_then(|l| text(l, "label")).or_else(|| text(component, "label"));
    let description = label.and_then(|l| text(l, "description"));
    let required = component.get("required").and_then(Value::as_bool).unwrap_or(kind == 4);
    if let Some(title) = title {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.label(RichText::new(title).font(theme::strong(15.0)).color(p.strong));
            if required {
                ui.label(RichText::new("*").font(theme::strong(15.0)).color(p.red));
            }
        });
    }
    if let Some(description) = description {
        ui.label(RichText::new(description).size(13.0).color(p.muted));
    }
    ui.add_space(6.0);
    let missing = form.missing.contains(&custom);
    let border = if missing { p.red } else { p.border };
    match kind {
        4 => {
            let paragraph = component.get("style").and_then(Value::as_u64) == Some(2);
            let max = component.get("max_length").and_then(Value::as_u64).unwrap_or(4000) as usize;
            let placeholder = text(component, "placeholder").unwrap_or_default();
            let entry = form.values.entry(custom.clone()).or_insert_with(|| Field::Text(String::new()));
            if let Field::Text(value) = entry {
                Frame::new().fill(p.rail).stroke(Stroke::new(1.0, border)).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
                    let edit = if paragraph { egui::TextEdit::multiline(value).desired_rows(4) } else { egui::TextEdit::singleline(value) };
                    ui.add(edit.frame(Frame::NONE).char_limit(max).hint_text(RichText::new(placeholder).color(p.faint)).desired_width(f32::INFINITY));
                });
                if paragraph && component.get("max_length").is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(RichText::new(format!("{}/{max}", value.chars().count())).size(12.0).color(p.muted));
                    });
                }
            }
        }
        3 | 21 | 22 | 23 => choices(ui, form, component, kind, &custom, border),
        5..=8 => {
            // Roles, channels and people of the form's server.
            let guild = app.channel(form.channel).and_then(|c| c.guild_id).and_then(|g| app.guild(g)).cloned();
            let me = app.me_id();
            let list: Vec<(String, String)> = match (kind, &guild) {
                (6, Some(g)) => g.roles.iter().filter(|r| r.id != g.id).map(|r| (r.id.to_string(), r.name.clone())).collect(),
                (8, Some(g)) => g.channels.iter().filter(|c| g.can_view(me, c)).map(|c| (c.id.to_string(), c.title())).collect(),
                (_, Some(g)) => app.members.keys().filter(|(guild, _)| *guild == g.id).filter_map(|(_, user)| app.users.get(user).map(|u| (u.id.to_string(), u.display_name().to_owned()))).collect(),
                _ => Vec::new(),
            };
            let max = component.get("max_values").and_then(Value::as_u64).unwrap_or(1) as usize;
            let placeholder = text(component, "placeholder").unwrap_or_else(|| t!("Make a selection").to_owned());
            let chosen = match form.values.get(&custom) {
                Some(Field::Values(values)) => values.clone(),
                _ => Vec::new(),
            };
            let shown = if chosen.is_empty() { placeholder } else { list.iter().filter(|(id, _)| chosen.contains(id)).map(|(_, name)| name.clone()).collect::<Vec<_>>().join(", ") };
            egui::ComboBox::from_id_salt(("form-select", custom.as_str())).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                for (id, name) in &list {
                    let mut on = chosen.contains(id);
                    if ui.checkbox(&mut on, name).changed() {
                        let mut next: Vec<String> = chosen.iter().filter(|c| *c != id).cloned().collect();
                        if on {
                            if max <= 1 {
                                next.clear();
                            }
                            next.push(id.clone());
                        }
                        next.truncate(max.max(1));
                        form.values.insert(custom.clone(), Field::Values(next));
                    }
                }
            });
        }
        19 => {
            ui.label(RichText::new(t!("File uploads in forms are not supported yet.")).size(13.0).color(p.muted));
        }
        _ => {}
    }
    if missing {
        ui.label(RichText::new(t!("This field is required.")).size(12.0).color(p.red));
    }
}

/// A string select, a radio group, a checkbox group or a checkbox.
fn choices(ui: &mut egui::Ui, form: &mut Form, component: &Value, kind: u64, custom: &str, border: Color32) {
    let p = theme::p();
    let list = options(component);
    match kind {
        23 => {
            let mut on = matches!(form.values.get(custom), Some(Field::Flag(true)));
            if ui.checkbox(&mut on, "").changed() {
                form.values.insert(custom.to_owned(), Field::Flag(on));
            }
        }
        21 => {
            let current = match form.values.get(custom) {
                Some(Field::Text(value)) => value.clone(),
                _ => String::new(),
            };
            for choice in &list {
                if ui.radio(current == choice.value, &choice.label).clicked() {
                    form.values.insert(custom.to_owned(), Field::Text(choice.value.clone()));
                }
                if let Some(description) = &choice.description {
                    ui.label(RichText::new(description).size(12.0).color(p.muted));
                }
            }
        }
        _ => {
            let max = if kind == 22 { list.len() } else { component.get("max_values").and_then(Value::as_u64).unwrap_or(1) as usize };
            let chosen = match form.values.get(custom) {
                Some(Field::Values(values)) => values.clone(),
                _ => Vec::new(),
            };
            let toggle = |form: &mut Form, value: &str, on: bool| {
                let mut next: Vec<String> = chosen.iter().filter(|c| c.as_str() != value).cloned().collect();
                if on {
                    if max <= 1 {
                        next.clear();
                    }
                    next.push(value.to_owned());
                }
                next.truncate(max.max(1));
                form.values.insert(custom.to_owned(), Field::Values(next));
            };
            if kind == 22 {
                for choice in &list {
                    let mut on = chosen.contains(&choice.value);
                    if ui.checkbox(&mut on, &choice.label).changed() {
                        toggle(form, &choice.value, on);
                    }
                }
                return;
            }
            let placeholder = component.get("placeholder").and_then(Value::as_str).map_or_else(|| t!("Make a selection").to_owned(), str::to_owned);
            let shown = if chosen.is_empty() { placeholder } else { list.iter().filter(|c| chosen.contains(&c.value)).map(|c| c.label.clone()).collect::<Vec<_>>().join(", ") };
            Frame::new().stroke(Stroke::new(1.0, border)).corner_radius(8).show(ui, |ui| {
                egui::ComboBox::from_id_salt(("form-select", custom)).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
                    for choice in &list {
                        let on = chosen.contains(&choice.value);
                        let response = if max <= 1 { ui.selectable_label(on, &choice.label) } else { ui.checkbox(&mut on.clone(), &choice.label) };
                        if response.clicked() {
                            toggle(form, &choice.value, !on);
                        }
                    }
                });
            });
        }
    }
}
