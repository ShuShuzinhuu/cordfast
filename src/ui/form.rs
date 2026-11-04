//! Discord's settings vocabulary, shared by the user and server settings so
//! both read as one product: the settings layer (a side column of sections
//! growing from 218 points, the page at most 740 wide, the round close
//! button with ESC under it), titles, field labels, notes, dividers,
//! inputs, switches, radio cards, selects, buttons and the bar that holds
//! unsaved changes. Sizes follow Discord's current client.

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, Vec2, pos2, vec2};

use crate::icons::{self, Icon};
use crate::theme;

/// The page's widest, as Discord's content column.
pub const CONTENT: f32 = 740.0;
/// The side column's sections.
const NAV: f32 = 218.0;

/// The settings layer. `nav` fills the side column, `page` the content,
/// scrolled; the content keeps `bottom` points free below it (for the
/// unsaved changes bar). True when the close button was clicked.
pub fn layer(ui: &mut egui::Ui, id: &str, nav: impl FnOnce(&mut egui::Ui), page: impl FnOnce(&mut egui::Ui)) -> bool {
    layer_with(ui, id, &mut (), |ui, ()| nav(ui), |ui, ()| page(ui))
}

/// The settings layer, both sides drawing from the same `state`.
pub fn layer_with<T>(ui: &mut egui::Ui, id: &str, state: &mut T, nav: impl FnOnce(&mut egui::Ui, &mut T), page: impl FnOnce(&mut egui::Ui, &mut T)) -> bool {
    let p = theme::p();
    // Discord's flex: the column grows from 218 and the page from 800, each
    // taking half of what is left.
    let free = (ui.available_width() - NAV - 800.0).max(0.0);
    let side = NAV + 26.0 + free / 2.0;
    egui::Panel::left(egui::Id::new((id, "nav")))
        .exact_size(side)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().fill(p.sidebar))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let column = Rect::from_min_max(pos2(full.right() - NAV - 6.0, full.top()), pos2(full.right() - 6.0, full.bottom()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                ScrollArea::vertical().id_salt((id, "nav-scroll")).auto_shrink(false).show(ui, |ui| {
                    ui.add_space(60.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.set_width(NAV);
                    nav(ui, state);
                    ui.add_space(20.0);
                });
            });
        });
    let mut close = false;
    egui::CentralPanel::default().frame(Frame::new().fill(p.chat)).show(ui, |ui| {
        let full = ui.max_rect();
        let width = (full.width() - 40.0 - 40.0 - 76.0).clamp(240.0, CONTENT);
        // The close button sits beside the page, as Discord's tools column.
        let tools = Rect::from_min_size(pos2(full.left() + 40.0 + width + 40.0, full.top() + 60.0), vec2(36.0, 52.0));
        close = close_button(ui, tools);
        let scroll = Rect::from_min_max(full.min, pos2(tools.left() - 16.0, full.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(scroll), |ui| {
            ScrollArea::vertical().id_salt((id, "page-scroll")).auto_shrink(false).show(ui, |ui| {
                Frame::new().inner_margin(Margin { left: 40, right: 0, top: 60, bottom: 80 }).show(ui, |ui| {
                    ui.set_width(width);
                    page(ui, state);
                });
            });
        });
    });
    close
}

/// Discord's close button: a ringed X with ESC under it.
fn close_button(ui: &mut egui::Ui, rect: Rect) -> bool {
    let p = theme::p();
    let circle = Rect::from_min_size(rect.min, Vec2::splat(36.0));
    let response = ui.interact(circle, ui.id().with("settings-close"), Sense::click());
    let color = if response.hovered() { p.strong } else { p.muted };
    if response.hovered() {
        ui.painter().circle_filled(circle.center(), 18.0, p.hover);
    }
    ui.painter().circle_stroke(circle.center(), 17.0, Stroke::new(2.0, color));
    icons::paint(ui, Rect::from_center_size(circle.center(), Vec2::splat(18.0)), Icon::Close, color);
    ui.painter().text(pos2(circle.center().x, circle.bottom() + 8.0), Align2::CENTER_TOP, "ESC", theme::strong(13.0), color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A group's name in the side column, as "MODERATION".
pub fn nav_header(ui: &mut egui::Ui, text: &str) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 10.0, rect.top() + 6.0), pos2(rect.right() - 10.0, rect.bottom())), &text.to_uppercase(), theme::strong(12.0), p.muted);
}

