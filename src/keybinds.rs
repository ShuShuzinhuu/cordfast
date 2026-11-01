//! Keyboard shortcuts, each one rebindable in Settings → Keybinds, as in
//! Discord. Most act while the window has focus; muting and
//! deafening also act from anywhere (a game in front), as Discord's do.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8};
#[cfg(windows)]
use std::sync::atomic::Ordering;

use egui::{Key, Modifiers};

use crate::settings::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    QuickSwitcher,
    ToggleMute,
    ToggleDeaf,
    ReturnToVoice,
    AnswerCall,
    StartCall,
    Back,
    Forward,
    DmsAndServer,
    PrevChannel,
    NextChannel,
    PrevUnread,
    NextUnread,
    PrevMention,
    NextMention,
    PrevServer,
    NextServer,
    OldestUnread,
    Pins,
    Inbox,
    Members,
    Search,
    Emoji,
    Gif,
    Sticker,
    Upload,
    CreateServer,
    NewGroup,
    Help,
    Shortcuts,
    JumpToCall,
    CopyChannelLink,
    ToggleCategories,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// Every shortcut: its id in the settings file, its name and Discord's
/// default (the bundle's keybinds, as its Ctrl+/ list shows them).
pub const ALL: &[(Shortcut, &str, &str, &str)] = &[
    (Shortcut::ToggleMute, "toggle_mute", n!("Toggle Mute"), "Ctrl+Shift+M"),
    (Shortcut::ToggleDeaf, "toggle_deaf", n!("Toggle Deafen"), "Ctrl+Shift+D"),
    (Shortcut::ReturnToVoice, "return_to_voice", n!("Return to Connected Voice Channel"), "Ctrl+Alt+ArrowLeft"),
    (Shortcut::JumpToCall, "jump_to_call", n!("Navigate to current call"), "Ctrl+Alt+Shift+V"),
    (Shortcut::AnswerCall, "answer_call", n!("Answer Incoming Call"), "Ctrl+Enter"),
    (Shortcut::StartCall, "start_call", n!("Start Call in Conversation"), "Ctrl+Quote"),
    (Shortcut::QuickSwitcher, "quick_switcher", n!("Quick Switcher"), "Ctrl+K"),
    (Shortcut::Back, "back", n!("Navigate Back"), "Alt+ArrowLeft"),
    (Shortcut::Forward, "forward", n!("Navigate Forward"), "Alt+ArrowRight"),
    (Shortcut::PrevServer, "prev_server", n!("Previous Server"), "Ctrl+Alt+ArrowUp"),
    (Shortcut::NextServer, "next_server", n!("Next Server"), "Ctrl+Alt+ArrowDown"),
    (Shortcut::DmsAndServer, "dms_and_server", n!("Toggle Between Last Server and DMs"), "Ctrl+Alt+ArrowRight"),
    (Shortcut::PrevChannel, "prev_channel", n!("Previous Channel"), "Alt+ArrowUp"),
    (Shortcut::NextChannel, "next_channel", n!("Next Channel"), "Alt+ArrowDown"),
    (Shortcut::PrevUnread, "prev_unread", n!("Previous Unread Channel"), "Alt+Shift+ArrowUp"),
    (Shortcut::NextUnread, "next_unread", n!("Next Unread Channel"), "Alt+Shift+ArrowDown"),
    (Shortcut::PrevMention, "prev_mention", n!("Previous Channel with Mentions"), "Ctrl+Alt+Shift+ArrowUp"),
    (Shortcut::NextMention, "next_mention", n!("Next Channel with Mentions"), "Ctrl+Alt+Shift+ArrowDown"),
    (Shortcut::OldestUnread, "oldest_unread", n!("Jump to Oldest Unread Message"), "Shift+PageUp"),
    (Shortcut::Pins, "pins", n!("Pinned Messages"), "Ctrl+P"),
    (Shortcut::Inbox, "inbox", n!("Inbox"), "Ctrl+I"),
    (Shortcut::Members, "members", n!("Member List"), "Ctrl+U"),
    (Shortcut::Search, "search", n!("Search"), "Ctrl+F"),
    (Shortcut::Emoji, "emoji", n!("Emoji Picker"), "Ctrl+E"),
    (Shortcut::Gif, "gif", n!("GIF Picker"), "Ctrl+G"),
    (Shortcut::Sticker, "sticker", n!("Sticker Picker"), "Ctrl+S"),
    (Shortcut::Upload, "upload", n!("Upload a File"), "Ctrl+Shift+U"),
    (Shortcut::CreateServer, "create_server", n!("Create or Join a Server"), "Ctrl+Shift+N"),
    (Shortcut::NewGroup, "new_group", n!("Create a Private Group"), "Ctrl+Shift+T"),
    (Shortcut::Help, "help", n!("Help"), "Ctrl+Shift+H"),
    (Shortcut::Shortcuts, "shortcuts", n!("Keyboard Shortcuts"), "Ctrl+Slash"),
    (Shortcut::CopyChannelLink, "copy_channel_link", n!("Copy channel link"), "Ctrl+Shift+L"),
    (Shortcut::ToggleCategories, "toggle_categories", n!("Collapse or Expand All Categories"), "Ctrl+Shift+A"),
    (Shortcut::ZoomIn, "zoom_in", n!("Zoom In"), "Ctrl+Plus"),
    (Shortcut::ZoomOut, "zoom_out", n!("Zoom Out"), "Ctrl+Minus"),
    (Shortcut::ZoomReset, "zoom_reset", n!("Reset Zoom"), "Ctrl+0"),
];

