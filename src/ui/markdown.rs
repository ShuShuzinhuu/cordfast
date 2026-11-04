//! Discord's markdown: **bold**, *italic*, __underline__, ~~strike~~,
//! `code`, ```blocks```, ||spoilers||, > quotes, # headings, - lists,
//! -# subtext, links, mentions, custom emoji and <t:...> timestamps.
//! Parsed into blocks of styled spans, drawn as wrapped widgets.

use egui::{Color32, Frame, Label, Margin, RichText, Sense, Stroke, vec2};

use crate::app::App;
use crate::discord::model::Id;
use crate::theme;

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
    spoiler: bool,
    /// Not Latin: drawn as its own run (see `text::script_job`).
    other_script: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Text,
    Newline,
    Link(String),
    /// A user (`@`), channel (`#`) or role (`&`) mention.
    Mention(char, Id),
    Everyone,
    Emoji { id: Id, animated: bool },
    Time(u64),
    /// Server navigation (`<id:customize>`) and slash commands, as a pill.
    Pill(String),
}

#[derive(Clone)]
pub struct Span {
    kind: Kind,
    text: String,
    style: Style,
}

pub enum Block {
    Paragraph(Vec<Span>),
    Heading(u8, Vec<Span>),
    Subtext(Vec<Span>),
    Quote(Vec<Span>),
    Item(Vec<Span>),
    Code(String),
}

pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("```") else { break };
        lines(&rest[..start], &mut blocks);
        let mut code = &after[..end];
        // A language tag on the opening line.
        if let Some((first, body)) = code.split_once('\n')
            && !first.contains(' ')
            && first.len() < 20
        {
            code = body;
        }
        blocks.push(Block::Code(code.trim_end_matches('\n').to_owned()));
        rest = &after[end + 3..];
        rest = rest.strip_prefix('\n').unwrap_or(rest);
    }
    lines(rest, &mut blocks);
    blocks
}

fn lines(text: &str, blocks: &mut Vec<Block>) {
    let mut paragraph: Vec<Span> = Vec::new();
    let flush = |paragraph: &mut Vec<Span>, blocks: &mut Vec<Block>| {
        while paragraph.last().is_some_and(|s| s.kind == Kind::Newline) {
            paragraph.pop();
        }
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(std::mem::take(paragraph)));
        }
    };
    for line in text.split('\n') {
        let special = if let Some(rest) = line.strip_prefix("-# ") {
            Some(Block::Subtext(inline(rest)))
        } else if let Some(rest) = line.strip_prefix("### ") {
            Some(Block::Heading(3, inline(rest)))
        } else if let Some(rest) = line.strip_prefix("## ") {
            Some(Block::Heading(2, inline(rest)))
        } else if let Some(rest) = line.strip_prefix("# ") {
            Some(Block::Heading(1, inline(rest)))
        } else if let Some(rest) = line.strip_prefix("> ") {
            Some(Block::Quote(inline(rest)))
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            Some(Block::Item(inline(rest)))
        } else {
            None
        };
        match special {
            Some(block) => {
                flush(&mut paragraph, blocks);
                blocks.push(block);
            }
            None => {
                if !paragraph.is_empty() {
                    paragraph.push(Span { kind: Kind::Newline, text: String::new(), style: Style::default() });
                }
                paragraph.extend(inline(line));
            }
        }
    }
    flush(&mut paragraph, blocks);
}

