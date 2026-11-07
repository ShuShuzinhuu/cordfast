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

/// The user settings since Discord's redesign: a window over the app
/// (min(screen − 80, 1400) wide, the screen less 146 tall), the sidebar
/// on its left with `footer` at its bottom, a 48-point header with the
/// page's `title` and the X, and the page in a column of at most 696
/// centred under it. True when the X was clicked.
pub fn modal<T>(ui: &mut egui::Ui, id: &str, below: Rect, state: &mut T, title: &str, footer: &str, nav: impl FnOnce(&mut egui::Ui, &mut T), page: impl FnOnce(&mut egui::Ui, &mut T)) -> bool {
    let p = theme::p();
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    // `below` is what the title bar leaves, and it stays usable.
    let height = (screen.height() - 146.0).min(below.height() - 16.0).max(300.0);
    let width = (screen.width() - 80.0).min(1400.0).max(600.0);
    let mut rect = Rect::from_center_size(screen.center(), vec2(width, height));
    if rect.top() < below.top() + 8.0 {
        rect = rect.translate(vec2(0.0, below.top() + 8.0 - rect.top()));
    }
    // Last frame's categories become this frame's outline; the page fills
    // the next one.
    ctx.data_mut(|d| {
        let outline: &mut Outline = d.get_temp_mut_or_default(outline_id());
        outline.shown = std::mem::take(&mut outline.next);
    });
    // Over what else the app floats on this order (the account card).
    ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::new((id, "modal"))));
    let mut close = false;
    egui::Area::new(egui::Id::new((id, "modal")))
        .order(egui::Order::Middle)
        .fixed_pos(below.min)
        .show(&ctx, |ui| {
            // The backdrop takes the pointer from the app behind.
            let (backdrop, _) = ui.allocate_exact_size(below.size(), Sense::click());
            ui.painter().rect_filled(backdrop, 0.0, Color32::from_black_alpha(150));
            ui.painter().rect(rect, 8.0, p.chat, Stroke::new(1.0, p.border), egui::StrokeKind::Outside);
            let _ = ui.interact(rect, egui::Id::new((id, "modal-body")), Sense::click());
            let side = Rect::from_min_size(rect.min, vec2(252.0, rect.height()));
            ui.painter().rect_filled(side, CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, p.sidebar);
            ui.painter().vline(side.right(), side.y_range(), Stroke::new(1.0, p.border));
            let footer_height = if footer.is_empty() { 0.0 } else { 40.0 };
            let column = Rect::from_min_max(side.min + vec2(16.0, 16.0), side.max - vec2(16.0, footer_height));
            ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                ScrollArea::vertical().id_salt((id, "nav-scroll")).auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.set_width(column.width());
                    nav(ui, state);
                    ui.add_space(16.0);
                });
            });
            if !footer.is_empty() {
                ui.painter().text(pos2(side.left() + 16.0, side.bottom() - 20.0), Align2::LEFT_CENTER, footer, FontId::proportional(11.0), p.faint);
            }
            // The header: the page's name and the X.
            let header = Rect::from_min_max(pos2(side.right() + 1.0, rect.top()), pos2(rect.right(), rect.top() + 48.0));
            super::paint_truncated(ui, Rect::from_min_max(pos2(header.left() + 16.0, header.top()), pos2(header.right() - 56.0, header.bottom())), title, theme::strong(16.0), p.strong);
            ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, p.border));
            let x = Rect::from_center_size(pos2(header.right() - 25.0, header.center().y), Vec2::splat(32.0));
            let response = ui.interact(x, egui::Id::new((id, "modal-close")), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(x, 6.0, p.hover);
            }
            icons::paint(ui, Rect::from_center_size(x.center(), Vec2::splat(20.0)), Icon::Close, if response.hovered() { p.strong } else { p.muted });
            close = response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            // The page, scrolled on its own for each page.
            let body = Rect::from_min_max(pos2(header.left(), header.bottom() + 1.0), rect.max - vec2(1.0, 4.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                ScrollArea::vertical().id_salt((id, "page-scroll", title)).auto_shrink(false).show(ui, |ui| {
                    let column = (body.width() - 48.0).clamp(240.0, 696.0);
                    let left = ((body.width() - column) / 2.0).floor().max(0.0) as i8;
                    ctx.data_mut(|d| {
                        let outline: &mut Outline = d.get_temp_mut_or_default(outline_id());
                        outline.recording = true;
                        outline.top = body.top();
                    });
                    Frame::new().inner_margin(Margin { left, right: 0, top: 60, bottom: 80 }).show(ui, |ui| {
                        ui.set_width(column);
                        page(ui, state);
                    });
                    ctx.data_mut(|d| d.get_temp_mut_or_default::<Outline>(outline_id()).recording = false);
                });
            });
        });
    close
}

