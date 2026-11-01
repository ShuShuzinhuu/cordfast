//! The interface's languages, as Discord offers them. The strings in the
//! code are English, the source language; every other language is a
//! gettext catalog in `assets/i18n` compiled into the program by
//! fastframe-i18n, so nothing is read or fetched at run time. A message a
//! catalog lacks shows in English.
//!
//! `t!("Settings")` gives a `&'static str` in the current language, and
//! `t!("Joined {when}", when = date)` a `String` with its fields filled;
//! `tn!(count, "{count} member", "{count} members")` picks the plural.
//! Strings kept in constants are marked with `n!("…")` (gettext's noop:
//! the literal itself, found by `scripts/i18n.py`) and translated where
//! they are shown, with [`tr`]. `tp!("navigation", "Forward")` tells apart
//! the same English said in two places.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

/// A string for the catalogs that stays as it is here: for constants,
/// translated with [`tr`] where they are shown.
#[macro_export]
macro_rules! n {
    ($source:literal) => {
        $source
    };
}

/// A string in the current language, its `{fields}` filled in.
#[macro_export]
macro_rules! t {
    ($source:literal) => {
        $crate::i18n::tr($source)
    };
    ($source:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut text = $crate::i18n::tr($source).to_owned();
        $(text = text.replace(concat!("{", stringify!($name), "}"), &$value.to_string());)+
        text
    }};
}

/// A phrase whose translation depends on where it appears ("Forward" is
/// "Avançar" in the title bar and "Encaminhar" for a message): gettext's
/// `msgctxt` names the place.
#[macro_export]
macro_rules! tp {
    ($context:literal, $source:literal) => {
        $crate::i18n::ptr($context, $source)
    };
}

