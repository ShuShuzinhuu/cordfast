//! Message text: times in the local zone, and Discord's `<@id>`-style
//! tokens turned into names.

use std::sync::OnceLock;

use jiff::tz::TimeZone;

fn zone() -> &'static TimeZone {
    static ZONE: OnceLock<TimeZone> = OnceLock::new();
    ZONE.get_or_init(TimeZone::system)
}

/// The account's time format: 0 the language's, 1 12-hour, 2 24-hour.
pub static HOUR_CYCLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The hour as the language writes it: "2:05 PM" in English, "14:05" in
/// most others (the pattern is a catalog string: `{HH}` or `{h}` for the
/// hour, `{mm}`, `{ampm}`).
fn hour(at: &jiff::Zoned) -> String {
    let h = at.hour();
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    // Settings → Language & Time → Time format, when not automatic.
    match HOUR_CYCLE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return format!("{h12}:{:02} {}", at.minute(), if h < 12 { "AM" } else { "PM" }),
        2 => return format!("{h:02}:{:02}", at.minute()),
        _ => {}
    }
    t!("{h}:{mm} {ampm}", h = h12, HH = format!("{h:02}"), mm = format!("{:02}", at.minute()), ampm = if h < 12 { "AM" } else { "PM" })
}

/// A date in figures, in the language's order: "10/03/2026" in English,
/// "03/10/2026" in Portuguese.
fn numeric(date: jiff::civil::Date) -> String {
    t!("{MM}/{DD}/{YYYY}", MM = format!("{:02}", date.month()), DD = format!("{:02}", date.day()), YYYY = date.year())
}

/// "Today at 2:05 PM", "Yesterday at 9:12 AM" or "10/03/2026 9:40 PM", in
/// the interface's language.
pub fn time(ms: u64) -> String {
    let Ok(stamp) = jiff::Timestamp::from_millisecond(ms as i64) else {
        return String::new();
    };
    let at = stamp.to_zoned(zone().clone());
    let today = jiff::Timestamp::now().to_zoned(zone().clone()).date();
    if at.date() == today {
        t!("Today at {time}", time = hour(&at))
    } else if today.yesterday().ok() == Some(at.date()) {
        t!("Yesterday at {time}", time = hour(&at))
    } else {
        format!("{} {}", numeric(at.date()), hour(&at))
    }
}

/// Just the hour, for messages grouped under another's header.
pub fn clock(ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64)
        .map(|s| hour(&s.to_zoned(zone().clone())))
        .unwrap_or_default()
}

/// Replaces `<@id>`, `<#id>`, `<@&id>`, `<:name:id>` and `<t:secs:style>`
/// with what they name, through `name` for the first three.
pub fn resolve(text: &str, name: impl Fn(char, u64) -> Option<String>) -> String {
    if !text.contains('<') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            return out;
        };
        let inner = &tail[1..end];
        match token(inner, &name) {
            Some(replacement) => out.push_str(&replacement),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn token(inner: &str, name: &impl Fn(char, u64) -> Option<String>) -> Option<String> {
    if let Some(id) = inner.strip_prefix("@&") {
        let id = id.parse().ok()?;
        return Some(format!("@{}", name('&', id).unwrap_or_else(|| t!("unknown-role").into())));
    }
    if let Some(id) = inner.strip_prefix('@') {
        let id = id.trim_start_matches('!').parse().ok()?;
        return Some(format!("@{}", name('@', id).unwrap_or_else(|| t!("unknown-user").into())));
    }
    if let Some(id) = inner.strip_prefix('#') {
        let id = id.parse().ok()?;
        return Some(format!("#{}", name('#', id).unwrap_or_else(|| t!("unknown").into())));
    }
    if let Some(rest) = inner.strip_prefix("t:") {
        let secs: u64 = rest.split(':').next()?.parse().ok()?;
        return Some(time(secs * 1000));
    }
    let emoji = inner.strip_prefix("a:").or_else(|| inner.strip_prefix(':'))?;
    let (emoji_name, id) = emoji.rsplit_once(':')?;
    id.parse::<u64>().ok()?;
    Some(format!(":{emoji_name}:"))
}

/// Splits text into plain runs and URLs.
pub fn links(text: &str) -> Vec<(bool, &str)> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(start) = rest
        .find("https://")
        .into_iter()
        .chain(rest.find("http://"))
        .min()
    {
        if start > 0 {
            pieces.push((false, &rest[..start]));
        }
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '>' || c == '<')
            .unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(['.', ',', ')', '!', '?', ';', ':']);
        pieces.push((true, url));
        rest = &tail[url.len()..];
    }
    if !rest.is_empty() {
        pieces.push((false, rest));
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_tokens() {
        let out = resolve("hi <@123> in <#9> <:pepe:55> <b>", |kind, id| {
            Some(format!("{kind}{id}"))
        });
        assert_eq!(out, "hi @@123 in ##9 :pepe: <b>");
    }

    #[test]
    fn splits_links() {
        let pieces = links("see https://a.com/x, ok");
        assert_eq!(pieces, vec![(false, "see "), (true, "https://a.com/x"), (false, ", ok")]);
    }
}

