//! Themes. A [`Palette`] holds every colour the interface draws with; the
//! built-in Discord palettes come first, then palette files from the themes
//! folder (fastframe-theme's format, so ZapFast and Spotifast themes work
//! here too, and its eight shared palettes are installed there).
//!
//! A palette file:
//!
//! ```json
//! { "base": "dark", "colors": { "accent": "#ff79c6", "window": "#282a36" } }
//! ```
//!
//! It accepts the sixteen shared names (window, panel, surface, accent...)
//! and CordFast's own (rail, sidebar, chat, link, mention...). What a file
//! leaves out is derived from what it sets.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::RwLock;

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub rail: Color32,
    pub sidebar: Color32,
    pub chat: Color32,
    pub composer: Color32,
    pub hover: Color32,
    pub message_hover: Color32,
    pub selected: Color32,
    pub text: Color32,
    pub strong: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub link: Color32,
    pub red: Color32,
    pub green: Color32,
    pub warning: Color32,
    pub mention: Color32,
    pub account: Color32,
    /// Lines between areas and around floating ones. In Discord's dark
    /// theme the areas differ in shade and lines take the rail's; with
    /// every background black (AMOLED) these lines are what separates them.
    pub border: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const DISCORD_DARK: Palette = Palette {
    dark: true,
    rail: rgb(0x1e1f22),
    sidebar: rgb(0x2b2d31),
    chat: rgb(0x313338),
    composer: rgb(0x383a40),
    hover: rgb(0x35373c),
    message_hover: rgb(0x2e3035),
    selected: rgb(0x404249),
    text: rgb(0xdbdee1),
    strong: rgb(0xf2f3f5),
    muted: rgb(0x949ba4),
    faint: rgb(0x6d6f78),
    accent: rgb(0x5865f2),
    on_accent: Color32::WHITE,
    link: rgb(0x00a8fc),
    red: rgb(0xf23f43),
    green: rgb(0x23a55a),
    warning: rgb(0xf0b232),
    mention: Color32::from_rgba_premultiplied(0x2c, 0x27, 0x13, 0x60),
    account: rgb(0x232428),
    border: rgb(0x1e1f22),
};

pub const DISCORD_LIGHT: Palette = Palette {
    dark: false,
    rail: rgb(0xe3e5e8),
    sidebar: rgb(0xf2f3f5),
    chat: rgb(0xffffff),
    composer: rgb(0xebedef),
    hover: rgb(0xe8e9eb),
    message_hover: rgb(0xf7f7f8),
    selected: rgb(0xd4d7dc),
    text: rgb(0x313338),
    strong: rgb(0x060607),
    muted: rgb(0x5c5e66),
    faint: rgb(0x8a8d93),
    accent: rgb(0x5865f2),
    on_accent: Color32::WHITE,
    link: rgb(0x006ce7),
    red: rgb(0xda373c),
    green: rgb(0x248046),
    warning: rgb(0xd49b16),
    mention: Color32::from_rgba_premultiplied(0x40, 0x34, 0x08, 0x26),
    account: rgb(0xebedef),
    border: rgb(0xe3e5e8),
};

/// AMOLED: every background is black, so the screen's pixels stay off;
/// only inputs, hover and selection light up, and thin lines part the areas.
pub const MIDNIGHT: Palette = Palette {
    rail: rgb(0x000000),
    sidebar: rgb(0x000000),
    chat: rgb(0x000000),
    composer: rgb(0x141418),
    hover: rgb(0x16161a),
    message_hover: rgb(0x0b0b0d),
    selected: rgb(0x202026),
    account: rgb(0x000000),
    border: rgb(0x1e1e23),
    ..DISCORD_DARK
};

pub const DRACULA: Palette = Palette {
    rail: rgb(0x191a21),
    sidebar: rgb(0x21222c),
    chat: rgb(0x282a36),
    composer: rgb(0x343746),
    hover: rgb(0x2f3241),
    message_hover: rgb(0x2b2d3a),
    selected: rgb(0x44475a),
    text: rgb(0xf8f8f2),
    strong: rgb(0xffffff),
    muted: rgb(0xa2a6c4),
    faint: rgb(0x6272a4),
    accent: rgb(0xbd93f9),
    on_accent: rgb(0x191a21),
    link: rgb(0x8be9fd),
    red: rgb(0xff5555),
    green: rgb(0x50fa7b),
    warning: rgb(0xf1fa8c),
    account: rgb(0x1d1e26),
    border: rgb(0x191a21),
    ..DISCORD_DARK
};