/// Inline markup. A marker toggles its style only when it closes later on.
pub fn inline(text: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut style = Style::default();
    let mut buffer = String::new();
    let push_text = |spans: &mut Vec<Span>, buffer: &mut String, style: Style| {
        if !buffer.is_empty() {
            spans.push(Span { kind: Kind::Text, text: std::mem::take(buffer), style });
        }
    };
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < text.len() {
        let rest = &text[i..];
        // Escapes.
        if rest.starts_with('\\')
            && let Some(c) = rest[1..].chars().next()
            && "*_~`|<>#-\\".contains(c)
        {
            buffer.push(c);
            i += 1 + c.len_utf8();
            continue;
        }
        // Inline code takes everything to its closing backtick.
        if rest.starts_with('`')
            && let Some(end) = rest[1..].find('`')
        {
            push_text(&mut spans, &mut buffer, style);
            spans.push(Span {
                kind: Kind::Text,
                text: rest[1..1 + end].to_owned(),
                style: Style { code: true, ..style },
            });
            i += end + 2;
            continue;
        }
        if rest.starts_with('[')
            && let Some(close) = rest.find("](")
            && !rest[1..close].contains('[')
            && let Some(end) = rest[close + 2..].find(')')
        {
            let url = rest[close + 2..close + 2 + end].trim_matches(['<', '>']);
            if url.starts_with("http://") || url.starts_with("https://") {
                push_text(&mut spans, &mut buffer, style);
                spans.push(Span { kind: Kind::Link(url.to_owned()), text: rest[1..close].to_owned(), style });
                i += close + 2 + end + 1;
                continue;
            }
        }
        if rest.starts_with("http://") || rest.starts_with("https://") {
            push_text(&mut spans, &mut buffer, style);
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                .unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(['.', ',', ')', '!', '?', ';', ':', '*', '_', '|', '~']);
            spans.push(Span { kind: Kind::Link(url.to_owned()), text: url.to_owned(), style });
            i += url.len();
            continue;
        }
        if rest.starts_with('<')
            && let Some(end) = rest.find('>')
            && let Some(kind) = token(&rest[1..end])
        {
            push_text(&mut spans, &mut buffer, style);
            let text = match &kind {
                Kind::Emoji { .. } => rest[1..end].rsplit(':').nth(1).unwrap_or("").to_owned(),
                _ => String::new(),
            };
            spans.push(Span { kind, text, style });
            i += end + 1;
            continue;
        }
        if rest.starts_with("@everyone") || rest.starts_with("@here") {
            push_text(&mut spans, &mut buffer, style);
            let len = if rest.starts_with("@everyone") { 9 } else { 5 };
            spans.push(Span { kind: Kind::Everyone, text: rest[..len].to_owned(), style });
            i += len;
            continue;
        }
        let markers: [(&str, fn(&mut Style) -> &mut bool); 7] = [
            ("***", |s| &mut s.bold),
            ("**", |s| &mut s.bold),
            ("__", |s| &mut s.underline),
            ("~~", |s| &mut s.strike),
            ("||", |s| &mut s.spoiler),
            ("*", |s| &mut s.italic),
            ("_", |s| &mut s.italic),
        ];
        let mut matched = false;
        for (marker, flag) in markers {
            if !rest.starts_with(marker) {
                continue;
            }
            let on = *flag(&mut style.clone());
            // Opening needs a closing marker ahead; `_` only between words.
            if !on && !rest[marker.len()..].contains(marker) {
                continue;
            }
            if marker == "_" && !on && i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
                continue;
            }
            push_text(&mut spans, &mut buffer, style);
            if marker == "***" {
                style.bold = !style.bold;
                style.italic = !style.italic;
            } else {
                let f = flag(&mut style);
                *f = !*f;
            }
            i += marker.len();
            matched = true;
            break;
        }
        if matched {
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        buffer.push(c);
        i += c.len_utf8();
    }
    push_text(&mut spans, &mut buffer, style);
    split_scripts(spans)
}

/// Splits text spans where the script changes between Latin and others,
/// so a fallback font chosen for one never draws the other.
fn split_scripts(spans: Vec<Span>) -> Vec<Span> {
    let latin = |c: char| c.is_ascii() || ('\u{a0}'..='\u{24f}').contains(&c) || ('\u{1e00}'..='\u{1eff}').contains(&c);
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if span.kind != Kind::Text || span.text.chars().all(latin) {
            out.push(span);
            continue;
        }
        let mut start = 0;
        let mut current: Option<bool> = None;
        let text = span.text.clone();
        for (i, c) in text.char_indices() {
            let kind = latin(c);
            if let Some(previous) = current
                && previous != kind
            {
                out.push(Span { kind: Kind::Text, text: text[start..i].to_owned(), style: Style { other_script: !previous, ..span.style } });
                start = i;
            }
            current = Some(kind);
        }
        out.push(Span { kind: Kind::Text, text: text[start..].to_owned(), style: Style { other_script: current == Some(false), ..span.style } });
    }
    out
}