/// How the settings page groups them.
pub const SECTIONS: &[(&str, &[Shortcut])] = &[
    (n!("Voice and Calls"), &[Shortcut::ToggleMute, Shortcut::ToggleDeaf, Shortcut::ReturnToVoice, Shortcut::JumpToCall, Shortcut::AnswerCall, Shortcut::StartCall]),
    (
        n!("Navigation"),
        &[
            Shortcut::QuickSwitcher,
            Shortcut::Back,
            Shortcut::Forward,
            Shortcut::PrevServer,
            Shortcut::NextServer,
            Shortcut::DmsAndServer,
            Shortcut::PrevChannel,
            Shortcut::NextChannel,
            Shortcut::PrevUnread,
            Shortcut::NextUnread,
            Shortcut::PrevMention,
            Shortcut::NextMention,
            Shortcut::OldestUnread,
            Shortcut::ToggleCategories,
        ],
    ),
    (n!("Chat and Panels"), &[Shortcut::Pins, Shortcut::Inbox, Shortcut::Members, Shortcut::Search, Shortcut::Emoji, Shortcut::Gif, Shortcut::Sticker, Shortcut::Upload, Shortcut::CopyChannelLink]),
    (n!("Other"), &[Shortcut::CreateServer, Shortcut::NewGroup, Shortcut::Help, Shortcut::Shortcuts, Shortcut::ZoomIn, Shortcut::ZoomOut, Shortcut::ZoomReset]),
];

/// Discord's combos that do not change: shown under the others.
pub const FIXED: &[(&str, &str)] = &[
    ("Enter", n!("Send Message")),
    ("Shift+Enter", n!("New Line")),
    ("↑", n!("Edit Your Last Message (empty box)")),
    ("Esc", n!("Mark Channel as Read, Cancel a Reply or Decline a Call")),
    ("Shift+Esc", n!("Mark Server as Read")),
    ("Ctrl+V", n!("Paste Text or Image")),
    (n!("Mouse Side Buttons"), n!("Back and Forward")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bind {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Bind {
    pub fn parse(text: &str) -> Option<Self> {
        let mut modifiers = Modifiers::NONE;
        let mut key = None;
        for part in text.split('+') {
            match part {
                "Ctrl" => modifiers = modifiers | Modifiers::COMMAND,
                "Shift" => modifiers = modifiers | Modifiers::SHIFT,
                "Alt" => modifiers = modifiers | Modifiers::ALT,
                name => key = Key::from_name(name),
            }
        }
        Some(Self { modifiers, key: key? })
    }

    pub fn text(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.command || self.modifiers.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(self.key.name().to_owned());
        parts.join("+")
    }

    /// As people read it: "Ctrl+Shift+M", arrows as arrows.
    pub fn label(self) -> String {
        let arrow = match self.key {
            Key::ArrowUp => Some("↑"),
            Key::ArrowDown => Some("↓"),
            Key::ArrowLeft => Some("←"),
            Key::ArrowRight => Some("→"),
            Key::Slash => Some("/"),
            Key::Quote => Some("'"),
            Key::PageUp => Some("PgUp"),
            Key::PageDown => Some("PgDn"),
            _ => None,
        };
        match arrow {
            Some(arrow) => {
                let text = self.text();
                let prefix = text.rsplit_once('+').map_or("", |(m, _)| m);
                if prefix.is_empty() { arrow.to_owned() } else { format!("{prefix}+{arrow}") }
            }
            None => self.text(),
        }
    }
}

/// The bind in use for a shortcut: the person's, or the default.
pub fn bind(settings: &Settings, shortcut: Shortcut) -> Option<Bind> {
    let (_, id, _, default) = ALL.iter().find(|(s, ..)| *s == shortcut)?;
    match settings.keybinds.get(*id) {
        Some(text) if text.is_empty() => None,
        Some(text) => Bind::parse(text),
        None => Bind::parse(default),
    }
}

/// Whether this frame pressed the shortcut (consuming the key).
pub fn pressed(ui: &egui::Ui, settings: &Settings, shortcut: Shortcut) -> bool {
    let Some(bind) = bind(settings, shortcut) else { return false };
    ui.input_mut(|i| i.consume_key(bind.modifiers, bind.key))
}

// ---- global mute and deafen ------------------------------------------------------

/// Flags the global watcher raises for the window to act on.
#[derive(Default)]
pub struct Global {
    /// 1 toggle mute, 2 toggle deafen.
    pub pressed: AtomicU8,
    /// While the window has focus egui handles the keys itself.
    pub focused: AtomicBool,
    pub binds: std::sync::Mutex<(Option<Bind>, Option<Bind>)>,
}

#[cfg(windows)]
fn virtual_key(key: Key) -> Option<i32> {
    let name = key.name();
    if name.len() == 1 {
        let c = name.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as i32);
        }
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<i32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x6f + n);
    }
    Some(match key {
        Key::Space => 0x20,
        Key::ArrowUp => 0x26,
        Key::ArrowDown => 0x28,
        Key::ArrowLeft => 0x25,
        Key::ArrowRight => 0x27,
        Key::Insert => 0x2d,
        Key::Delete => 0x2e,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::Backtick => 0xc0,
        _ => return None,
    })
}