pub const GRUVBOX: Palette = Palette {
    rail: rgb(0x1d2021),
    sidebar: rgb(0x282828),
    chat: rgb(0x32302f),
    composer: rgb(0x3c3836),
    hover: rgb(0x3a3735),
    message_hover: rgb(0x2e2c2b),
    selected: rgb(0x504945),
    text: rgb(0xebdbb2),
    strong: rgb(0xfbf1c7),
    muted: rgb(0xa89984),
    faint: rgb(0x7c6f64),
    accent: rgb(0xd79921),
    on_accent: rgb(0x1d2021),
    link: rgb(0x83a598),
    red: rgb(0xfb4934),
    green: rgb(0xb8bb26),
    warning: rgb(0xfabd2f),
    account: rgb(0x242424),
    border: rgb(0x1d2021),
    ..DISCORD_DARK
};

pub const BUILT_IN: &[(&str, Palette)] = &[
    ("Discord Escuro", DISCORD_DARK),
    ("Discord Claro", DISCORD_LIGHT),
    ("Meia-noite (AMOLED)", MIDNIGHT),
    ("Dracula", DRACULA),
    ("Gruvbox", GRUVBOX),
];

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

impl fastframe_theme::Palette for Palette {
    fn base(base: fastframe_theme::Base) -> Self {
        match base {
            fastframe_theme::Base::Dark => DISCORD_DARK,
            fastframe_theme::Base::Light => DISCORD_LIGHT,
        }
    }

    fn set(&mut self, name: &str, color: Color32) -> bool {
        match name {
            "window" | "chat" => self.chat = color,
            "panel" | "sidebar" => self.sidebar = color,
            "rail" => self.rail = color,
            "surface" | "composer" => self.composer = color,
            "surface_hover" | "hover" => self.hover = color,
            "surface_active" | "selected" => self.selected = color,
            "message_hover" => self.message_hover = color,
            "text" => self.text = color,
            "strong" => self.strong = color,
            "secondary" | "muted" => self.muted = color,
            "dim" | "faint" => self.faint = color,
            "accent" => self.accent = color,
            "on_accent" => self.on_accent = color,
            "link" => self.link = color,
            "danger" | "red" => self.red = color,
            "green" => self.green = color,
            "warning" => self.warning = color,
            "mention" => self.mention = color,
            "account" => self.account = color,
            "outline" | "border" => self.border = color,
            // Shared names CordFast has no use for.
            "accent_hover" | "overlay" | "shadow" => {}
            _ => return false,
        }
        true
    }

    fn derive(&mut self, given: &BTreeSet<&str>) {
        let has = |names: &[&str]| names.iter().any(|n| given.contains(n));
        let towards = if self.dark { Color32::BLACK } else { Color32::WHITE };
        if !has(&["rail"]) && has(&["panel", "sidebar"]) {
            self.rail = mix(self.sidebar, towards, 0.25);
        }
        if !has(&["account"]) {
            self.account = mix(self.sidebar, towards, 0.15);
        }
        if !has(&["outline", "border"]) {
            self.border = self.rail;
        }
        if !has(&["message_hover"]) {
            self.message_hover = mix(self.chat, self.hover, 0.5);
        }
        if !has(&["strong"]) && has(&["text"]) {
            let to = if self.dark { Color32::WHITE } else { Color32::BLACK };
            self.strong = mix(self.text, to, 0.5);
        }
        if !has(&["link"]) && has(&["accent"]) {
            self.link = mix(self.accent, self.text, 0.25);
        }
        if !has(&["mention"]) {
            let w = self.warning;
            self.mention = Color32::from_rgba_unmultiplied(w.r(), w.g(), w.b(), 0x1c);
        }
        self.dark = (self.chat.r() as u32 + self.chat.g() as u32 + self.chat.b() as u32) < 384;
    }
}

static CURRENT: RwLock<Palette> = RwLock::new(DISCORD_DARK);