fn token(inner: &str) -> Option<Kind> {
    if let Some(place) = inner.strip_prefix("id:") {
        return Some(Kind::Pill(match place {
            "customize" => t!("Channels & Roles").into(),
            "browse" => t!("Browse Channels").into(),
            "guide" => t!("Server Guide").into(),
            "home" => t!("Home").into(),
            "linked-roles" => t!("Linked Roles").into(),
            other => other.to_owned(),
        }));
    }
    if let Some(command) = inner.strip_prefix('/')
        && let Some((name, id)) = command.rsplit_once(':')
        && id.parse::<u64>().is_ok()
    {
        return Some(Kind::Pill(format!("/{name}")));
    }
    if let Some(id) = inner.strip_prefix("@&") {
        return Some(Kind::Mention('&', id.parse().ok()?));
    }
    if let Some(id) = inner.strip_prefix('@') {
        return Some(Kind::Mention('@', id.trim_start_matches('!').parse().ok()?));
    }
    if let Some(id) = inner.strip_prefix('#') {
        return Some(Kind::Mention('#', id.parse().ok()?));
    }
    if let Some(rest) = inner.strip_prefix("t:") {
        return Some(Kind::Time(rest.split(':').next()?.parse().ok()?));
    }
    let (animated, emoji) = match inner.strip_prefix("a:") {
        Some(rest) => (true, rest),
        None => (false, inner.strip_prefix(':')?),
    };
    let (_, id) = emoji.rsplit_once(':')?;
    Some(Kind::Emoji { id: id.parse().ok()?, animated })
}

/// Whether the text is only emoji (up to 30), which Discord draws large.
pub fn only_emoji(blocks: &[Block]) -> bool {
    let [Block::Paragraph(spans)] = blocks else { return false };
    let mut count = 0;
    for span in spans {
        match &span.kind {
            Kind::Emoji { .. } => count += 1,
            Kind::Text if span.text.trim().is_empty() => {}
            Kind::Text => match fastframe_emoji::only_emoji(span.text.trim()) {
                Some(n) => count += n,
                None => return false,
            },
            _ => return false,
        }
    }
    count > 0 && count <= 30
}

