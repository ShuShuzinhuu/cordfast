//! Discord's expression picker: one popout over the composer with its
//! GIFs, Stickers and Emoji tabs (emoji alone for a reaction). Sizes from
//! the web client's stylesheets: 498 points wide, 486 tall with the tabs
//! (440 without), the 48-point category rail on the left, 48-point emoji
//! cells nine to a row, 32-point section headers that stay on top while
//! their section scrolls, and the 48-point inspector under the list naming
//! the emoji pointed at and its server. The emoji tab opens on Favorites
//! and Frequently Used (Discord's own frecency, `settings-proto/2`), then
//! each server's emoji, then the Unicode categories; a click on a
//! section's name hides it, and Shift keeps the picker open for another.
//! Rows are laid out once per frame and only the visible ones drawn.

use egui::{Align2, Color32, FontId, Frame, Margin, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{Action, App, ExpressionTab, PickerTarget};
use crate::discord::model::{Emoji, Id};
use crate::icons::{self, Icon};
use crate::theme;

const WIDTH: f32 = 498.0;
const HEIGHT_TABS: f32 = 486.0;
const HEIGHT_EMOJI: f32 = 440.0;
const RAIL: f32 = 48.0;
const CELL: f32 = 48.0;
const PER_ROW: usize = 9;
const HEADER: f32 = 32.0;
const SECTION_GAP: f32 = 12.0;
const INSPECTOR: f32 = 48.0;
/// A sticker's preview and the room around it.
const STICKER: f32 = 96.0;
const STICKER_CELL: f32 = 108.0;
const STICKERS_PER_ROW: usize = 4;

/// Discord's Unicode categories, People taking the smileys too.
const UNICODE: [(&str, &str, Icon, &[emojis::Group]); 8] = [
    ("people", n!("People"), Icon::Reaction, &[emojis::Group::SmileysAndEmotion, emojis::Group::PeopleAndBody]),
    ("nature", n!("Nature"), Icon::Nature, &[emojis::Group::AnimalsAndNature]),
    ("food", n!("Food"), Icon::Food, &[emojis::Group::FoodAndDrink]),
    ("activity", n!("Activities"), Icon::Controller, &[emojis::Group::Activities]),
    ("travel", n!("Travel"), Icon::Travel, &[emojis::Group::TravelAndPlaces]),
    ("objects", n!("Objects"), Icon::Objects, &[emojis::Group::Objects]),
    ("symbols", n!("Symbols"), Icon::Heart, &[emojis::Group::Symbols]),
    ("flags", n!("Flags"), Icon::Flag, &[emojis::Group::Flags]),
];

/// What a cell holds.
#[derive(Clone)]
enum Pick {
    /// A server's emoji, and the server.
    Custom(Emoji, Id),
    /// A Unicode emoji (in the chosen skin tone) and its name.
    Unicode(&'static str, &'static str),
    /// A server's sticker: id, name, format, server.
    Sticker(Id, String, u64, Id),
}

/// A section's mark in the rail and beside its name.
#[derive(Clone)]
enum Mark {
    Icon(Icon),
    Guild(Id),
}

struct Section {
    /// What hides it (a server's id, or a category's), and whether it is.
    key: String,
    /// None for search results, which have no header.
    title: Option<String>,
    mark: Mark,
    picks: Vec<Pick>,
    collapsed: bool,
}

/// Where each section starts in the list, and the list's height.
struct Layout {
    tops: Vec<f32>,
    height: f32,
}

fn layout(sections: &[Section], cell: f32, per_row: usize) -> Layout {
    let mut tops = Vec::with_capacity(sections.len());
    let mut y = 0.0;
    for (i, section) in sections.iter().enumerate() {
        tops.push(y);
        if section.title.is_some() {
            y += HEADER;
        }
        if !section.collapsed {
            y += section.picks.len().div_ceil(per_row) as f32 * cell;
            if i + 1 < sections.len() {
                y += SECTION_GAP;
            }
        }
    }
    Layout { tops, height: y + 8.0 }
}

/// A Unicode emoji in the picked skin tone, where it has tones.
fn toned(emoji: &'static emojis::Emoji, tone: u8) -> &'static emojis::Emoji {
    let tone = match tone {
        1 => emojis::SkinTone::Light,
        2 => emojis::SkinTone::MediumLight,
        3 => emojis::SkinTone::Medium,
        4 => emojis::SkinTone::MediumDark,
        5 => emojis::SkinTone::Dark,
        _ => return emoji,
    };
    emoji.with_skin_tone(tone).unwrap_or(emoji)
}

fn unicode(emoji: &'static emojis::Emoji, tone: u8) -> Pick {
    let shown = toned(emoji, tone);
    Pick::Unicode(shown.as_str(), emoji.shortcode().unwrap_or(emoji.name()))
}

/// A frecency key (a custom emoji's id or a Unicode emoji's name) as a cell.
fn emoji_for(app: &App, key: &str, tone: u8) -> Option<Pick> {
    if let Ok(id) = key.parse::<Id>() {
        return app.guilds.iter().find_map(|g| g.emojis.iter().find(|e| e.id == Some(id)).map(|e| Pick::Custom(e.clone(), g.id)));
    }
    emojis::get_by_shortcode(key).or_else(|| emojis::get(key)).map(|e| unicode(e, tone))
}

/// The servers in the picker's order: the open one first, then the rail's.
fn guild_order(app: &App) -> Vec<&crate::discord::model::Guild> {
    let mut guilds: Vec<_> = app.guilds.iter().collect();
    guilds.sort_by_key(|g| Some(g.id) != app.selected_guild);
    guilds
}

fn emoji_sections(app: &App, search: &str) -> Vec<Section> {
    let tone = app.settings.skin_tone;
    let collapsed = |key: &str| app.settings.collapsed_expressions.iter().any(|k| k == key);
    let needle = search.trim().trim_matches(':').to_lowercase();
    if !needle.is_empty() {
        let mut picks: Vec<Pick> = Vec::new();
        for guild in guild_order(app) {
            picks.extend(guild.emojis.iter().filter(|e| e.name.to_lowercase().contains(&needle)).map(|e| Pick::Custom(e.clone(), guild.id)));
        }
        picks.extend(
            emojis::iter()
                .filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
                .filter(|e| e.name().contains(&needle) || e.shortcodes().any(|s| s.contains(&needle)))
                .map(|e| unicode(e, tone)),
        );
        return vec![Section { key: "search".into(), title: None, mark: Mark::Icon(Icon::Search), picks, collapsed: false }];
    }
    let mut sections = Vec::new();
    if let Some(frecency) = &app.frecency {
        let favorites: Vec<Pick> = frecency.favorite_emojis.iter().filter_map(|k| emoji_for(app, k, tone)).collect();
        if !favorites.is_empty() {
            sections.push(Section { key: "favorites".into(), title: Some(crate::i18n::tr(n!("Favorites")).to_owned()), mark: Mark::Icon(Icon::Star), picks: favorites, collapsed: collapsed("favorites") });
        }
        let recent: Vec<Pick> = frecency.emojis.iter().filter_map(|k| emoji_for(app, k, tone)).collect();
        if !recent.is_empty() {
            sections.push(Section { key: "recent".into(), title: Some(crate::i18n::tr(n!("Frequently Used")).to_owned()), mark: Mark::Icon(Icon::Clock), picks: recent, collapsed: collapsed("recent") });
        }
    }
    for guild in guild_order(app).into_iter().filter(|g| !g.emojis.is_empty()) {
        let key = guild.id.to_string();
        let picks = guild.emojis.iter().map(|e| Pick::Custom(e.clone(), guild.id)).collect();
        sections.push(Section { collapsed: collapsed(&key), key, title: Some(guild.name.clone()), mark: Mark::Guild(guild.id), picks });
    }
    for (key, title, icon, groups) in UNICODE {
        let picks = groups
            .iter()
            .flat_map(|g| g.emojis())
            .filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
            .map(|e| unicode(e, tone))
            .collect();
        sections.push(Section { key: key.into(), title: Some(crate::i18n::tr(title).to_owned()), mark: Mark::Icon(icon), picks, collapsed: collapsed(key) });
    }
    sections
}

fn sticker_sections(app: &App, search: &str) -> Vec<Section> {
    let needle = search.trim().to_lowercase();
    let collapsed = |key: &str| app.settings.collapsed_expressions.iter().any(|k| k == key);
    let sticker = |id: Id| {
        app.guilds.iter().find_map(|g| g.stickers.iter().find(|(s, ..)| *s == id).map(|(s, name, format)| Pick::Sticker(*s, name.clone(), *format, g.id)))
    };
    if !needle.is_empty() {
        let picks = guild_order(app)
            .into_iter()
            .flat_map(|g| g.stickers.iter().filter(|(_, name, _)| name.to_lowercase().contains(&needle)).map(move |(id, name, format)| Pick::Sticker(*id, name.clone(), *format, g.id)))
            .collect();
        return vec![Section { key: "search".into(), title: None, mark: Mark::Icon(Icon::Search), picks, collapsed: false }];
    }
    let mut sections = Vec::new();
    if let Some(frecency) = &app.frecency {
        let favorites: Vec<Pick> = frecency.favorite_stickers.iter().filter_map(|id| sticker(*id)).collect();
        if !favorites.is_empty() {
            sections.push(Section { key: "sticker-favorites".into(), title: Some(crate::i18n::tr(n!("Favorites")).to_owned()), mark: Mark::Icon(Icon::Star), picks: favorites, collapsed: collapsed("sticker-favorites") });
        }
        let recent: Vec<Pick> = frecency.stickers.iter().filter_map(|id| sticker(*id)).collect();
        if !recent.is_empty() {
            sections.push(Section { key: "sticker-recent".into(), title: Some(crate::i18n::tr(n!("Frequently Used")).to_owned()), mark: Mark::Icon(Icon::Clock), picks: recent, collapsed: collapsed("sticker-recent") });
        }
    }
    for guild in guild_order(app).into_iter().filter(|g| !g.stickers.is_empty()) {
        let key = format!("stickers:{}", guild.id);
        let picks = guild.stickers.iter().map(|(id, name, format)| Pick::Sticker(*id, name.clone(), *format, guild.id)).collect();
        sections.push(Section { collapsed: collapsed(&key), key, title: Some(guild.name.clone()), mark: Mark::Guild(guild.id), picks });
    }
    sections
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(state) = &app.expressions else { return };
    let (tab, target) = (state.tab, state.target);
    let p = theme::p();
    let tabs = target == PickerTarget::Composer;
    let tab = if tabs { tab } else { ExpressionTab::Emoji };
    let screen = ctx.content_rect();
    let height = if tabs { HEIGHT_TABS } else { HEIGHT_EMOJI };
    let pos = pos2(screen.right() - WIDTH - 16.0, (screen.bottom() - height - 80.0).max(screen.top() + 40.0));
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut chosen: Option<Pick> = None;
    let mut chosen_gif: Option<String> = None;
    let area = egui::Area::new("expression-picker".into()).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        Frame::new()
            .fill(p.sidebar)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(8)
            .shadow(egui::Shadow { offset: [0, 8], blur: 16, spread: 0, color: Color32::from_black_alpha(60) })
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
                let mut content = rect;
                if tabs {
                    // Discord's tab bar: 16 from the top, 30 tall, 12 over the tab.
                    let mut x = rect.left() + 16.0;
                    for (each, label) in [(ExpressionTab::Gifs, t!("GIFs")), (ExpressionTab::Stickers, t!("Stickers")), (ExpressionTab::Emoji, t!("Emoji"))] {
                        let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::strong(14.0), p.text);
                        let button = Rect::from_min_size(pos2(x, rect.top() + 16.0), vec2(galley.size().x + 24.0, 30.0));
                        let response = ui.interact(button, ui.id().with(("expression-tab", each as u8)), Sense::click());
                        let active = each == tab;
                        if active || response.hovered() {
                            ui.painter().rect_filled(button, 4.0, if active { p.selected } else { p.hover });
                        }
                        let color = if active || response.hovered() { p.strong } else { p.text };
                        ui.painter().galley(button.center() - galley.size() / 2.0, galley, color);
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                            && let Some(state) = &mut app.expressions
                        {
                            state.tab = each;
                            state.jump = None;
                        }
                        x = button.right() + 8.0;
                    }
                    content = Rect::from_min_max(pos2(rect.left(), rect.top() + 58.0), rect.max);
                }
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content).id_salt(("expression-content", tab as u8)));
                match tab {
                    ExpressionTab::Emoji | ExpressionTab::Stickers => chosen = list_tab(app, &mut child, content, tab, tabs),
                    ExpressionTab::Gifs => chosen_gif = gif_tab(app, &mut child, content),
                }
            });
    });
    // Over the message hover bars, which take their layer after it.
    ctx.move_to_top(area.response.layer_id);
    if ctx.input(|i| i.pointer.any_pressed())
        && let Some(pointer) = ctx.pointer_interact_pos()
        && !area.response.rect.contains(pointer)
        && !egui::Popup::is_any_open(ctx)
    {
        close = true;
        app.expressions_closed = Some((ctx.input(|i| i.time), tab));
    }
    let keep_open = ctx.input(|i| i.modifiers.shift);
    if let Some(pick) = chosen {
        use_pick(app, target, pick);
        if !keep_open || matches!(target, PickerTarget::Reaction { .. }) {
            close = true;
        }
    }
    if let Some(url) = chosen_gif {
        app.composer = url;
        app.actions.push(Action::Send);
        close = true;
    }
    if close {
        app.expressions = None;
    }
}