/// The page's categories as the modal's sidebar lists them under the open
/// page: last frame's titles with where they were drawn.
#[derive(Clone, Default)]
struct Outline {
    shown: Vec<(String, f32)>,
    next: Vec<(String, f32)>,
    /// Where the page's view starts on the screen.
    top: f32,
    /// True while the modal's page draws, so the server settings' pages
    /// record nothing.
    recording: bool,
    /// The category a sidebar click asked to scroll to.
    jump: Option<String>,
}

fn outline_id() -> egui::Id {
    egui::Id::new("settings-outline")
}

/// The open page's sub-items under its sidebar item, Discord's way: a
/// line on their left, the part being read in white. Only for pages of
/// two or more named parts.
pub fn nav_outline(ui: &mut egui::Ui) {
    let p = theme::p();
    let outline: Outline = ui.ctx().data(|d| d.get_temp(outline_id())).unwrap_or_default();
    if outline.shown.len() < 2 {
        return;
    }
    let current = outline.shown.iter().rposition(|(_, y)| *y <= outline.top + 120.0).unwrap_or(0);
    let mut jump = None;
    for (i, (title, _)) in outline.shown.iter().enumerate() {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        let chosen = i == current;
        ui.painter().vline(rect.left() + 16.0, rect.y_range(), Stroke::new(2.0, if chosen { p.strong } else { p.border }));
        let color = if chosen || response.hovered() { p.strong } else { p.muted };
        super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 32.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom())), title, FontId::proportional(15.0), color);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            jump = Some(title.clone());
        }
    }
    if jump.is_some() {
        ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Outline>(outline_id()).jump = jump);
    }
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
            ui.add(egui::Label::new(RichText::new(title).font(theme::strong(16.0)).color(p.strong)).wrap());
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
    select_with_icon(ui, id, shown, options, width, None)
}

/// [`select`] with an icon before the value, as Discord's device menus
/// (the microphone, the headphones).
pub fn select_with_icon(ui: &mut egui::Ui, id: &str, shown: &str, options: &[String], width: f32, icon: Option<Icon>) -> Option<usize> {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    ui.painter().rect(rect, 8.0, p.rail, Stroke::new(1.0, if open { p.accent } else { p.border }), egui::StrokeKind::Inside);
    let mut left = rect.left() + 12.0;
    if let Some(icon) = icon {
        icons::paint(ui, Rect::from_center_size(pos2(left + 8.0, rect.center().y), Vec2::splat(16.0)), icon, p.text);
        left += 24.0;
    }
    super::paint_truncated(ui, Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right() - 36.0, rect.bottom())), shown, FontId::proportional(16.0), p.text);
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
        Kind::Secondary => p.strong,
        _ => Color32::WHITE,
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::strong(14.0), text_color);
    let width = if kind == Kind::Link { galley.size().x + 8.0 } else { (galley.size().x + 32.0).max(96.0) };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 40.0), sense);
    let hovered = enabled && response.hovered();
    let fill = match kind {
        Kind::Primary => Some(p.accent),
        // Discord's refreshed secondary: the surface lifted a step, with a
        // thin border.
        Kind::Secondary => Some(p.hover),
        Kind::Danger => Some(p.red),
        Kind::Success => Some(Color32::from_rgb(0x24, 0x80, 0x46)),
        Kind::Link => None,
    };
    if let Some(fill) = fill {
        let fill = if !enabled { fill.gamma_multiply(0.5) } else if hovered && kind == Kind::Secondary { p.selected } else if hovered { darker(fill) } else { fill };
        ui.painter().rect_filled(rect, 8.0, fill);
        if kind == Kind::Secondary {
            ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.0, p.selected), egui::StrokeKind::Inside);
        }
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
    // The line that closes the section above, then the name.
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 11.0), Sense::hover());
    ui.painter().hline(line.x_range().shrink(8.0), line.bottom() - 0.5, Stroke::new(1.0, p.border));
    ui.add_space(8.0);
    if text.is_empty() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom())), text, FontId::proportional(14.0), p.muted);
}