/// The whole date and hour, as Discord's tooltips on timestamps write it:
/// "Tuesday, October 28, 2026 at 12:00 PM".
pub fn full(ms: u64) -> String {
    const DAYS: [&str; 7] = [n!("Monday"), n!("Tuesday"), n!("Wednesday"), n!("Thursday"), n!("Friday"), n!("Saturday"), n!("Sunday")];
    jiff::Timestamp::from_millisecond(ms as i64)
        .map(|s| {
            let at = s.to_zoned(zone().clone());
            let weekday = crate::i18n::tr(DAYS[at.weekday().to_monday_zero_offset() as usize]);
            t!("{weekday}, {date} at {time}", weekday = weekday, date = day(ms), time = hour(&at))
        })
        .unwrap_or_default()
}

/// When something ends: the hour today, or the date and hour after.
pub fn until(ms: u64) -> String {
    let Ok(stamp) = jiff::Timestamp::from_millisecond(ms as i64) else {
        return String::new();
    };
    let at = stamp.to_zoned(zone().clone());
    if at.date() == jiff::Timestamp::now().to_zoned(zone().clone()).date() {
        hour(&at)
    } else {
        format!("{} {}", numeric(at.date()), hour(&at))
    }
}

/// The local date, as Discord's day separators write it: "October 27,
/// 2026" ("27 de outubro de 2026" in Portuguese).
pub fn day(ms: u64) -> String {
    const MONTHS: [&str; 12] = [
        n!("January"), n!("February"), n!("March"), n!("April"), n!("May"), n!("June"),
        n!("July"), n!("August"), n!("September"), n!("October"), n!("November"), n!("December"),
    ];
    jiff::Timestamp::from_millisecond(ms as i64)
        .map(|s| {
            let date = s.to_zoned(zone().clone()).date();
            t!("{month} {day}, {year}", month = crate::i18n::tr(MONTHS[date.month() as usize - 1]), day = date.day(), year = date.year())
        })
        .unwrap_or_default()
}

/// Months cut short. "May " keeps its space so the catalogs tell it from
/// the full month.
const SHORT_MONTHS: [&str; 12] = [n!("Jan"), n!("Feb"), n!("Mar"), n!("Apr"), n!("May "), n!("Jun"), n!("Jul"), n!("Aug"), n!("Sep"), n!("Oct"), n!("Nov"), n!("Dec")];

/// The local date with the month cut short, as Discord's "Member Since"
/// writes it: "Oct 3, 2015" ("3 de out. de 2015" in Portuguese).
pub fn short_day(ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64)
        .map(|s| {
            let date = s.to_zoned(zone().clone()).date();
            let month = crate::i18n::tr(SHORT_MONTHS[date.month() as usize - 1]).trim_end();
            t!("{month} {day}, {year}", month = month, day = date.day(), year = date.year())
        })
        .unwrap_or_default()
}

/// The month and year, as a server's card writes when it began: "Mar
/// 2024" ("mar. de 2024" in Portuguese).
pub fn month_year(ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64)
        .map(|s| {
            let date = s.to_zoned(zone().clone()).date();
            let month = crate::i18n::tr(SHORT_MONTHS[date.month() as usize - 1]).trim_end();
            t!("{month} {year}", month = month, year = date.year())
        })
        .unwrap_or_default()
}

/// A count with the language's thousands separator: "17,826" ("17.826" in
/// Portuguese).
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let separator = tp!("thousands separator", ",");
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push_str(separator);
        }
        out.push(c);
    }
    out
}

/// Whether a character is Latin (with its accents) or common punctuation,
/// which the interface font draws.
fn latin(c: char) -> bool {
    c.is_ascii() || ('\u{a0}'..='\u{24f}').contains(&c) || ('\u{1e00}'..='\u{1eff}').contains(&c)
}

/// A one-line layout split where the text changes between Latin and other
/// scripts. A run that starts in a fallback font (Oriya "୨୧" before
/// "-mídia") would otherwise keep that font for the Latin letters after it,
/// and some fallback fonts draw "í" as "i" and a separate "´".
pub fn script_job(text: &str, font: egui::FontId, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    // Formats that differ (by an invisible amount of letter spacing) keep
    // egui from merging the sections back into one shaping run.
    let format = |latin: bool| egui::TextFormat {
        extra_letter_spacing: if latin { 0.0 } else { 0.001 },
        ..egui::TextFormat::simple(font.clone(), color)
    };
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (i, c) in text.char_indices() {
        let kind = latin(c);
        if let Some(previous) = current
            && previous != kind
        {
            job.append(&text[start..i], 0.0, format(previous));
            start = i;
        }
        current = Some(kind);
    }
    job.append(&text[start..], 0.0, format(current.unwrap_or(true)));
    job
}