/// A section in the side column: 32 points, the chosen one filled. One
/// the person may not open shows faded and does nothing.
pub fn nav_item(ui: &mut egui::Ui, text: &str, selected: bool, enabled: bool) -> egui::Response {
    let p = theme::p();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), sense);
    if selected || (enabled && response.hovered()) {
        ui.painter().rect_filled(rect, 4.0, if selected { p.selected } else { p.hover });
    }
    let color = if !enabled { p.faint } else if selected || response.hovered() { p.strong } else { p.muted };
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 10.0, rect.top()), pos2(rect.right() - 10.0, rect.bottom())), text, FontId::proportional(16.0), color);
    response
}

/// The line between groups in the side column.
pub fn nav_separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 17.0), Sense::hover());
    ui.painter().hline(rect.x_range().shrink(10.0), rect.center().y, Stroke::new(1.0, theme::p().selected));
}

/// The page's title.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::strong(20.0)).color(theme::p().strong));
    ui.add_space(20.0);
}

/// A field's name, as Discord's: small capitals over the control.
pub fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(theme::strong(12.0)).color(theme::p().muted));
    ui.add_space(8.0);
}

/// A note under a field or a heading.
pub fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(14.0).color(theme::p().muted)).wrap());
}

/// The line between a page's parts, with room on both sides.
pub fn divider(ui: &mut egui::Ui) {
    ui.add_space(28.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, theme::p().selected));
    ui.add_space(28.0);
}

/// A text box: 40 points, the input colour, 16-point text.
pub fn input(ui: &mut egui::Ui, value: &mut String, hint: &str, width: f32) -> egui::Response {
    let p = theme::p();
    Frame::new()
        .fill(p.rail)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(10, 10))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .frame(Frame::NONE)
                    .font(FontId::proportional(16.0))
                    .text_color(p.text)
                    .hint_text(RichText::new(hint).color(p.faint))
                    .desired_width(width - 22.0),
            )
        })
        .inner
}

/// A text area with Discord's count of what is left in its corner.
pub fn textarea(ui: &mut egui::Ui, value: &mut String, hint: &str, rows: usize, limit: usize) -> egui::Response {
    let p = theme::p();
    let inner = Frame::new()
        .fill(p.rail)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(Margin { left: 10, right: 10, top: 10, bottom: 24 })
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(value)
                    .frame(Frame::NONE)
                    .font(FontId::proportional(16.0))
                    .text_color(p.text)
                    .hint_text(RichText::new(hint).color(p.faint))
                    .char_limit(limit)
                    .desired_rows(rows)
                    .desired_width(ui.available_width()),
            )
        });
    let left = limit.saturating_sub(value.chars().count());
    ui.painter().text(inner.response.rect.right_bottom() - vec2(10.0, 6.0), Align2::RIGHT_BOTTOM, left.to_string(), FontId::proportional(12.0), p.faint);
    inner.inner
}

/// Discord's switch: green with a check when on, grey with an x when off,
/// the knob sliding between them.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = theme::p();
    let (rect, mut response) = ui.allocate_exact_size(vec2(40.0, 24.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.15);
    let off = Color32::from_rgb(0x80, 0x84, 0x8e);
    let track = lerp_color(off, p.green, t);
    // A locked switch (Vencord's required plugins) shows dimmed.
    let track = if ui.is_enabled() { track } else { track.gamma_multiply(0.45) };
    ui.painter().rect_filled(rect, 12.0, track);
    let knob = pos2(rect.left() + 12.0 + 16.0 * t, rect.center().y);
    ui.painter().circle_filled(knob, 9.0, Color32::WHITE);
    let mark = Rect::from_center_size(knob, Vec2::splat(10.0));
    if t > 0.5 {
        let points = vec![mark.left_center() + vec2(0.5, 0.5), mark.center_bottom() + vec2(-1.0, -2.0), mark.right_top() + vec2(-0.5, 2.0)];
        ui.painter().add(egui::Shape::line(points, Stroke::new(2.0, track)));
    } else {
        let m = mark.shrink(1.5);
        ui.painter().line_segment([m.left_top(), m.right_bottom()], Stroke::new(2.0, track));
        ui.painter().line_segment([m.right_top(), m.left_bottom()], Stroke::new(2.0, track));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A switch with its title and note, and the line under them, as
/// Discord's settings rows. True when it changed.
pub fn switch_row(ui: &mut egui::Ui, on: &mut bool, title: &str, note_text: &str) -> bool {
    let p = theme::p();
    let mut changed = false;
    ui.horizontal(|ui| {
        let width = ui.available_width();
        ui.allocate_ui_with_layout(vec2(width - 56.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(title).size(16.0).color(p.strong)).wrap());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = switch(ui, on).changed();
        });
    });
    if !note_text.is_empty() {
        ui.add_space(6.0);
        note(ui, note_text);
    }
    ui.add_space(18.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.selected));
    ui.add_space(18.0);
    changed
}