/// Draws parsed blocks. `key` names the message, for revealed spoilers.
pub fn show(app: &mut App, ui: &mut egui::Ui, blocks: &[Block], color: Color32, key: Id) {
    let p = theme::p();
    let jumbo = only_emoji(blocks);
    let mut spoiler_index = 0usize;
    for block in blocks {
        match block {
            Block::Paragraph(spans) => spans_view(app, ui, spans, color, 15.0, jumbo, key, &mut spoiler_index),
            Block::Heading(level, spans) => {
                let size = match level { 1 => 24.0, 2 => 20.0, _ => 17.0 };
                ui.add_space(4.0);
                let bold: Vec<Span> = spans
                    .iter()
                    .cloned()
                    .map(|mut s| {
                        s.style.bold = true;
                        s
                    })
                    .collect();
                spans_view(app, ui, &bold, p.strong, size, false, key, &mut spoiler_index);
            }
            Block::Subtext(spans) => spans_view(app, ui, spans, p.muted, 12.5, false, key, &mut spoiler_index),
            Block::Item(spans) => {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(color));
                    ui.vertical(|ui| spans_view(app, ui, spans, color, 15.0, false, key, &mut spoiler_index));
                });
            }
            Block::Quote(spans) => {
                let response = Frame::new()
                    .inner_margin(Margin { left: 12, right: 0, top: 0, bottom: 0 })
                    .show(ui, |ui| spans_view(app, ui, spans, color, 15.0, false, key, &mut spoiler_index));
                let r = response.response.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(r.min, vec2(4.0, r.height())),
                    2.0,
                    p.faint,
                );
            }
            Block::Code(code) => {
                Frame::new()
                    .fill(p.sidebar)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(4)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width().min(560.0));
                        ui.add(
                            Label::new(RichText::new(code).monospace().size(13.5).color(p.text))
                                .wrap()
                                .selectable(true),
                        );
                    });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spans_view(
    app: &mut App,
    ui: &mut egui::Ui,
    spans: &[Span],
    color: Color32,
    size: f32,
    jumbo: bool,
    key: Id,
    spoilers: &mut usize,
) {
    let p = theme::p();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for span in spans {
            let mut text = match &span.kind {
                Kind::Text | Kind::Link(_) | Kind::Everyone => RichText::new(&span.text),
                Kind::Newline => {
                    ui.end_row();
                    continue;
                }
                Kind::Mention(kind, id) => {
                    if *kind == '@'
                        && !app.users.contains_key(id)
                        && let Some(guild) = app.selected_guild
                    {
                        app.request_member(guild, *id);
                    }
                    let name = app.name_for(*kind, *id).unwrap_or_else(|| match kind {
                        '#' => t!("unknown-channel").into(),
                        '&' => t!("unknown-role").into(),
                        _ => t!("unknown-user").into(),
                    });
                    let prefix = if *kind == '#' { '#' } else { '@' };
                    let label = RichText::new(format!("{prefix}{name}"))
                        .size(size)
                        .color(p.strong)
                        .background_color(p.accent.linear_multiply(0.3));
                    let response = ui.add(Label::new(label).sense(Sense::click()));
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if *kind == '@'
                        && response.clicked()
                        && let Some(user) = app.users.get(id).cloned()
                    {
                        let at = ui.ctx().pointer_latest_pos().unwrap_or_default();
                        app.actions.push(crate::app::Action::OpenProfile {
                            user,
                            guild: app.selected_guild,
                            at,
                        });
                    } else if *kind == '#' && response.clicked() {
                        app.actions.push(crate::app::Action::SelectChannel(*id));
                    }
                    continue;
                }
                Kind::Emoji { id, animated: _ } => {
                    let px = if jumbo { 48.0 } else { size * 1.45 };
                    let url = format!("https://cdn.discordapp.com/emojis/{id}.png?size={}", if jumbo { 96 } else { 48 });
                    let (rect, response) = ui.allocate_exact_size(vec2(px, px), Sense::hover());
                    let ppp = ui.ctx().pixels_per_point();
                    match app.images.get(&url, (px * ppp) as u32) {
                        Some((texture, _)) => {
                            egui::Image::from_texture((texture, rect.size())).paint_at(ui, rect);
                        }
                        None => {
                            ui.painter().rect_filled(rect, 4.0, p.selected);
                        }
                    }
                    response.on_hover_text(format!(":{}:", span.text));
                    continue;
                }
                Kind::Time(secs) => RichText::new(crate::text::time(secs * 1000))
                    .background_color(p.selected),
                Kind::Pill(text) => RichText::new(text)
                    .color(p.strong)
                    .background_color(p.accent.linear_multiply(0.3)),
            };
            let style = span.style;
            // Discord draws a message's emoji as Twemoji pictures, not with
            // the system font (names elsewhere keep the system's): plain
            // text is split around them.
            if matches!(span.kind, Kind::Text)
                && !style.code
                && !style.spoiler
                && fastframe_emoji::pieces(&span.text).iter().any(|piece| matches!(piece, fastframe_emoji::Piece::Emoji(_)))
            {
                for piece in fastframe_emoji::pieces(&span.text) {
                    match piece {
                        fastframe_emoji::Piece::Emoji(cluster) => {
                            let px = if jumbo { 48.0 } else { (size * 1.375).round() };
                            twemoji(app, ui, cluster, px);
                        }
                        fastframe_emoji::Piece::Text(run) => {
                            let mut part = Span { text: run.to_owned(), ..span.clone() };
                            part.kind = Kind::Text;
                            text_label(ui, &part, color, if jumbo { 42.0 } else { size });
                        }
                    }
                }
                continue;
            }
            let base_size = if jumbo && matches!(span.kind, Kind::Text) { 42.0 } else { size };
            text = text.size(base_size).color(color);
            if style.bold {
                text = text.font(theme::strong(base_size));
            }
            if style.other_script {
                text = text.extra_letter_spacing(0.001);
            }
            if style.italic {
                text = text.italics();
            }
            if style.underline {
                text = text.underline();
            }
            if style.strike {
                text = text.strikethrough();
            }
            if style.code {
                text = text.monospace().size(base_size * 0.88).background_color(p.sidebar);
            }
            if matches!(span.kind, Kind::Everyone) {
                text = text.color(p.strong).background_color(p.accent.linear_multiply(0.3));
            }
            if style.spoiler {
                let index = *spoilers;
                *spoilers += 1;
                let shown = app.revealed.contains(&(key, index));
                if !shown {
                    text = text.color(p.rail).background_color(p.rail);
                } else {
                    text = text.background_color(p.selected);
                }
                let response = ui.add(Label::new(text).sense(Sense::click()));
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if shown {
                        app.revealed.remove(&(key, index));
                    } else {
                        app.revealed.insert((key, index));
                    }
                }
                continue;
            }
            match &span.kind {
                Kind::Link(url) => {
                    ui.hyperlink_to(text.color(p.link), url);
                }
                _ => {
                    ui.add(Label::new(text).wrap().selectable(true));
                }
            }
        }
    });
}

/// A run of plain message text in its style, as a selectable label.
fn text_label(ui: &mut egui::Ui, span: &Span, color: Color32, size: f32) {
    let style = span.style;
    let mut text = RichText::new(&span.text).size(size).color(color);
    if style.bold {
        text = text.font(theme::strong(size));
    }
    if style.other_script {
        text = text.extra_letter_spacing(0.001);
    }
    if style.italic {
        text = text.italics();
    }
    if style.underline {
        text = text.underline();
    }
    if style.strike {
        text = text.strikethrough();
    }
    ui.add(Label::new(text).wrap().selectable(true));
}