/// A sidebar page: its icon and name, 36 points, the open one filled; a
/// destructive one (Log Out) in red.
pub fn nav_icon_item(ui: &mut egui::Ui, icon: Icon, text: &str, selected: bool, danger: bool) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 8.0, if selected { p.selected } else { p.hover });
    }
    let color = if danger { p.red } else if selected || response.hovered() { p.strong } else { p.muted };
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(20.0)), icon, color);
    super::paint_truncated(ui, Rect::from_min_max(pos2(rect.left() + 32.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom())), text, FontId::proportional(16.0), color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The sidebar's search box.
pub fn nav_search(ui: &mut egui::Ui, text: &mut String) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().rect(rect, 8.0, p.chat, Stroke::new(1.0, p.selected), egui::StrokeKind::Inside);
    icons::paint(ui, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(18.0)), Icon::Search, p.muted);
    ui.put(
        Rect::from_min_max(pos2(rect.left() + 38.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom())),
        egui::TextEdit::singleline(text)
            .frame(Frame::NONE)
            .font(FontId::proportional(16.0))
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
        let mut outline: Outline = ui.ctx().data(|d| d.get_temp(outline_id())).unwrap_or_default();
        // In the user settings' window the parts are 24-point headings the
        // sidebar lists; the server settings keep their 18.
        let size = if outline.recording { 24.0 } else { 18.0 };
        let response = ui.add(egui::Label::new(RichText::new(title).font(theme::strong(size)).color(p.strong)).wrap());
        if outline.recording {
            outline.next.push((title.to_owned(), response.rect.top()));
            if outline.jump.as_deref() == Some(title) {
                outline.jump = None;
                ui.scroll_to_rect(response.rect.expand2(vec2(0.0, 24.0)), Some(egui::Align::TOP));
            }
            ui.ctx().data_mut(|d| d.insert_temp(outline_id(), outline));
            ui.add_space(8.0);
        }
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
    let width = ui.available_width();
    let out = ui
        .allocate_ui_with_layout(vec2(width, 40.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // The name takes what the control leaves; long values beside a
            // button come first, so the control is laid out from the right.
            ui.allocate_ui_with_layout(vec2(width * 0.4, 24.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(egui::Label::new(RichText::new(title).font(theme::strong(16.0)).color(p.strong)).wrap());
                if !subtitle.is_empty() {
                    ui.add_space(2.0);
                    note(ui, subtitle);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control).inner
        })
        .inner;
    ui.add_space(22.0);
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

/// A setting's value beside its button on the right of a row, as the
/// account's username and email.
pub fn value(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(16.0).color(theme::p().strong));
}

/// Inline link text (Reveal, a help article): the link colour, underlined
/// on hover. True when clicked.
pub fn link(ui: &mut egui::Ui, text: &str) -> bool {
    let p = theme::p();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(16.0), p.link);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    if response.hovered() {
        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.link));
    }
    ui.painter().galley(rect.min, galley, p.link);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A row that opens more (Multi-Factor Authentication, Logged-in Devices):