/// Discord's radio group: full-width cards, a ring that fills when chosen,
/// the title and, under it, what the choice means. Returns the one
/// clicked.
pub fn radio_cards<T: Copy + PartialEq>(ui: &mut egui::Ui, current: T, options: &[(T, &str, &str)]) -> Option<T> {
    let mut picked = None;
    for (value, title, detail) in options {
        let chosen = *value == current;
        if radio_card(ui, chosen, title, detail).clicked() && !chosen {
            picked = Some(*value);
        }
    }
    picked
}

/// One card of a radio group: the ring, the title and its detail under it.
/// The response's rect is the card, for anything the caller draws on its
/// right.
pub fn radio_card(ui: &mut egui::Ui, chosen: bool, title: &str, detail: &str) -> egui::Response {
    let p = theme::p();
    let galley_width = ui.available_width() - 52.0;
    let title_galley = ui.painter().layout(title.to_string(), FontId::proportional(16.0), p.strong, galley_width);
    let detail_galley = (!detail.is_empty()).then(|| ui.painter().layout(detail.to_string(), FontId::proportional(14.0), p.muted, galley_width));
    let height = 20.0 + title_galley.size().y + detail_galley.as_ref().map_or(0.0, |g| g.size().y + 2.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height.max(44.0)), Sense::click());
    let fill = if chosen { p.selected } else if response.hovered() { p.hover } else { p.sidebar };
    ui.painter().rect_filled(rect, 8.0, fill);
    let ring = pos2(rect.left() + 22.0, rect.top() + 10.0 + title_galley.size().y / 2.0);
    ui.painter().circle_stroke(ring, 9.0, Stroke::new(2.0, if chosen { p.strong } else { p.muted }));
    if chosen {
        ui.painter().circle_filled(ring, 5.0, p.strong);
    }
    let x = rect.left() + 44.0;
    let title_height = title_galley.size().y;
    ui.painter().galley(pos2(x, rect.top() + 10.0), title_galley, p.strong);
    if let Some(detail) = detail_galley {
        ui.painter().galley(pos2(x, rect.top() + 12.0 + title_height), detail, p.muted);
    }
    ui.add_space(8.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Discord's select: an input-coloured box with a chevron that opens the
/// choices under it. Returns the index picked.
pub fn select(ui: &mut egui::Ui, id: &str, shown: &str, options: &[String], width: f32) -> Option<usize> {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    ui.painter().rect(rect, 8.0, p.rail, Stroke::new(1.0, if open { p.accent } else { p.border }), egui::StrokeKind::Inside);
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 36.0, rect.bottom())), shown, FontId::proportional(16.0), p.text);
    icons::paint(ui, Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(18.0)), Icon::ChevronDown, if response.hovered() || open { p.strong } else { p.muted });
    let mut picked = None;
    let _ = id;
    egui::Popup::menu(&response.on_hover_cursor(egui::CursorIcon::PointingHand))
        .width(width)
        .gap(4.0)
        .frame(Frame::new().fill(p.rail).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(6)))
        .show(|ui| {
            ui.set_width(width - 12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (i, option) in options.iter().enumerate() {
                    let current = option == shown;
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
                    if current || response.hovered() {
                        ui.painter().rect_filled(row, 4.0, if response.hovered() { p.hover } else { p.selected });
                    }
                    super::paint_truncated(ui, Rect::from_min_max(pos2(row.left() + 10.0, row.top()), pos2(row.right() - 30.0, row.bottom())), option, FontId::proportional(15.0), if current { p.strong } else { p.text });
                    if current {
                        icons::paint(ui, Rect::from_center_size(pos2(row.right() - 16.0, row.center().y), Vec2::splat(16.0)), Icon::Check, p.accent);
                    }
                    if response.clicked() {
                        picked = Some(i);
                        ui.close();
                    }
                }
            });
        });
    picked
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The accent colour: the page's main action.
    Primary,
    /// Grey: a secondary action.
    Secondary,
    /// Red: something that removes.
    Danger,
    /// Green: saving.
    Success,
    /// Text alone, underlined on hover.
    Link,
}