/// Twemoji's file for an emoji: its code points in hex, joined by "-",
/// without U+FE0F unless the sequence is joined with U+200D (the rule
/// Twemoji's own parser follows).
pub fn twemoji_code(cluster: &str) -> String {
    let joined = cluster.contains('\u{200d}');
    cluster
        .chars()
        .filter(|c| joined || *c != '\u{fe0f}')
        .map(|c| format!("{:x}", c as u32))
        .collect::<Vec<_>>()
        .join("-")
}

/// The Twemoji picture of an emoji in `rect`, the system's emoji while it
/// loads or if Twemoji has none.
pub fn paint_twemoji(app: &mut App, ui: &egui::Ui, cluster: &str, rect: egui::Rect) {
    let url = format!("https://cdn.jsdelivr.net/gh/jdecked/twemoji@15.1.0/assets/72x72/{}.png", twemoji_code(cluster));
    let ppp = ui.ctx().pixels_per_point();
    match app.images.get(&url, (rect.height() * ppp).min(72.0) as u32) {
        Some((texture, _)) => {
            egui::Image::from_texture((texture, rect.size())).paint_at(ui, rect);
        }
        None => fastframe_emoji::paint_cluster(ui, cluster, rect),
    }
}

/// An emoji in a message, as Discord draws it: the Twemoji picture (the
/// maintained fork, pinned), with the system's emoji while it loads or if
/// Twemoji has no picture for it; its :name: on hover.
fn twemoji(app: &mut App, ui: &mut egui::Ui, cluster: &str, px: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(px, px), Sense::hover());
    paint_twemoji(app, ui, cluster, rect);
    if let Some(name) = emojis::get(cluster).and_then(emojis::Emoji::shortcode) {
        response.on_hover_text(format!(":{name}:"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_twemoji_files_as_twemoji_does() {
        assert_eq!(twemoji_code("😀"), "1f600");
        // Without a joiner, U+FE0F goes: ❤️ is 2764.
        assert_eq!(twemoji_code("❤\u{fe0f}"), "2764");
        assert_eq!(twemoji_code("#\u{fe0f}\u{20e3}"), "23-20e3");
        // Joined sequences keep it.
        assert_eq!(twemoji_code("🏳\u{fe0f}\u{200d}🌈"), "1f3f3-fe0f-200d-1f308");
        assert_eq!(twemoji_code("👍🏽"), "1f44d-1f3fd");
    }

    fn styles(text: &str) -> Vec<(String, bool, bool)> {
        inline(text)
            .into_iter()
            .map(|s| (s.text, s.style.bold, s.style.italic))
            .collect()
    }

    #[test]
    fn parses_inline_styles() {
        assert_eq!(
            styles("a **b** *c* d"),
            vec![
                ("a ".into(), false, false),
                ("b".into(), true, false),
                (" ".into(), false, false),
                ("c".into(), false, true),
                (" d".into(), false, false),
            ]
        );
        // An unmatched marker stays text.
        assert_eq!(styles("2 * 3"), vec![("2 * 3".into(), false, false)]);
        // Underscores inside words are not italics.
        assert_eq!(styles("snake_case_name"), vec![("snake_case_name".into(), false, false)]);
    }

    #[test]
    fn parses_blocks_and_tokens() {
        let blocks = parse("# Title\n> quote\n```rs\nfn main() {}\n```\nhi <@12> <:pepe:34>");
        assert!(matches!(blocks[0], Block::Heading(1, _)));
        assert!(matches!(blocks[1], Block::Quote(_)));
        assert!(matches!(&blocks[2], Block::Code(c) if c == "fn main() {}"));
        let Block::Paragraph(spans) = &blocks[3] else { panic!() };
        assert!(spans.iter().any(|s| s.kind == Kind::Mention('@', 12)));
        assert!(spans.iter().any(|s| matches!(s.kind, Kind::Emoji { id: 34, .. })));
    }

    #[test]
    fn shows_the_shrug_as_the_command_writes_it() {
        let blocks = parse(&crate::discord::commands::built_in_text("shrug", ""));
        let Some(Block::Paragraph(spans)) = blocks.first() else { panic!("not a paragraph") };
        assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<String>(), "¯\\_(ツ)_/¯");
    }
}