fn use_pick(app: &mut App, target: PickerTarget, pick: Pick) {
    match (&pick, target) {
        (Pick::Sticker(id, ..), _) => {
            let Some(channel) = app.selected_channel else { return };
            app.commands.send(crate::backend::Command::Api {
                key: "sticker".into(),
                method: "POST",
                path: format!("/channels/{channel}/messages"),
                body: Some(serde_json::json!({
                    "sticker_ids": [id.to_string()],
                    "nonce": crate::discord::model::now_snowflake().to_string(),
                })),
            });
            if let Some(frecency) = &mut app.frecency {
                frecency.used_sticker(*id);
            }
        }
        (Pick::Custom(emoji, _), PickerTarget::Composer) => {
            super::emoji::insert_custom(app, emoji);
            app.focus_composer = true;
        }
        (Pick::Unicode(text, _), PickerTarget::Composer) => {
            app.composer.push_str(text);
            app.focus_composer = true;
        }
        (Pick::Custom(emoji, _), PickerTarget::Reaction { channel, message }) => {
            app.actions.push(Action::React { channel, message, emoji: emoji.api_name(), add: true });
        }
        (Pick::Unicode(text, _), PickerTarget::Reaction { channel, message }) => {
            app.actions.push(Action::React { channel, message, emoji: (*text).to_owned(), add: true });
        }
    }
    let key = match &pick {
        Pick::Custom(emoji, _) => emoji.id.map(|id| id.to_string()),
        Pick::Unicode(_, name) => Some((*name).to_owned()),
        Pick::Sticker(..) => None,
    };
    if let (Some(key), Some(frecency)) = (key, &mut app.frecency) {
        frecency.used_emoji(key);
    }
}