/// the name, its state and a chevron on the right, the whole row
/// clickable. True when clicked.
pub fn link_row(ui: &mut egui::Ui, title: &str, state: &str) -> bool {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    super::paint_truncated(ui, Rect::from_min_max(rect.min, pos2(rect.center().x, rect.bottom())), title, theme::strong(16.0), p.strong);
    let chevron = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), Vec2::splat(20.0));
    icons::paint_chevron_right(ui, chevron, if response.hovered() { p.strong } else { p.text });
    ui.painter().text(pos2(chevron.left() - 12.0, rect.center().y), Align2::RIGHT_CENTER, state, FontId::proportional(16.0), p.strong);
    ui.add_space(14.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// One of the account's status rows: the icon in a rounded square, the
/// name with what it means under it, and the state on the right, with a
/// chevron when the row opens more. True when clicked.
pub fn status_row(ui: &mut egui::Ui, icon: Icon, tint: Color32, title: &str, detail: &str, state: &str, opens: bool) -> bool {
    let p = theme::p();
    let state_galley = ui.painter().layout_no_wrap(state.to_owned(), FontId::proportional(16.0), p.strong);
    let right = state_galley.size().x + if opens { 44.0 } else { 12.0 };
    let text_width = (ui.available_width() - 64.0 - right).max(120.0);
    let title_galley = ui.painter().layout(title.to_owned(), theme::strong(16.0), p.strong, text_width);
    let detail_galley = ui.painter().layout(detail.to_owned(), FontId::proportional(14.0), p.muted, text_width);
    let height = (title_galley.size().y + 4.0 + detail_galley.size().y).max(48.0);
    let sense = if opens { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    let square = Rect::from_min_size(pos2(rect.left(), rect.center().y - 24.0), Vec2::splat(48.0));
    ui.painter().rect_filled(square, 8.0, p.hover);
    icons::paint(ui, Rect::from_center_size(square.center(), Vec2::splat(24.0)), icon, tint);
    let x = square.right() + 16.0;
    let top = rect.center().y - height / 2.0;
    let title_height = title_galley.size().y;
    ui.painter().galley(pos2(x, top), title_galley, p.strong);
    ui.painter().galley(pos2(x, top + title_height + 4.0), detail_galley, p.muted);
    let mut end = rect.right();
    if opens {
        let chevron = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), Vec2::splat(20.0));
        icons::paint_chevron_right(ui, chevron, if response.hovered() { p.strong } else { p.text });
        end = chevron.left() - 12.0;
    }
    ui.painter().galley(pos2(end - state_galley.size().x, rect.center().y - state_galley.size().y / 2.0), state_galley, p.strong);
    ui.add_space(24.0);
    if opens { response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() } else { false }
}

/// Discord's plain radio group (Input Profile): a ring that fills when
/// chosen, the title and what it means under it, no card. Returns the one
/// clicked.
pub fn radios<T: Copy + PartialEq>(ui: &mut egui::Ui, current: T, options: &[(T, &str, &str)]) -> Option<T> {
    let p = theme::p();
    let mut picked = None;
    for (value, title, detail) in options {
        let chosen = *value == current;
        let width = ui.available_width() - 32.0;
        let title_galley = ui.painter().layout(title.to_string(), FontId::proportional(16.0), p.strong, width);
        let detail_galley = (!detail.is_empty()).then(|| ui.painter().layout(detail.to_string(), FontId::proportional(14.0), p.muted, width));
        let height = title_galley.size().y + detail_galley.as_ref().map_or(0.0, |g| g.size().y + 4.0);
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height.max(24.0)), Sense::click());
        let ring = pos2(rect.left() + 10.0, rect.top() + title_galley.size().y / 2.0);
        let color = if chosen { p.accent } else if response.hovered() { p.strong } else { p.muted };
        ui.painter().circle_stroke(ring, 9.0, Stroke::new(2.0, color));
        if chosen {
            ui.painter().circle_filled(ring, 5.0, p.accent);
        }
        let title_height = title_galley.size().y;
        ui.painter().galley(pos2(rect.left() + 32.0, rect.top()), title_galley, p.strong);
        if let Some(detail) = detail_galley {
            ui.painter().galley(pos2(rect.left() + 32.0, rect.top() + title_height + 4.0), detail, p.muted);
        }
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !chosen {
            picked = Some(*value);
        }
        ui.add_space(16.0);
    }
    picked
}