/// The palette on screen.
pub fn p() -> Palette {
    *CURRENT.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn themes_dir() -> PathBuf {
    crate::settings::config_dir().join("themes")
}

/// Every theme to choose from: the built-in ones, then the folder's files.
pub fn catalog() -> Vec<(String, Palette)> {
    let mut list: Vec<(String, Palette)> =
        BUILT_IN.iter().map(|(n, p)| ((*n).to_owned(), *p)).collect();
    let dir = themes_dir();
    if let Err(error) = fastframe_theme::presets::install(&dir) {
        log::warn!("themes: {error}");
    }
    let mut files: Vec<(String, Palette)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            match fastframe_theme::parse_palette::<Palette>(&text) {
                Ok(palette) => {
                    let name = e.path().file_stem()?.to_string_lossy().into_owned();
                    Some((name, palette))
                }
                Err(error) => {
                    log::warn!("theme {}: {error}", e.path().display());
                    None
                }
            }
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    list.extend(files);
    list
}

/// Saves `palette` as a theme file named `name` (the settings' editor),
/// every colour written out so it reads back the same.
pub fn save(name: &str, palette: &Palette) -> std::io::Result<()> {
    let hex = |c: Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
    let p = palette;
    let colors = [
        ("chat", p.chat), ("sidebar", p.sidebar), ("rail", p.rail), ("composer", p.composer), ("hover", p.hover),
        ("selected", p.selected), ("message_hover", p.message_hover), ("text", p.text), ("strong", p.strong),
        ("muted", p.muted), ("faint", p.faint), ("accent", p.accent), ("on_accent", p.on_accent), ("link", p.link),
        ("red", p.red), ("green", p.green), ("warning", p.warning), ("mention", p.mention), ("account", p.account), ("border", p.border),
    ];
    let body = serde_json::json!({
        "base": if p.dark { "dark" } else { "light" },
        "colors": colors.iter().map(|(k, c)| ((*k).to_owned(), serde_json::Value::String(hex(*c)))).collect::<serde_json::Map<_, _>>(),
    });
    let file: String = name.chars().filter(|c| !r#"\/:*?"<>|"#.contains(*c)).collect();
    std::fs::create_dir_all(themes_dir())?;
    std::fs::write(themes_dir().join(format!("{file}.json")), serde_json::to_string_pretty(&body).unwrap_or_default())
}

/// Applies a palette, with the accent overridden when one is set.
pub fn apply(ctx: &egui::Context, mut palette: Palette, accent: Option<[u8; 3]>) {
    if let Some([r, g, b]) = accent {
        palette.accent = Color32::from_rgb(r, g, b);
    }
    *CURRENT.write().unwrap_or_else(std::sync::PoisonError::into_inner) = palette;
    ctx.set_theme(if palette.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = palette.chat;
        visuals.window_fill = palette.sidebar;
        visuals.window_stroke = egui::Stroke::new(1.0, palette.border);
        visuals.extreme_bg_color = palette.composer;
        visuals.faint_bg_color = palette.message_hover;
        visuals.override_text_color = Some(palette.text);
        visuals.hyperlink_color = palette.link;
        visuals.selection.bg_fill = palette.accent.linear_multiply(0.6);
        visuals.selection.stroke = egui::Stroke::new(1.0, palette.strong);
        for (widget, fill) in [
            (&mut visuals.widgets.noninteractive, palette.sidebar),
            (&mut visuals.widgets.inactive, palette.selected),
            (&mut visuals.widgets.hovered, palette.hover),
            (&mut visuals.widgets.active, palette.accent),
            (&mut visuals.widgets.open, palette.selected),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = egui::Stroke::NONE;
            widget.fg_stroke.color = palette.text;
        }
        visuals.widgets.active.fg_stroke.color = palette.on_accent;
        visuals.widgets.noninteractive.fg_stroke.color = palette.text;
    });
}

pub fn install(ctx: &egui::Context) {
    crate::fonts::install(ctx);
    fastframe_emoji::EmojiSetup::default().install();
    fastframe_emoji::warm_up();
    ctx.add_plugin(fastframe_emoji::EmojiPlugin::default());
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.5));
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.visuals.menu_corner_radius = 6.into();
        style.visuals.window_corner_radius = 8.into();
    });
}

/// Bold-ish text: the platform face's semibold weight.
pub fn strong(size: f32) -> egui::FontId {
    fastframe_fonts::Weight::SemiBold.font_id(size)
}

/// Text in the colour a role gives a name, as it is: Discord shows a
/// black role black on the dark theme too.
pub fn role_color(color: u32) -> Option<Color32> {
    (color != 0).then(|| rgb(color))
}

/// A name in a gradient role's two colours, letter by letter.
pub fn gradient_job(text: &str, font: egui::FontId, from: Color32, to: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let count = text.chars().count().max(2) as f32 - 1.0;
    for (i, c) in text.chars().enumerate() {
        let color = mix(from, to, i as f32 / count);
        let mut buffer = [0u8; 4];
        job.append(c.encode_utf8(&mut buffer), 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    job
}