/// Discord's search bar: 30 points, the input colour, the magnifier on the
/// right (a cross to clear once something is typed).
fn search_bar(ui: &mut egui::Ui, rect: Rect, text: &mut String, hint: &str, id: &str) -> bool {
    let p = theme::p();
    ui.painter().rect_filled(rect, 4.0, p.rail);
    let edit = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(rect.right() - 32.0, rect.bottom()));
    let response = ui.put(
        edit,
        egui::TextEdit::singleline(text)
            .id(egui::Id::new(id))
            .frame(Frame::NONE)
            .font(FontId::proportional(16.0))
            .text_color(p.text)
            .hint_text(egui::RichText::new(hint).color(p.faint))
            .vertical_align(egui::Align::Center),
    );
    if !response.has_focus() && !ui.ctx().egui_wants_keyboard_input() {
        response.request_focus();
    }
    let icon = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(18.0));
    if text.is_empty() {
        icons::paint(ui, icon, Icon::Search, p.muted);
    } else {
        let clear = ui.interact(icon.expand(4.0), ui.id().with((id, "clear")), Sense::click());
        icons::paint(ui, icon, Icon::Close, if clear.hovered() { p.strong } else { p.muted });
        if clear.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            text.clear();
        }
    }
    response.changed()
}

/// The emoji or sticker tab: the search (and the skin tone), the rail, the
/// list and the inspector. Returns what was clicked.
fn list_tab(app: &mut App, ui: &mut egui::Ui, rect: Rect, tab: ExpressionTab, tabs: bool) -> Option<Pick> {
    let p = theme::p();
    let stickers = tab == ExpressionTab::Stickers;
    let (cell, per_row) = if stickers { (STICKER_CELL, STICKERS_PER_ROW) } else { (CELL, PER_ROW) };
    // The header: the search, and for emoji the skin tone beside it.
    let top = if tabs { rect.top() } else { rect.top() + 16.0 };
    let tone_width = if stickers { 0.0 } else { 40.0 };
    let bar = Rect::from_min_max(pos2(rect.left() + 16.0, top), pos2(rect.right() - 16.0 - tone_width, top + 30.0));
    let Some(state) = app.expressions.as_mut() else { return None };
    let (search, hint, id) = if stickers {
        (&mut state.sticker_search, t!("Find the perfect sticker"), "sticker-search")
    } else if matches!(state.target, PickerTarget::Reaction { .. }) {
        (&mut state.emoji_search, t!("Find the perfect reaction"), "emoji-search")
    } else {
        (&mut state.emoji_search, t!("Find the perfect emoji"), "emoji-search")
    };
    if search_bar(ui, bar, search, hint, id) {
        state.jump = Some(0.0);
    }
    let search = search.clone();
    let jump = state.jump.take();
    if !stickers {
        skin_tone(app, ui, Rect::from_center_size(pos2(rect.right() - 16.0 - 18.0, bar.center().y), Vec2::splat(36.0)));
    }
    let header_bottom = bar.bottom() + 12.0;
    ui.painter().hline(rect.x_range(), header_bottom, Stroke::new(1.0, p.border));

    let sections = if stickers { sticker_sections(app, &search) } else { emoji_sections(app, &search) };
    let layout = layout(&sections, cell, per_row);
    let list = Rect::from_min_max(pos2(rect.left() + RAIL + 8.0, header_bottom + 1.0), pos2(rect.right(), rect.bottom() - INSPECTOR));
    let rail = Rect::from_min_max(pos2(rect.left(), header_bottom + 1.0), pos2(rect.left() + RAIL, rect.bottom()));
    let inspector = Rect::from_min_max(pos2(rect.left() + RAIL, rect.bottom() - INSPECTOR), rect.max);

    let mut hovered: Option<Pick> = None;
    let mut chosen = None;
    let mut toggled: Option<String> = None;
    let mut viewport_top = 0.0;
    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list).id_salt(("expression-list", tab as u8)));
    let mut scroll = egui::ScrollArea::vertical().id_salt(("expression-scroll", tab as u8)).auto_shrink(false);
    if let Some(y) = jump {
        scroll = scroll.vertical_scroll_offset(y);
    }
    scroll.show_viewport(&mut list_ui, |ui, viewport| {
        ui.set_height(layout.height);
        viewport_top = viewport.min.y;
        let origin = ui.max_rect().min;
        let width = ui.max_rect().width();
        let ppp = ui.ctx().pixels_per_point();
        for (i, section) in sections.iter().enumerate() {
            let mut y = layout.tops[i];
            if section.title.is_some() {
                let header = Rect::from_min_size(origin + vec2(0.0, y), vec2(width, HEADER));
                if header.top() < origin.y + viewport.max.y && header.bottom() > origin.y + viewport.min.y && section_header(app, ui, header, section, ("expression-header", i)) {
                    toggled = Some(section.key.clone());
                }
                y += HEADER;
            }
            if section.collapsed {
                continue;
            }
            let rows = section.picks.len().div_ceil(per_row);
            let first = (((viewport.min.y - y) / cell).floor().max(0.0)) as usize;
            let last = (((viewport.max.y - y) / cell).ceil().max(0.0) as usize).min(rows);
            for row in first..last {
                for col in 0..per_row {
                    let Some(pick) = section.picks.get(row * per_row + col) else { break };
                    let at = Rect::from_min_size(origin + vec2(col as f32 * cell, y + row as f32 * cell), Vec2::splat(cell));
                    let response = ui.interact(at, ui.id().with(("expression-cell", i, row, col)), Sense::click());
                    if response.hovered() {
                        hovered = Some(pick.clone());
                        ui.painter().rect_filled(if stickers { at.shrink(6.0) } else { at }, 4.0, p.hover);
                    }
                    paint_pick(app, ui, pick, if stickers { Rect::from_center_size(at.center(), Vec2::splat(STICKER)) } else { at.shrink(4.0) }, ppp);
                    if response.clicked() {
                        chosen = Some(pick.clone());
                    }
                }
            }
        }
        if sections.iter().all(|s| s.picks.is_empty()) {
            let message = if stickers && search.is_empty() { t!("None of your servers have stickers.") } else { t!("No results found") };
            ui.painter().text(origin + vec2(width / 2.0, 60.0), Align2::CENTER_CENTER, message, FontId::proportional(15.0), p.muted);
        }
        // The section under the top stays named there, as Discord's
        // sticky headers.
        if let Some(i) = layout.tops.iter().rposition(|t| *t <= viewport.min.y)
            && sections[i].title.is_some()
            && layout.tops[i] < viewport.min.y
        {
            let header = Rect::from_min_size(origin + vec2(0.0, viewport.min.y), vec2(width, HEADER));
            ui.painter().rect_filled(header, 0.0, p.sidebar);
            if section_header(app, ui, header, &sections[i], ("expression-sticky", i)) {
                toggled = Some(sections[i].key.clone());
            }
        }
    });
    if let Some(key) = toggled {
        let list = &mut app.settings.collapsed_expressions;
        if let Some(at) = list.iter().position(|k| *k == key) {
            list.remove(at);
        } else {
            list.push(key);
        }
        app.actions.push(Action::SettingsChanged);
    }

    // The rail: each section's mark, the one in view lit; a click scrolls
    // to it.
    let active = layout.tops.iter().rposition(|t| *t <= viewport_top + 1.0).unwrap_or(0);
    if search.trim().is_empty() {
        let mut rail_ui = ui.new_child(egui::UiBuilder::new().max_rect(rail).id_salt(("expression-rail", tab as u8)));
        egui::ScrollArea::vertical().id_salt(("expression-rail-scroll", tab as u8)).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut rail_ui, |ui| {
            ui.set_width(RAIL);
            ui.add_space(8.0);
            let mut was_guild = false;
            for (i, section) in sections.iter().enumerate() {
                let is_guild = matches!(section.mark, Mark::Guild(_));
                if was_guild && !is_guild {
                    // Discord's line between the servers and the categories.
                    let (line, _) = ui.allocate_exact_size(vec2(RAIL, 25.0), Sense::hover());
                    ui.painter().hline((line.left() + 12.0)..=(line.right() - 12.0), line.center().y, Stroke::new(1.0, p.border));
                }
                was_guild = is_guild;
                // Servers 32 with 8 under them, categories 24 in a 4 padding
                // with 2 under them (the stylesheet's constants).
                let (slot, response) = ui.allocate_exact_size(vec2(RAIL, 32.0 + if is_guild { 8.0 } else { 2.0 }), Sense::click());
                let mark = Rect::from_min_size(pos2(slot.center().x - 16.0, slot.top()), Vec2::splat(32.0));
                let lit = i == active;
                match &section.mark {
                    Mark::Guild(guild) => {
                        if lit {
                            // Discord's pill on the rail's edge.
                            ui.painter().rect_filled(Rect::from_min_size(pos2(slot.left(), mark.top() + 8.0), vec2(4.0, 16.0)), 2.0, p.strong);
                        }
                        let (url, initials) = app.guild(*guild).map(|g| (g.icon_url(), super::initials(&g.name))).unwrap_or_default();
                        let radius = if lit || response.hovered() { 10.0 } else { 16.0 };
                        app.images.circle(ui, mark, url.as_deref(), &initials, radius);
                    }
                    Mark::Icon(icon) => {
                        if lit || response.hovered() {
                            ui.painter().rect_filled(mark, 4.0, if lit { p.rail } else { p.hover });
                        }
                        icons::paint(ui, mark.shrink(4.0), *icon, if lit || response.hovered() { p.strong } else { p.muted });
                    }
                }
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let response = match &section.title {
                    Some(title) => response.on_hover_text(title),
                    None => response,
                };
                if response.clicked()
                    && let Some(state) = &mut app.expressions
                {
                    state.jump = Some(layout.tops[i]);
                    if section.collapsed {
                        toggled_open(app, &section.key);
                    }
                }
            }
        });
    }

    // The inspector: the pick pointed at, or the first one.
    let shown = hovered.or_else(|| sections.iter().find(|s| !s.collapsed).and_then(|s| s.picks.first().cloned()));
    ui.painter().hline(inspector.x_range(), inspector.top(), Stroke::new(1.0, p.border));
    if let Some(pick) = shown {
        let graphic = Rect::from_center_size(pos2(inspector.left() + 16.0 + 14.0, inspector.center().y), Vec2::splat(28.0));
        paint_pick(app, ui, &pick, graphic, ui.ctx().pixels_per_point());
        let (title, guild) = match &pick {
            Pick::Custom(emoji, guild) => (format!(":{}:", emoji.name), Some(*guild)),
            Pick::Unicode(_, name) => (format!(":{name}:"), None),
            Pick::Sticker(_, name, _, guild) => (name.clone(), Some(*guild)),
        };
        let text_left = graphic.right() + 8.0;
        let text_right = inspector.right() - if guild.is_some() { 56.0 } else { 16.0 };
        let from = guild.and_then(|g| app.guild(g)).map(|g| t!("from {guildName}", guildName = g.name));
        match &from {
            Some(from) => {
                super::paint_truncated(ui, Rect::from_min_max(pos2(text_left, inspector.top() + 6.0), pos2(text_right, inspector.center().y + 2.0)), &title, theme::strong(16.0), p.strong);
                super::paint_truncated(ui, Rect::from_min_max(pos2(text_left, inspector.center().y), pos2(text_right, inspector.bottom() - 6.0)), from, FontId::proportional(13.0), p.muted);
            }
            None => super::paint_truncated(ui, Rect::from_min_max(pos2(text_left, inspector.top()), pos2(text_right, inspector.bottom())), &title, theme::strong(16.0), p.strong),
        }
        if let Some(guild) = guild.and_then(|g| app.guild(g)) {
            let (url, initials) = (guild.icon_url(), super::initials(&guild.name));
            let icon = Rect::from_center_size(pos2(inspector.right() - 16.0 - 16.0, inspector.center().y), Vec2::splat(32.0));
            app.images.circle(ui, icon, url.as_deref(), &initials, 16.0);
        }
    }
    chosen
}