/// Discord's mic test meter: a row of thin ticks, lit from the left up
/// to `level` (0 to 1).
pub fn tick_meter(ui: &mut egui::Ui, width: f32, level: f32) {
    let p = theme::p();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let count = (rect.width() / 6.0).floor() as usize;
    let lit = (level.clamp(0.0, 1.0) * count as f32).round() as usize;
    for i in 0..count {
        let x = rect.left() + i as f32 * 6.0;
        let tick = Rect::from_min_size(pos2(x, rect.top() + 1.0), vec2(3.0, rect.height() - 2.0));
        ui.painter().rect_filled(tick, 1.5, if i < lit { p.green } else { p.selected });
    }
}

/// Discord's input sensitivity slider: the track yellow up to the
/// threshold and green after it, the live `level` (dB) drawn over it.
/// True when it moved.
pub fn sensitivity_slider(ui: &mut egui::Ui, value: &mut f32, level: f32) -> bool {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click_and_drag());
    let track = Rect::from_min_max(pos2(rect.left() + 8.0, rect.center().y - 4.0), pos2(rect.right() - 8.0, rect.center().y + 4.0));
    let at = |db: f32| track.left() + track.width() * ((db + 100.0) / 100.0).clamp(0.0, 1.0);
    let mut changed = false;
    if let Some(pointer) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked())
    {
        let db = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * 100.0 - 100.0;
        if (db - *value).abs() > f32::EPSILON {
            *value = db;
            changed = true;
        }
    }
    let knob = at(*value);
    let yellow = p.warning;
    let green = p.green;
    ui.painter().rect_filled(Rect::from_min_max(track.min, pos2(knob, track.bottom())), 4.0, yellow.gamma_multiply(0.45));
    ui.painter().rect_filled(Rect::from_min_max(pos2(knob, track.top()), track.max), 4.0, green.gamma_multiply(0.45));
    // The voice now: brighter where it reaches.
    let reach = at(level);
    if reach > track.left() {
        let lit_left = Rect::from_min_max(track.min, pos2(reach.min(knob), track.bottom()));
        ui.painter().rect_filled(lit_left, 4.0, yellow);
        if reach > knob {
            ui.painter().rect_filled(Rect::from_min_max(pos2(knob, track.top()), pos2(reach, track.bottom())), 4.0, green);
        }
    }
    ui.painter().circle_filled(pos2(knob, rect.center().y), 9.0, Color32::WHITE);
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    changed
}

/// Discord's volume slider: the name over a thin track, filled up to a
/// white round knob; the value shows while it is held. True when it moved.
pub fn volume_slider(ui: &mut egui::Ui, title: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    let p = theme::p();
    ui.label(RichText::new(title).font(theme::strong(16.0)).color(p.strong));
    ui.add_space(6.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 24.0), Sense::click_and_drag());
    let track = Rect::from_min_max(pos2(rect.left() + 9.0, rect.center().y - 3.0), pos2(rect.right() - 9.0, rect.center().y + 3.0));
    let (low, high) = (*range.start(), *range.end());
    let mut changed = false;
    if let Some(pointer) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked())
    {
        let moved = low + ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * (high - low);
        changed = moved != *value;
        *value = moved;
    }
    let knob = track.left() + track.width() * ((*value - low) / (high - low)).clamp(0.0, 1.0);
    ui.painter().rect_filled(track, 3.0, p.selected);
    ui.painter().rect_filled(Rect::from_min_max(track.min, pos2(knob, track.bottom())), 3.0, p.accent);
    ui.painter().circle_filled(pos2(knob, rect.center().y), 9.0, Color32::WHITE);
    if response.dragged() {
        ui.painter().text(pos2(knob, rect.top() - 4.0), Align2::CENTER_BOTTOM, format!("{:.0}%", *value * 100.0), theme::strong(12.0), p.strong);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
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