/// Discord's button: 38 points tall, 16 on each side, 14-point text.
pub fn button(ui: &mut egui::Ui, text: &str, kind: Kind) -> egui::Response {
    button_enabled(ui, text, kind, true)
}

pub fn button_enabled(ui: &mut egui::Ui, text: &str, kind: Kind, enabled: bool) -> egui::Response {
    let p = theme::p();
    let text_color = match kind {
        Kind::Primary => p.on_accent,
        Kind::Link => p.text,
        _ => Color32::WHITE,
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::strong(14.0), text_color);
    let width = if kind == Kind::Link { galley.size().x + 8.0 } else { (galley.size().x + 32.0).max(96.0) };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 38.0), sense);
    let hovered = enabled && response.hovered();
    let fill = match kind {
        Kind::Primary => Some(p.accent),
        Kind::Secondary => Some(Color32::from_rgb(0x4e, 0x50, 0x58)),
        Kind::Danger => Some(p.red),
        Kind::Success => Some(Color32::from_rgb(0x24, 0x80, 0x46)),
        Kind::Link => None,
    };
    if let Some(fill) = fill {
        let fill = if !enabled { fill.gamma_multiply(0.5) } else if hovered { darker(fill) } else { fill };
        ui.painter().rect_filled(rect, 8.0, fill);
    }
    let color = if enabled { text_color } else { text_color.gamma_multiply(0.6) };
    let at = rect.center() - galley.size() / 2.0;
    if kind == Kind::Link && hovered {
        ui.painter().hline(at.x..=at.x + galley.size().x, at.y + galley.size().y, Stroke::new(1.0, color));
    }
    ui.painter().galley(at, galley, color);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A secondary button the whole width of its card, as the profile's.
pub fn button_wide(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    ui.painter().rect(rect, 8.0, if response.hovered() { p.selected } else { p.hover }, Stroke::new(1.0, p.selected), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::strong(14.0), p.strong);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn darker(c: Color32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * 0.85) as u8, (c.g() as f32 * 0.85) as u8, (c.b() as f32 * 0.85) as u8)
}

/// What the unsaved changes bar asked for.
#[derive(Default)]
pub struct SaveBar {
    pub reset: bool,
    pub save: bool,
}

/// Discord's bar over the bottom of the page while changes wait: "Careful
/// — you have unsaved changes!", Reset and Save Changes. Shaking red for a moment when someone tries to leave
/// (`nag` since then).
pub fn save_bar(ctx: &egui::Context, id: &str, page: Rect, nag: Option<std::time::Instant>) -> SaveBar {
    let p = theme::p();
    let mut out = SaveBar::default();
    let shake = nag.map(|t| t.elapsed().as_secs_f32()).filter(|t| *t < 0.6);
    let dx = shake.map_or(0.0, |t| (t * 60.0).sin() * 6.0 * (1.0 - t / 0.6));
    let width = page.width().min(CONTENT);
    let at = Pos2::new(page.left() + dx, page.bottom() - 72.0);
    egui::Area::new(egui::Id::new((id, "save-bar")))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let fill = if shake.is_some() { p.red } else { Color32::from_rgb(0x11, 0x12, 0x14) };
            Frame::new()
                .fill(fill)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin { left: 16, right: 10, top: 10, bottom: 10 })
                .shadow(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(90) })
                .show(ui, |ui| {
                    ui.set_width(width - 26.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!("Careful — you have unsaved changes!")).size(16.0).color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            out.save = button(ui, t!("Save Changes"), Kind::Success).clicked();
                            out.reset = button(ui, t!("Reset"), Kind::Link).clicked();
                        });
                    });
                });
        });
    if shake.is_some() {
        ctx.request_repaint();
    }
    out
}