fn toggled_open(app: &mut App, key: &str) {
    app.settings.collapsed_expressions.retain(|k| k != key);
    app.actions.push(Action::SettingsChanged);
}

/// A section's name, with its mark before it and the chevron that shows
/// it is open or hidden. True when clicked.
fn section_header(app: &mut App, ui: &mut egui::Ui, rect: Rect, section: &Section, id: (&str, usize)) -> bool {
    let p = theme::p();
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    let color = if response.hovered() { p.strong } else { p.muted };
    let mark = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), Vec2::splat(16.0));
    match &section.mark {
        Mark::Guild(guild) => {
            let (url, initials) = app.guild(*guild).map(|g| (g.icon_url(), super::initials(&g.name))).unwrap_or_default();
            app.images.circle(ui, mark, url.as_deref(), &initials, 4.0);
        }
        Mark::Icon(icon) => icons::paint(ui, mark, *icon, color),
    }
    let title = section.title.clone().unwrap_or_default().to_uppercase();
    let width = ui.painter().layout_no_wrap(title.clone(), theme::strong(12.0), color).size().x.min(rect.width() - 60.0);
    let left = mark.right() + 8.0;
    super::paint_truncated(ui, Rect::from_min_max(pos2(left, rect.top()), pos2(left + width + 1.0, rect.bottom())), &title, theme::strong(12.0), color);
    let chevron = Rect::from_center_size(pos2(left + width + 12.0, rect.center().y), Vec2::splat(14.0));
    if section.collapsed {
        icons::paint_chevron_right(ui, chevron, color);
    } else {
        icons::paint(ui, chevron, Icon::ChevronDown, color);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn paint_pick(app: &mut App, ui: &egui::Ui, pick: &Pick, rect: Rect, ppp: f32) {
    match pick {
        Pick::Custom(emoji, _) => {
            // Still until the pointer is on it, as Discord's picker: a page
            // of animated emoji would hold many frames for nothing.
            if let Some(id) = emoji.id
                && let Some((texture, size)) = app.images.get(&crate::discord::model::emoji_url(id, emoji.animated && ui.rect_contains_pointer(rect), 48), (rect.width() * ppp) as u32)
            {
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                egui::Image::from_texture((texture, size * scale)).paint_at(ui, Rect::from_center_size(rect.center(), size * scale));
            }
        }
        Pick::Unicode(text, _) => fastframe_emoji::paint_cluster(ui, text, rect),
        Pick::Sticker(id, _, format, _) => {
            let mut url = crate::discord::model::sticker_url(*id, *format);
            if !ui.rect_contains_pointer(rect) {
                url.push_str(crate::backend::STILL);
            }
            if let Some((texture, size)) = app.images.get(&url, (rect.width() * ppp) as u32) {
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                egui::Image::from_texture((texture, size * scale)).paint_at(ui, Rect::from_center_size(rect.center(), size * scale));
            }
        }
    }
}

/// Discord's skin tone picker beside the emoji search: the hand in the
/// chosen tone; a click lists the six to pick from.
fn skin_tone(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let p = theme::p();
    let hand = emojis::get("👏").expect("the clapping hands");
    let response = ui.interact(rect, ui.id().with("skin-tone"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    fastframe_emoji::paint_cluster(ui, toned(hand, app.settings.skin_tone).as_str(), rect.shrink(7.0));
    let response = response.on_hover_text(t!("Select emoji skin tone"));
    egui::Popup::menu(&response).frame(Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(4).inner_margin(Margin::same(0))).show(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for tone in 0..=5u8 {
            let (cell, response) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(cell, 4.0, p.hover);
            }
            fastframe_emoji::paint_cluster(ui, toned(hand, tone).as_str(), cell.shrink(7.0));
            if response.clicked() {
                app.settings.skin_tone = tone;
                app.actions.push(Action::SettingsChanged);
                ui.close();
            }
        }
    });
}

/// The GIF tab: the search; with nothing typed, Discord's front page
/// (Trending GIFs and its categories, each a tile with its name); then the
/// GIFs in two columns. A click sends the GIF at once.
fn gif_tab(app: &mut App, ui: &mut egui::Ui, rect: Rect) -> Option<String> {
    let p = theme::p();
    let now = ui.input(|i| i.time);
    let locale = crate::i18n::current().discord_tag();
    let bar = Rect::from_min_max(pos2(rect.left() + 16.0, rect.top()), pos2(rect.right() - 16.0, rect.top() + 30.0));
    let state = app.expressions.as_mut()?;
    let picker = &mut state.gifs;
    if search_bar(ui, bar, &mut picker.query, t!("Search Tenor"), "gif-search") {
        picker.typed_at = Some(now);
        picker.trending_open = false;
    }
    // What the page should show: the front page, the trending GIFs, or a
    // search once typing pauses.
    let wanted = if picker.trending_open {
        Some(String::new())
    } else if picker.query.trim().is_empty() {
        None
    } else if picker.typed_at.is_none_or(|t| now - t > 0.35) {
        Some(picker.query.trim().to_owned())
    } else {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        picker.asked.clone()
    };
    let mut commands = Vec::new();
    if wanted.is_none() && !picker.asked_trending {
        picker.asked_trending = true;
        commands.push(crate::admin::get("gif-trending", format!("/gifs/trending?media_format=mp4&locale={locale}")));
    }
    if wanted != picker.asked {
        picker.asked = wanted.clone();
        picker.results = None;
        match &wanted {
            Some(query) if query.is_empty() => commands.push(crate::admin::get("gifs", format!("/gifs/trending-gifs?media_format=mp4&locale={locale}&limit=50"))),
            Some(query) => {
                let encoded: String = query.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
                commands.push(crate::admin::get("gifs", format!("/gifs/search?q={encoded}&media_format=mp4&locale={locale}&limit=50")));
            }
            None => {}
        }
    }
    let trending = picker.trending.clone();
    let results = picker.results.clone();
    let showing_front = wanted.is_none();
    for command in commands {
        app.commands.send(command);
    }
    let body = Rect::from_min_max(pos2(rect.left() + 16.0, bar.bottom() + 12.0), pos2(rect.right() - 8.0, rect.bottom() - 8.0));
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("gif-body"));
    let mut chosen = None;
    let mut open_category: Option<String> = None;
    let mut open_trending = false;
    egui::ScrollArea::vertical().id_salt(("gif-scroll", showing_front)).auto_shrink(false).show(&mut body_ui, |ui| {
        let width = ui.available_width() - 8.0;
        let column = (width - 8.0) / 2.0;
        if showing_front {
            let Some((categories, preview)) = trending else {
                ui.vertical_centered(|ui| ui.spinner());
                return;
            };
            // Discord's tiles: Trending GIFs, then each category.
            let tiles = std::iter::once((t!("Trending GIFs").to_owned(), preview.unwrap_or_default(), true)).chain(categories.into_iter().map(|(name, src)| (name, src, false)));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for (name, src, is_trending) in tiles {
                    let (tile, response) = ui.allocate_exact_size(vec2(column, 110.0), Sense::click());
                    ui.painter().rect_filled(tile, 4.0, p.rail);
                    if !src.is_empty()
                        && let Some((texture, size)) = app.images.get(&src, (column * 1.5) as u32)
                    {
                        // Cover the tile, cropped to it.
                        let scale = (tile.width() / size.x).max(tile.height() / size.y);
                        let shown = size * scale;
                        let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(tile.width() / shown.x, tile.height() / shown.y));
                        egui::Image::from_texture((texture, tile.size())).uv(uv).corner_radius(4.0).paint_at(ui, tile);
                    }
                    let shade = if response.hovered() { 90 } else { 140 };
                    ui.painter().rect_filled(tile, 4.0, Color32::from_black_alpha(shade));
                    ui.painter().text(tile.center(), Align2::CENTER_CENTER, &name, theme::strong(16.0), Color32::WHITE);
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        if is_trending {
                            open_trending = true;
                        } else {
                            open_category = Some(name);
                        }
                    }
                }
            });
            return;
        }
        let Some(results) = results else {
            ui.vertical_centered(|ui| ui.spinner());
            return;
        };
        if results.is_empty() {
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(t!("No results found")).color(p.muted)));
            return;
        }
        // Two columns, each GIF going under the shorter one.
        let origin = ui.cursor().min;
        let mut heights = [0.0f32; 2];
        for gif in &results {
            let col = if heights[0] <= heights[1] { 0 } else { 1 };
            let height = (column * gif.height.max(1) as f32 / gif.width.max(1) as f32).clamp(60.0, 320.0);
            let at = Rect::from_min_size(origin + vec2(col as f32 * (column + 8.0), heights[col]), vec2(column, height));
            heights[col] += height + 8.0;
            let response = ui.interact(at, ui.id().with(("gif", &gif.url)), Sense::click());
            if ui.is_rect_visible(at) {
                match app.images.get(&gif.preview, (column * 1.5) as u32) {
                    Some((texture, _)) => egui::Image::from_texture((texture, at.size())).corner_radius(4.0).paint_at(ui, at),
                    None => {
                        ui.painter().rect_filled(at, 4.0, p.rail);
                    }
                }
            }
            if response.hovered() {
                ui.painter().rect_stroke(at, 4.0, Stroke::new(2.0, p.accent), egui::StrokeKind::Inside);
            }
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                chosen = Some(gif.url.clone());
            }
        }
        ui.allocate_space(vec2(width, heights[0].max(heights[1])));
    });
    if let Some(state) = &mut app.expressions {
        if let Some(name) = open_category {
            state.gifs.query = name;
            state.gifs.typed_at = None;
        }
        if open_trending {
            state.gifs.trending_open = true;
        }
    }
    chosen
}