/// Ctrl+V pressed since the last look. egui hears a paste only when the
/// clipboard holds text, so a screenshot is pasted by watching the keys.
#[cfg(windows)]
pub fn paste_pressed() -> bool {
    static WAS: AtomicBool = AtomicBool::new(false);
    let down = held(0x11) && held(0x56);
    let was = WAS.swap(down, Ordering::Relaxed);
    down && !was
}

#[cfg(not(windows))]
pub fn paste_pressed() -> bool {
    false
}

#[cfg(windows)]
fn held(vk: i32) -> bool {
    // SAFETY: GetAsyncKeyState only reads the key's state.
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

/// Watches the mute and deafen binds wherever the focus is.
pub fn watch(global: Arc<Global>, wake: crate::backend::Wake) {
    #[cfg(windows)]
    std::thread::Builder::new()
        .name("cordfast-keybinds".into())
        .spawn(move || {
            let combo = |bind: Option<Bind>| -> bool {
                let Some(bind) = bind else { return false };
                let Some(vk) = virtual_key(bind.key) else { return false };
                held(vk)
                    && (bind.modifiers.command || bind.modifiers.ctrl) == held(0x11)
                    && bind.modifiers.shift == held(0x10)
                    && bind.modifiers.alt == held(0x12)
            };
            let mut was = (false, false);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(30));
                if global.focused.load(Ordering::Relaxed) {
                    was = (false, false);
                    continue;
                }
                let (mute, deaf) = *global.binds.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let now = (combo(mute), combo(deaf));
                if now.0 && !was.0 {
                    global.pressed.store(1, Ordering::Relaxed);
                    wake();
                }
                if now.1 && !was.1 {
                    global.pressed.store(2, Ordering::Relaxed);
                    wake();
                }
                was = now;
            }
        })
        .expect("unable to start the keybind thread");
    #[cfg(not(windows))]
    {
        let _ = (global, wake);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_round_trip() {
        let bind = Bind::parse("Ctrl+Shift+M").unwrap();
        assert!(bind.modifiers.command && bind.modifiers.shift && bind.key == Key::M);
        assert_eq!(bind.text(), "Ctrl+Shift+M");
        assert_eq!(Bind::parse("Alt+ArrowUp").unwrap().label(), "Alt+↑");
        assert_eq!(Bind::parse("Ctrl+Slash").unwrap().label(), "Ctrl+/");
        assert_eq!(Bind::parse("Ctrl+Alt+Shift+ArrowDown").unwrap().label(), "Ctrl+Alt+Shift+↓");
    }

    #[test]
    fn every_default_parses_and_every_shortcut_has_a_section() {
        for (shortcut, id, _, default) in ALL {
            assert!(Bind::parse(default).is_some(), "{id}: {default}");
            assert!(SECTIONS.iter().any(|(_, list)| list.contains(shortcut)), "{id} has no section");
        }
    }
}
