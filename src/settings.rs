//! Preferences, saved as JSON in the config folder
//! (`%APPDATA%\CordFast\config\settings.json` on Windows).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Whether this is the demo session (`CORDFAST_DEMO`).
fn demo() -> bool {
    std::env::var_os("CORDFAST_DEMO").is_some()
}

pub fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "CordFast")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    /// The interface's language (a tag of `i18n::ALL`); none follows the
    /// system's until one is picked, as Discord starts in it.
    pub language: Option<String>,
    pub accent: Option<[u8; 3]>,
    pub zoom: f32,
    pub compact: bool,
    pub show_avatars: bool,
    pub keep_running_in_tray: bool,
    /// The system's title bar and frame instead of Discord's.
    pub native_titlebar: bool,
    pub notifications: bool,
    pub notify_dms: bool,
    pub notify_mentions: bool,
    pub notify_sound: bool,
    pub notify_preview: bool,
    pub plugins: BTreeMap<String, bool>,
    // Voice.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub input_volume: f32,
    pub output_volume: f32,
    /// Microphone level, in dBFS, under which nothing is sent.
    pub input_sensitivity: f32,
    pub automatic_sensitivity: bool,
    pub noise_suppression: bool,
    /// Takes out of the microphone what the speakers play (on in Discord).
    pub echo_cancellation: bool,
    /// Brings the voice to a steady level (on in Discord).
    pub automatic_gain_control: bool,
    /// With the automatic sensitivity, voice detection opens the
    /// microphone instead of its level alone.
    pub advanced_voice_activity: bool,
    /// Asks before leaving one voice channel for another.
    pub switch_voice_warning: bool,
    pub push_to_talk: bool,
    pub push_to_talk_key: u32,
    pub user_volumes: BTreeMap<String, f32>,
    /// Servers whose muted channels are hidden ("Hide Muted Channels").
    pub hide_muted: Vec<String>,
    /// The folder the last file was picked from, where the next dialog opens.
    pub last_folder: Option<String>,
    /// The channel list's width, dragged at its edge as in Discord.
    pub sidebar_width: f32,
    /// Shortcuts changed from their defaults: id → "Ctrl+Shift+M" ("" for none).
    pub keybinds: BTreeMap<String, String>,
    /// Discord's sounds switched off in Settings (their asset names:
    /// "message1", "user_join"…).
    pub sounds_off: Vec<String>,
    /// The composer's emoji, GIF and sticker buttons (Appearance → Chat
    /// Box).
    pub emoji_button: bool,
    pub gif_button: bool,
    pub sticker_button: bool,
    /// Role colours: 0 in names, 1 next to names, 2 not shown.
    pub role_colors: u8,
    /// The emoji picker's skin tone: 0 none, 1 light to 5 dark.
    pub skin_tone: u8,
    /// The expression picker's sections hidden by clicking their name
    /// (a server's id, or a category's).
    pub collapsed_expressions: Vec<String>,
    /// Accounts signed in on this computer (their tokens are in the
    /// credential store), for switching as Discord does.
    pub accounts: Vec<Account>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub username: String,
    pub avatar: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "Discord Escuro".into(),
            language: None,
            accent: None,
            zoom: 1.0,
            compact: false,
            show_avatars: true,
            keep_running_in_tray: true,
            native_titlebar: false,
            notifications: true,
            notify_dms: true,
            notify_mentions: true,
            notify_sound: true,
            notify_preview: true,
            plugins: BTreeMap::new(),
            input_device: None,
            output_device: None,
            input_volume: 1.0,
            output_volume: 1.0,
            input_sensitivity: -50.0,
            automatic_sensitivity: true,
            noise_suppression: true,
            echo_cancellation: true,
            automatic_gain_control: true,
            advanced_voice_activity: true,
            switch_voice_warning: false,
            push_to_talk: false,
            push_to_talk_key: 0x05,
            user_volumes: BTreeMap::new(),
            hide_muted: Vec::new(),
            last_folder: None,
            sidebar_width: 240.0,
            keybinds: BTreeMap::new(),
            sounds_off: Vec::new(),
            emoji_button: true,
            gif_button: true,
            sticker_button: true,
            role_colors: 0,
            skin_tone: 0,
            collapsed_expressions: Vec::new(),
            accounts: Vec::new(),
        }
    }
}

impl Settings {
    /// A file dialog that opens where the last file came from, or in the
    /// pictures folder, never in the program's folder the shortcut starts
    /// it in.
    pub fn file_dialog(&self) -> rfd::FileDialog {
        let folder = self.last_folder.as_ref().map(PathBuf::from).filter(|p| p.is_dir()).or_else(|| {
            let dirs = directories::UserDirs::new()?;
            Some(dirs.picture_dir().unwrap_or(dirs.home_dir()).to_path_buf())
        });
        match folder {
            Some(folder) => rfd::FileDialog::new().set_directory(folder),
            None => rfd::FileDialog::new(),
        }
    }

    /// Remembers the folder of a picked file for the next dialog.
    pub fn picked(&mut self, file: &std::path::Path) {
        if let Some(folder) = file.parent() {
            self.last_folder = Some(folder.to_string_lossy().into_owned());
            self.save();
        }
    }

    fn path() -> PathBuf {
        config_dir().join("settings.json")
    }

    pub fn load() -> Self {
        // The demo session starts from the defaults, in English, and leaves
        // the real settings alone (screenshots look the same everywhere).
        if demo() {
            return Self { language: Some("en-US".into()), ..Self::default() };
        }
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes through a temporary file, so a crash never leaves half a file.
    pub fn save(&self) {
        if demo() {
            return;
        }
        let path = Self::path();
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        let result = std::fs::create_dir_all(config_dir()).and_then(|()| {
            let temp = path.with_extension("json.tmp");
            std::fs::write(&temp, text)?;
            std::fs::rename(&temp, &path)
        });
        if let Err(error) = result {
            log::warn!("settings: {error}");
        }
    }

    pub fn plugin(&self, plugin: crate::plugins::Plugin) -> bool {
        let info = plugin.info();
        info.required || self.plugins.get(info.id).copied().unwrap_or(info.default)
    }
}