/// A counted phrase in the current language: `{count}` and any other
/// fields filled in.
#[macro_export]
macro_rules! tn {
    ($count:expr, $one:literal, $many:literal $(, $name:ident = $value:expr)* $(,)?) => {{
        let count = $count;
        #[allow(unused_mut)]
        let mut text = $crate::i18n::ntr($one, $many, u32::try_from(count).unwrap_or(u32::MAX)).replace("{count}", &count.to_string());
        $(text = text.replace(concat!("{", stringify!($name), "}"), &$value.to_string());)*
        text
    }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Locale {
    English,
    PortugueseBrazil,
    Spanish,
    French,
    German,
    Italian,
    Russian,
    Japanese,
}

/// Each language: its tag (the settings file and the catalog's name), its
/// own name, its name in English (translated where it is shown) and its
/// flag, as Discord's language list shows them.
pub const ALL: &[(Locale, &str, &str, &str, &str)] = &[
    (Locale::English, "en-US", "English, US", n!("English, US"), "🇺🇸"),
    (Locale::PortugueseBrazil, "pt-BR", "Português do Brasil", n!("Portuguese, Brazilian"), "🇧🇷"),
    (Locale::Spanish, "es-ES", "Español", n!("Spanish"), "🇪🇸"),
    (Locale::French, "fr", "Français", n!("French"), "🇫🇷"),
    (Locale::German, "de", "Deutsch", n!("German"), "🇩🇪"),
    (Locale::Italian, "it", "Italiano", n!("Italian"), "🇮🇹"),
    (Locale::Russian, "ru", "Русский", n!("Russian"), "🇷🇺"),
    (Locale::Japanese, "ja", "日本語", n!("Japanese"), "🇯🇵"),
];

impl fastframe_i18n::Locale for Locale {
    fn catalog(self) -> Option<&'static dyn fastframe_i18n::Translator> {
        Some(match self {
            Self::English => return None,
            Self::PortugueseBrazil => &pt_br::Translator,
            Self::Spanish => &es_es::Translator,
            Self::French => &fr::Translator,
            Self::German => &de::Translator,
            Self::Italian => &it::Translator,
            Self::Russian => &ru::Translator,
            Self::Japanese => &ja::Translator,
        })
    }
}

impl Locale {
    /// The language a settings tag names.
    pub fn from_tag(tag: &str) -> Option<Self> {
        ALL.iter().find(|(_, t, ..)| t.eq_ignore_ascii_case(tag)).map(|(l, ..)| *l)
    }

    /// The closest language to a system tag: Portuguese is Brazil's,
    /// Spanish Spain's, as Discord's own choices.
    fn from_system(tag: &fastframe_i18n::LanguageTag) -> Option<Self> {
        Some(match tag.language.as_str() {
            "en" => Self::English,
            "pt" => Self::PortugueseBrazil,
            "es" => Self::Spanish,
            "fr" => Self::French,
            "de" => Self::German,
            "it" => Self::Italian,
            "ru" => Self::Russian,
            "ja" => Self::Japanese,
            _ => return None,
        })
    }

    /// The system's language, English when it is not one of these. Tests
    /// read English whatever the machine prefers.
    pub fn system() -> Self {
        if cfg!(test) {
            return Self::English;
        }
        fastframe_i18n::detect(Self::from_system).unwrap_or(Self::English)
    }

    /// Discord's help center in this language.
    pub fn help_url(self) -> &'static str {
        match self {
            Self::English => "https://support.discord.com/hc/en-us",
            Self::PortugueseBrazil => "https://support.discord.com/hc/pt-br",
            Self::Spanish => "https://support.discord.com/hc/es",
            Self::French => "https://support.discord.com/hc/fr",
            Self::German => "https://support.discord.com/hc/de",
            Self::Italian => "https://support.discord.com/hc/it",
            Self::Russian => "https://support.discord.com/hc/ru",
            Self::Japanese => "https://support.discord.com/hc/ja",
        }
    }

    /// The tag the chosen language uses for dates and Discord's API
    /// (`locale` in the user settings and the requests' headers).
    pub fn discord_tag(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::PortugueseBrazil => "pt-BR",
            Self::Spanish => "es-ES",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Russian => "ru",
            Self::Japanese => "ja",
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Shows the interface in `locale` from the next frame on.
pub fn set(locale: Locale) {
    CURRENT.store(locale as u8, Ordering::Relaxed);
}

pub fn current() -> Locale {
    let value = CURRENT.load(Ordering::Relaxed);
    ALL.iter().map(|(l, ..)| *l).find(|l| *l as u8 == value).unwrap_or(Locale::English)
}

/// The settings' choice: a tag, or none to follow the system.
pub fn apply(choice: Option<&str>) {
    set(choice.and_then(Locale::from_tag).unwrap_or_else(Locale::system));
}

/// `source` in the current language. The compiled catalogs hand back
/// their own static strings, so this never allocates.
pub fn tr(source: &'static str) -> &'static str {
    match fastframe_i18n::gettext(current(), source) {
        Cow::Borrowed(text) => text,
        Cow::Owned(_) => source,
    }
}

/// `source` as the place `context` names says it, in the current language.
pub fn ptr(context: &'static str, source: &'static str) -> &'static str {
    match fastframe_i18n::pgettext(current(), context, source) {
        Cow::Borrowed(text) => text,
        Cow::Owned(_) => source,
    }
}

/// The singular or plural of a counted phrase in the current language.
pub fn ntr(one: &'static str, many: &'static str, count: u32) -> &'static str {
    match fastframe_i18n::ngettext(current(), one, many, count) {
        Cow::Borrowed(text) => text,
        Cow::Owned(_) => if count == 1 { one } else { many },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_a_tag_and_a_catalog_but_english() {
        for (locale, tag, ..) in ALL {
            assert_eq!(Locale::from_tag(tag), Some(*locale));
            assert_eq!(fastframe_i18n::Locale::catalog(*locale).is_none(), *locale == Locale::English, "{tag}");
        }
    }

    /// The `{fields}` of a text, sorted.
    fn fields(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = text.match_indices('{').filter_map(|(at, _)| text[at..].find('}').map(|end| &text[at..=at + end])).collect();
        found.sort_unstable();
        found
    }

    /// A translation that drops or invents a `{field}` would show a raw
    /// brace or lose a name: every catalog keeps the English one's fields
    /// (a singular may leave out `{count}`, as "one message" does). Date
    /// and time patterns, fields and punctuation only, are each
    /// language's pick of the fields `text.rs` offers ("{HH}:{mm}").
    #[test]
    fn translations_keep_their_fields() {
        for (_, tag, ..) in &ALL[1..] {
            let path = format!("{}/assets/i18n/{tag}.po", env!("CARGO_MANIFEST_DIR"));
            let text = std::fs::read_to_string(&path).unwrap();
            let (mut one, mut many) = (String::new(), None::<String>);
            for line in text.lines() {
                let quoted = |prefix: &str| line.strip_prefix(prefix).map(|rest| rest.trim_matches('"').replace("\\\"", "\""));
                if let Some(id) = quoted("msgid ") {
                    (one, many) = (id, None);
                } else if let Some(plural) = quoted("msgid_plural ") {
                    many = Some(plural);
                } else if let Some(translated) = quoted("msgstr ").or_else(|| line.strip_prefix("msgstr[").and_then(|rest| rest.split_once("] ")).map(|(_, rest)| rest.trim_matches('"').replace("\\\"", "\""))) {
                    if translated.is_empty() || one.is_empty() {
                        continue;
                    }
                    let source = many.as_deref().unwrap_or(&one);
                    let pattern = fields(source).iter().fold(source.to_owned(), |rest, field| rest.replacen(field, "", 1));
                    if !pattern.chars().any(char::is_alphabetic) {
                        continue;
                    }
                    let mut expected = fields(source);
                    let got = fields(&translated);
                    if many.is_some() && !got.contains(&"{count}") {
                        expected.retain(|field| *field != "{count}");
                    }
                    assert_eq!(got, expected, "{tag}: {one:?} -> {translated:?}");
                }
            }
        }
    }
}