// ---- Discord's settings since its redesign ------------------------------------

/// The new sidebar's section name ("Billing", "Experience").
pub fn nav_section(ui: &mut egui::Ui, text: &str) {
    let p = theme::p();
    ui.add_space(12.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 10.0, rect.top()), pos2(rect.right() - 10.0, rect.bottom())), text, theme::strong(12.0), p.muted);
}

/// A sidebar page: its icon and name, 36 points, the open one filled; a
/// destructive one (Log Out) in red.
pub fn nav_icon_item(ui: &mut egui::Ui, icon: Icon, text: &str, selected: bool, danger: bool) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 6.0, if selected { p.selected } else { p.hover });
    }
    let color = if danger { p.red } else if selected || response.hovered() { p.strong } else { p.muted };
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(20.0)), icon, color);
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 40.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom())), text, FontId::proportional(15.0), color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The sidebar's search box.
pub fn nav_search(ui: &mut egui::Ui, text: &mut String) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().rect(rect, 6.0, p.rail, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(16.0)), Icon::Search, p.muted);
    ui.put(
        Rect::from_min_max(pos2(rect.left() + 32.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom())),
        egui::TextEdit::singleline(text)
            .frame(Frame::NONE)
            .font(FontId::proportional(14.0))
            .text_color(p.text)
            .hint_text(RichText::new(t!("Search")).color(p.faint))
            .vertical_align(egui::Align::Center),
    );
    ui.add_space(8.0);
}

/// A part of a page, as Discord's settings categories: the name, what it
/// is about, the settings, and the line under them. An empty title leaves
/// the header out.
pub fn category(ui: &mut egui::Ui, title: &str, subtitle: &str, body: impl FnOnce(&mut egui::Ui)) {
    let p = theme::p();
    if !title.is_empty() {
        ui.add(egui::Label::new(RichText::new(title).font(theme::strong(18.0)).color(p.strong)).wrap());
        if !subtitle.is_empty() {
            ui.add_space(4.0);
            note(ui, subtitle);
        }
        ui.add_space(16.0);
    }
    body(ui);
    ui.add_space(12.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.selected));
    ui.add_space(28.0);
}

/// A setting's name and what it does, over a control drawn by the caller
/// on the right (a button, a select): returns the control's response.
pub fn row<R>(ui: &mut egui::Ui, title: &str, subtitle: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = theme::p();
    let out = ui
        .horizontal(|ui| {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(vec2(width * 0.6, 24.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(RichText::new(title).size(16.0).color(p.strong)).wrap());
                if !subtitle.is_empty() {
                    ui.add_space(2.0);
                    note(ui, subtitle);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control).inner
        })
        .inner;
    ui.add_space(18.0);
    out
}

/// A slider row: the name, the note, and the slider under them with its
/// value; true when it moved.
pub fn slider_row(ui: &mut egui::Ui, title: &str, subtitle: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, shown: impl Fn(f64) -> String) -> bool {
    let p = theme::p();
    ui.label(RichText::new(title).size(16.0).color(p.strong));
    if !subtitle.is_empty() {
        ui.add_space(2.0);
        note(ui, subtitle);
    }
    ui.add_space(8.0);
    ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(120.0);
    ui.visuals_mut().selection.bg_fill = p.accent;
    let changed = ui.add(egui::Slider::new(value, range).trailing_fill(true).custom_formatter(move |v, _| shown(v))).changed();
    ui.add_space(20.0);
    changed
}

/// A note that a page or setting is not in this client yet.
pub fn unavailable(ui: &mut egui::Ui, text: &str) {
    let p = theme::p();
    Frame::new().fill(p.rail).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            icons::paint(ui, rect, Icon::Info, p.muted);
            ui.add(egui::Label::new(RichText::new(text).size(14.0).color(p.text)).wrap());
        });
    });
    ui.add_space(16.0);
}
