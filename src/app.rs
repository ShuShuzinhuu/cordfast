//! Application state. Views in `ui` draw it and push [`Action`]s, which are
//! applied after the frame; events from the runtime are applied before it.
//! The state outlives windows: closing to the tray keeps it in the shell.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::backend::{Auth, Backend, Command, Commands, Event};
use crate::discord::gateway::Dispatch;
use crate::discord::model::{
    Channel, ChannelKind, Folders, Guild, Id, Member, Message, Mutes, ReadState, Relationship,
    User, now_snowflake, sf,
};
use crate::discord::rest::Profile;
use crate::images::Images;
use crate::plugins::Plugin;
use crate::settings::Settings;
use crate::voice::{VoiceCommand, VoiceEvent};

/// Messages kept for a channel that is not on screen.
const KEEP_HIDDEN: usize = 50;
/// Messages kept for the open channel.
const KEEP_OPEN: usize = 500;

/// The password form's state, beside the QR code.
#[derive(Default)]
pub struct LoginForm {
    pub login: String,
    pub password: String,
    pub busy: bool,
    pub error: Option<String>,
    /// After the password: the second factor's kinds, the one in use, the code.
    pub mfa: Option<(bool, bool, bool)>,
    pub mfa_kind: &'static str,
    pub code: String,
    pub sms_phone: Option<String>,
}

pub enum AuthView {
    Checking,
    Generating,
    Code { modules: Vec<bool>, width: usize },
    Scanned(String),
    Failed(String),
    LoggedIn,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Default)]
pub struct History {
    /// Oldest first.
    pub messages: Vec<Message>,
    pub loading: bool,
    pub complete: bool,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub struct ProfileDraft {
    pub display_name: String,
    pub pronouns: String,
    pub bio: String,
}

/// The user settings' pages, in Discord's sidebar order (since its
/// redesign), then CordFast's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Profile,
    Account,
    DataPrivacy,
    MessagingPermissions,
    Notifications,
    Clips,
    Nitro,
    ServerBoost,
    Subscriptions,
    GiftInventory,
    Billing,
    Voice,
    Appearance,
    Accessibility,
    System,
    Language,
    RegisteredGames,
    ActivityPrivacy,
    Overlay,
    ConnectedApps,
    Developer,
    Plugins,
    About,
}

pub enum ProfileState {
    Loading,
    Ready(Box<Profile>),
    Failed(String),
}

pub struct ProfileView {
    pub user: User,
    pub guild: Option<Id>,
    pub state: ProfileState,
    pub at: egui::Pos2,
    /// Opened from the account panel: the card's bottom sits just over
    /// `at`, its left edge on it, as Discord opens your own.
    pub above: bool,
}

#[derive(Default)]
pub struct VoiceUi {
    /// The channel we are in, or joining.
    pub channel: Option<(Option<Id>, Id)>,
    pub connected: bool,
    pub since: Option<Instant>,
    pub speaking: HashSet<Id>,
    pub mic_level: f32,
    pub regions: Vec<(String, u32)>,
    pub mute: bool,
    pub deaf: bool,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub struct Call {
    pub ringing: Vec<Id>,
    pub region: Option<String>,
}

/// The open channel's member list, row by row as the gateway keeps it.
#[derive(Default)]
pub struct MemberList {
    pub guild: Id,
    pub channel: Id,
    pub list: String,
    pub rows: Vec<Option<crate::discord::gateway::ListItem>>,
    pub online: u64,
    pub total: u64,
    pub requested: u64,
    /// Each section's size, from the update's group list.
    pub groups: HashMap<String, u64>,
}

#[derive(Clone)]
pub struct Gif {
    /// What gets sent: the GIF page, which Discord embeds.
    pub url: String,
    pub preview: String,
    pub width: u32,
    pub height: u32,
}

/// The GIF tab: Discord's front page (its trending categories), the
/// trending GIFs, or a search.
#[derive(Default)]
pub struct GifPicker {
    pub query: String,
    /// What was last asked for ("" for the trending GIFs); `None` while
    /// on the front page.
    pub asked: Option<String>,
    /// When the search box last changed, to search once typing pauses.
    pub typed_at: Option<f64>,
    pub results: Option<Vec<Gif>>,
    /// The front page: categories (name, preview) and the preview of the
    /// trending GIFs; asked for once.
    pub trending: Option<(Vec<(String, String)>, Option<String>)>,
    pub asked_trending: bool,
    /// The "Trending GIFs" page is open.
    pub trending_open: bool,
}

/// The expression picker's tabs, in Discord's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionTab {
    Gifs,
    Stickers,
    Emoji,
}

/// Discord's expression picker: GIFs, stickers and emoji in one popout
/// over the composer (emoji alone for a reaction).
pub struct Expressions {
    pub tab: ExpressionTab,
    pub target: PickerTarget,
    pub emoji_search: String,
    pub sticker_search: String,
    pub gifs: GifPicker,
    /// Where the list should scroll to, from the category rail.
    pub jump: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickerTarget {
    Composer,
    Reaction { channel: Id, message: Id },
}

/// The panel that replaces the member list: pinned messages or a search.
pub enum Side {
    Pins(Option<Vec<Message>>),
    Search { query: String, results: Option<(u64, Vec<Message>)> },
}

/// Where the window is: a server and its channel, a DM, or the friends
/// page (neither). The title bar's arrows go back and forward through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub guild: Option<Id>,
    pub channel: Option<Id>,
}

/// The inbox under the title bar's button, as Discord's: the channels with
/// unread messages, and recent mentions everywhere.
pub struct Inbox {
    pub tab: InboxTab,
    pub mentions: Option<Vec<Message>>,
}

/// Where a dragged channel lands among its new siblings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Before(Id),
    After(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxTab {
    Unread,
    Mentions,
}

/// A small dialog over everything.
pub enum Modal {
    /// Join with an invite, or create a server.
    AddServer { invite: String, name: String },
    NewThread { channel: Id, message: Id, name: String },
    /// Pick friends for a DM or a group (up to nine).
    NewDm { picked: Vec<Id>, filter: String },
    /// Set a custom status: text and how long it lasts (minutes).
    CustomStatus { text: String, minutes: Option<u64> },
    /// Discord's "Are you sure?" before deleting, kicking or banning.
    Confirm { title: String, body: String, button: String, then: Confirmed },
    /// "Change Nickname": your own or, with the permission, someone's.
    Nickname { guild: Id, user: Id, nick: String },
    /// "Add Note": a note on someone only you see.
    Note { user: Id, name: String, note: String },
    /// "Forward": a message to up to five conversations, with an
    /// optional note after it.
    Forward { channel: Id, guild: Option<Id>, message: Id, picked: Vec<Id>, filter: String, note: String },
    /// "Set a channel status" for the voice channel you are in.
    VoiceStatus { channel: Id, status: String },
    /// An upload card's pencil: the file's name, description and spoiler.
    Attachment { index: usize, upload: crate::backend::Upload },
}

/// What a confirmation does once confirmed.
#[derive(Clone, Copy)]
pub enum Confirmed {
    DeleteMessage { channel: Id, id: Id },
    Kick { guild: Id, user: Id },
    Ban { guild: Id, user: Id },
    RemoveFriend { user: Id },
    DeleteRole { guild: Id, role: Id },
    DeleteChannel { channel: Id },
    /// Leaving the voice channel we are in for another.
    SwitchVoice { guild: Option<Id>, channel: Id },
    /// Settings → Log Out.
    Logout,
}

impl Confirmed {
    /// Whether the button warns (red) or just goes ahead (the brand colour).
    pub fn danger(&self) -> bool {
        !matches!(self, Self::SwitchVoice { .. })
    }

    pub fn run(self, app: &mut App) {
        let api = |key: &str, method: &'static str, path: String, body: Option<serde_json::Value>| Command::Api { key: key.into(), method, path, body };
        match self {
            Self::DeleteMessage { channel, id } => app.actions.push(Action::Delete { channel, id }),
            Self::Kick { guild, user } => app.commands.send(api("kicked", "DELETE", format!("/guilds/{guild}/members/{user}"), None)),
            Self::Ban { guild, user } => app.commands.send(api("banned", "PUT", format!("/guilds/{guild}/bans/{user}"), Some(serde_json::json!({ "delete_message_seconds": 0 })))),
            Self::RemoveFriend { user } => app.actions.push(Action::Relationship { user, kind: None }),
            Self::SwitchVoice { guild, channel } => app.commands.send(Command::Call { guild, channel, ring: false }),
            Self::Logout => app.actions.push(Action::Logout),
            Self::DeleteRole { guild, role } => {
                app.commands.send(crate::admin::change("DELETE", format!("/guilds/{guild}/roles/{role}"), None, &format!("roles:{}", t!("Role deleted."))));
                if let Some(admin) = &mut app.admin {
                    admin.role = None;
                    admin.role_saved = None;
                }
            }
            Self::DeleteChannel { channel } => {
                app.commands.send(crate::admin::change("DELETE", format!("/channels/{channel}"), None, &format!("channels:{}", t!("Channel deleted."))));
                if let Some(admin) = &mut app.admin {
                    admin.channel = None;
                }
            }
        }
    }
}

/// A message being jumped to: lit up for a moment, as Discord does.
pub struct Jump {
    pub channel: Id,
    pub message: Id,
    pub at: Instant,
    /// Brought into view already.
    pub scrolled: bool,
    /// Older pages asked for while looking for it.
    pub pages: u8,
}

/// A server tag's card: which server, opened beside the tag.
pub struct TagCard {
    pub guild: Id,
    pub at: egui::Pos2,
}

pub struct Viewer {
    pub url: String,
    pub original: String,
    pub size: egui::Vec2,
    /// 1 fits the window; more is a close-up.
    pub zoom: f32,
    pub offset: egui::Vec2,
    /// Who posted it, for the viewer's top bar and its Forward.
    pub from: Option<ViewerSource>,
}

/// Who posted a picture open in the viewer, and where.
pub struct ViewerSource {
    pub author: User,
    pub name: String,
    pub at: u64,
    pub channel: Id,
    pub guild: Option<Id>,
    pub message: Id,
}

pub enum Action {
    SelectGuild(Option<Id>),
    SelectChannel(Id),
    LoadOlder(Id),
    Send,
    Reply { id: Id, author: String },
    StartEdit { id: Id, content: String },
    SaveEdit,
    CancelEdit,
    /// Opens a DM or group with these users.
    NewDm(Vec<Id>),
    Delete { channel: Id, id: Id },
    React { channel: Id, message: Id, emoji: String, add: bool },
    CancelCompose,
    Logout,
    NewQr,
    TokenLogin,
    PasswordLogin,
    MfaCode,
    MfaKind(&'static str),
    OpenProfile { user: User, guild: Option<Id>, at: egui::Pos2 },
    /// Your own profile from the account panel, opened above `at`.
    OpenOwnProfile { at: egui::Pos2 },
    /// A server tag clicked: its server's card beside `at`.
    OpenTagCard { guild: Id, at: egui::Pos2 },
    /// The card's Join: a server with a public profile, joined directly.
    JoinTagGuild(Id),
    MessageUser(Id),
    /// The title bar's arrows (and the mouse's side buttons).
    Back,
    Forward,
    /// The box at the bottom of someone's profile: opens the DM and sends.
    SendDm { user: Id, text: String },
    CallUser(Id),
    JoinVoice { guild: Option<Id>, channel: Id, ring: bool },
    LeaveVoice,
    ToggleMute,
    ToggleDeaf,
    AcceptCall(Id),
    DeclineCall(Id),
    SetRegion(String),
    OpenSettings(SettingsTab),
    CloseSettings,
    SettingsChanged,
    AttachFiles,
    OpenForum(Id),
    /// Marks these channels read up to their latest message.
    MarkRead(Vec<Id>),
    /// "Mark Unread": unread from this message on.
    MarkUnread { channel: Id, message: Id },
    /// Brings a message into view and lights it up, loading older pages
    /// until it arrives.
    JumpTo { channel: Id, message: Id },
    /// Mutes for so many seconds (-1: until unmuted), or unmutes (None).
    Mute { guild: Option<Id>, channel: Option<Id>, seconds: Option<i64> },
    /// "Notification Settings": 0 every message, 1 @mentions,
    /// 2 nothing, 3 (a channel) as its category or server.
    NotifyLevel { guild: Option<Id>, channel: Option<Id>, level: u8 },
    CloseDm(Id),
    LeaveGuild(Id),
    OpenGuildSettings(Id),
    /// A channel dragged to another place: its category (none for the
    /// top) and where among the channels there.
    MoveChannel { guild: Id, channel: Id, parent: Option<Id>, anchor: Anchor },
    /// A category dragged before another one, or to the end.
    MoveCategory { guild: Id, category: Id, before: Option<Id> },
    /// A status for `minutes`, or for good.
    SetStatus(&'static str, Option<u64>),
    SetCustomStatus { text: Option<String>, minutes: Option<u64> },
    OpenFriends,
    AddFriend,
    /// `Some(1)` accepts, `Some(2)` blocks, `None` removes.
    Relationship { user: Id, kind: Option<u64> },
    GuildTab(crate::admin::Tab),
    CloseGuildSettings,
    SwitchAccount(Id),
    AddAccount,
    RemoveAccount(String),
    /// Restarts into the downloaded update.
    InstallUpdate,
}

pub struct App {
    pub commands: Commands,
    backend: Backend,
    pub images: Images,
    pub settings: Settings,
    pub palettes: Vec<(String, crate::theme::Palette)>,
    theme_dirty: bool,
    pub auth: AuthView,
    pub connection: Connection,
    pub me: Option<User>,
    pub guilds: Vec<Guild>,
    pub dms: Vec<Channel>,
    pub read: HashMap<Id, ReadState>,
    pub latest: HashMap<Id, Id>,
    pub selected_guild: Option<Id>,
    pub selected_channel: Option<Id>,
    remembered: HashMap<Option<Id>, Id>,
    pub histories: HashMap<Id, History>,
    pub users: HashMap<Id, User>,
    pub members: HashMap<(Id, Id), Member>,
    requested_members: HashSet<(Id, Id)>,
    pub member_requests: Vec<(Id, Id)>,
    pub typing: HashMap<Id, Vec<(Id, Instant)>>,
    subscribed: HashSet<Id>,
    pub folders: Option<Folders>,
    pub open_folders: HashSet<i64>,
    pub mutes: Mutes,
    pub relationships: HashMap<Id, Relationship>,
    pub composer: String,
    pub replying: Option<(Id, String)>,
    pub editing: Option<Id>,
    pub edit_text: String,
    pub focus_edit: bool,
    pub files: Vec<crate::backend::Upload>,
    last_typing: Option<Instant>,
    pub token_input: String,
    pub login_form: LoginForm,
    pub notice: Option<(String, Instant)>,
    /// Discord's bar over the app after the call went on without us, with
    /// "Reconnect" to the channel it was in.
    pub voice_notice: Option<VoiceNotice>,
    pub actions: Vec<Action>,
    pub focus_composer: bool,
    pub page: Option<SettingsTab>,
    /// What the settings pages asked the API for: none while it loads.
    pub settings_data: HashMap<&'static str, Option<Result<serde_json::Value, String>>>,
    /// The settings sidebar's search.
    pub settings_search: String,
    pub admin: Option<crate::admin::GuildAdmin>,
    pub profile: Option<ProfileView>,
    pub viewer: Option<Viewer>,
    pub forums: HashMap<Id, Option<Vec<(Channel, Option<Message>)>>>,
    pub voice: VoiceUi,
    pub mic_test: bool,
    pub dm_voice: HashMap<Id, Id>,
    /// (muted, deafened) for everyone in voice.
    pub voice_flags: HashMap<Id, (bool, bool)>,
    pub calls: HashMap<Id, Call>,
    pub activity: Option<String>,
    /// Your own activities in full (what the pipe and process detection
    /// report), for your profile.
    pub own_activities: Vec<crate::discord::gateway::Activity>,
    pub presences: HashMap<Id, crate::discord::gateway::Presence>,
    /// Profiles fetched for the DM side panel.
    pub profiles: HashMap<Id, Box<Profile>>,
    /// Applications' icons for activity cards: requested, then the URL or none.
    pub app_icons: HashMap<Id, Option<String>>,
    /// Applications' names, learned with their icons.
    pub app_names: HashMap<Id, String>,
    pub tag_card: Option<TagCard>,
    /// The loading screen: since when, and which tip it shows.
    pub loading: Option<(Instant, usize)>,
    /// Servers' public cards: requested (none yet), then the card or why not.
    pub guild_profiles: HashMap<Id, Option<Result<crate::discord::model::GuildProfile, String>>>,
    pub show_dm_profile: bool,
    pub requested_profiles: HashSet<Id>,
    pub status: String,
    /// When a timed status ends and we are back online.
    pub status_until: Option<u64>,
    pub custom_status: Option<String>,
    pub friends_tab: u8,
    pub friend_input: String,
    /// The account's settings, as Discord keeps them for every client.
    pub preloaded: crate::discord::preloaded::Preloaded,
    /// The expression picker, while open.
    pub expressions: Option<Expressions>,
    /// When a click outside closed it, and on which tab: the button that
    /// was clicked does not open it again.
    pub expressions_closed: Option<(f64, ExpressionTab)>,
    /// Frequently used and favourite emoji and stickers (settings-proto 2):
    /// asked for when the picker first opens.
    pub frecency: Option<crate::discord::frecency::Frecency>,
    frecency_asked: bool,
    pub member_list: Option<MemberList>,
    pub side: Option<Side>,
    pub inbox: Option<Inbox>,
    /// Places left behind and, after going back, ahead; and the last place
    /// seen, to notice a move.
    pub back: Vec<Place>,
    pub forward: Vec<Place>,
    place: Option<Place>,
    /// Collapsed channel categories.
    pub collapsed: HashSet<Id>,
    /// Text to put on the clipboard next frame (from an API answer).
    pub clipboard: Option<String>,
    /// The profile being edited in Settings → Perfis.
    pub profile_draft: Option<ProfileDraft>,
    /// Where each channel's unread messages began when it was opened,
    /// for the red "NEW" line.
    pub unread_from: HashMap<Id, Id>,
    pub modal: Option<Modal>,
    pub show_members: bool,
    /// What autocomplete put in the composer and what it stands for:
    /// ("@Ana", "<@123>"), (":pepe:", "<:pepe:456>").
    pub mention_tokens: Vec<(String, String)>,
    pub heights: HashMap<Id, (f32, f32)>,
    /// Loading older messages: (channel, distance from the bottom, arrived).
    pub scroll_anchor: Option<(Id, f32, bool)>,
    /// A scroll position to apply on the next frame.
    pub scroll_to: Option<(Id, f32)>,
    /// A message to bring into view and light up (clicking a reply).
    pub jump: Option<Jump>,
    /// Text typed in a profile's box, sent once the DM it opens arrives.
    pub pending_dm: Option<(Id, String)>,
    pub revealed: HashSet<(Id, usize)>,
    pub switcher: Option<String>,
    pub devices: (Vec<String>, Vec<String>),
    // Window and tray.
    pub focused: bool,
    pub has_window: bool,
    pub quit: bool,
    pub hide: bool,
    pub show: bool,
    pub tray: Option<fastframe_tray::Tray>,
    /// What Windows starts at login, read and written off this thread.
    pub autostart: std::sync::Arc<std::sync::Mutex<Option<fastframe_autostart::State>>>,
    pub updates: crate::updates::Shared,
    pub keybinds: std::sync::Arc<crate::keybinds::Global>,
    /// The shortcut whose new keys Settings is waiting for.
    pub recording: Option<&'static str>,
    /// Each place's slash commands (a server's, or a conversation's); none
    /// while they load.
    pub command_index: HashMap<Id, Option<crate::discord::commands::Index>>,
    /// The slash command being filled in the composer.
    pub slash: Option<SlashDraft>,
    /// The command picker's highlighted row.
    pub slash_pick: usize,
    /// The nonces of the last commands sent, to tell their failures.
    pub sent_commands: Vec<String>,
}

/// Why the call ended without us, and where it was.
#[derive(Clone, Copy)]
pub struct VoiceNotice {
    /// Another device of the account took it.
    pub elsewhere: bool,
    pub guild: Option<Id>,
    pub channel: Id,
}

/// A slash command picked in the composer, its options being filled.
pub struct SlashDraft {
    pub channel: Id,
    /// Where its index is kept: the server, or the conversation.
    pub key: Id,
    pub entry: usize,
    pub fields: Vec<SlashField>,
    pub focus: usize,
    /// Focus the field once drawn.
    pub refocus: bool,
    /// The application's suggestions for the focused field.
    pub suggestions: Vec<(String, serde_json::Value)>,
    /// The last suggestions asked: their nonce, field and text.
    pub asked: Option<(String, usize, String)>,
    /// When the focused field last changed, to ask once typing pauses.
    pub typed_at: Option<Instant>,
}

/// One option of the command being filled: what was written and, for a
/// pick (a choice, a user, a role, a channel), the value it stands for.
pub struct SlashField {
    pub option: crate::discord::commands::CommandOption,
    pub text: String,
    pub value: Option<serde_json::Value>,
}

impl App {
    pub fn new(wake: crate::backend::Wake) -> Self {
        let settings = Settings::load();
        crate::i18n::apply(settings.language.as_deref());
        crate::discord::rest::NO_TRACK.store(
            settings.plugin(Plugin::NoTrack),
            std::sync::atomic::Ordering::Relaxed,
        );
        let backend = Backend::spawn(wake, crate::icon::png(256));
        let commands = backend.commands.clone();
        commands.send(Command::RichPresence(settings.plugins.get("rich-presence").copied().unwrap_or(true)));
        Self {
            images: Images::new(commands.clone()),
            commands,
            backend,
            settings,
            palettes: crate::theme::catalog(),
            theme_dirty: true,
            auth: AuthView::Checking,
            connection: Connection::Connecting,
            me: None,
            guilds: Vec::new(),
            dms: Vec::new(),
            read: HashMap::new(),
            latest: HashMap::new(),
            selected_guild: None,
            selected_channel: None,
            remembered: HashMap::new(),
            histories: HashMap::new(),
            users: HashMap::new(),
            members: HashMap::new(),
            requested_members: HashSet::new(),
            member_requests: Vec::new(),
            typing: HashMap::new(),
            subscribed: HashSet::new(),
            folders: None,
            open_folders: HashSet::new(),
            mutes: Mutes::default(),
            relationships: HashMap::new(),
            composer: String::new(),
            replying: None,
            editing: None,
            edit_text: String::new(),
            focus_edit: false,
            files: Vec::new(),
            last_typing: None,
            token_input: String::new(),
            login_form: LoginForm::default(),
            notice: None,
            voice_notice: None,
            actions: Vec::new(),
            focus_composer: false,
            page: None,
            admin: None,
            profile: None,
            viewer: None,
            forums: HashMap::new(),
            voice: VoiceUi::default(),
            mic_test: false,
            dm_voice: HashMap::new(),
            voice_flags: HashMap::new(),
            calls: HashMap::new(),
            activity: None,
            presences: HashMap::new(),
            own_activities: Vec::new(),
            profiles: HashMap::new(),
            app_icons: HashMap::new(),
            app_names: HashMap::new(),
            tag_card: None,
            loading: None,
            guild_profiles: HashMap::new(),
            show_dm_profile: true,
            requested_profiles: HashSet::new(),
            status: "online".into(),
            status_until: None,
            custom_status: None,
            friends_tab: 0,
            friend_input: String::new(),
            preloaded: Default::default(),
            settings_data: HashMap::new(),
            settings_search: String::new(),
            expressions: None,
            expressions_closed: None,
            frecency: None,
            frecency_asked: false,
            member_list: None,
            side: None,
            inbox: None,
            back: Vec::new(),
            forward: Vec::new(),
            place: None,
            collapsed: HashSet::new(),
            clipboard: None,
            profile_draft: None,
            unread_from: HashMap::new(),
            modal: None,
            show_members: true,
            mention_tokens: Vec::new(),
            heights: HashMap::new(),
            scroll_anchor: None,
            scroll_to: None,
            jump: None,
            pending_dm: None,
            revealed: HashSet::new(),
            switcher: None,
            devices: (Vec::new(), Vec::new()),
            focused: true,
            has_window: false,
            quit: false,
            hide: false,
            show: false,
            tray: None,
            autostart: std::sync::Arc::default(),
            updates: crate::updates::Shared::default(),
            keybinds: std::sync::Arc::default(),
            recording: None,
            command_index: HashMap::new(),
            slash: None,
            slash_pick: 0,
            sent_commands: Vec::new(),
        }
    }

    /// Hands the mute and deafen binds to the global watcher.
    pub fn sync_global_keybinds(&self) {
        use crate::keybinds::{Shortcut, bind};
        *self.keybinds.binds.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            (bind(&self.settings, Shortcut::ToggleMute), bind(&self.settings, Shortcut::ToggleDeaf));
    }

    fn autostart_entry() -> fastframe_autostart::Autostart {
        fastframe_autostart::Autostart::new("cordfast", "CordFast")
    }

    /// Reads what starts at login, in the background.
    pub fn refresh_autostart(&self) {
        let slot = self.autostart.clone();
        std::thread::spawn(move || {
            let state = Self::autostart_entry().state();
            *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(state);
        });
    }

    /// Starts at login with the window, in the tray, or not at all.
    pub fn set_autostart(&self, start: Option<fastframe_autostart::Start>) {
        let slot = self.autostart.clone();
        std::thread::spawn(move || {
            let state = Self::autostart_entry().set(start);
            match state {
                Ok(state) => *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(state),
                Err(error) => log::warn!("autostart: {error}"),
            }
        });
    }

    /// Plays one of Discord's sounds through the chosen output, unless
    /// deafened (Discord keeps the deafen and undeafen ones).
    fn sound(&self, sound: crate::sounds::Sound) {
        use crate::sounds::Sound;
        if self.voice.deaf && !matches!(sound, Sound::Deafen | Sound::Undeafen) {
            return;
        }
        if self.settings.sounds_off.iter().any(|s| s == sound.name()) {
            return;
        }
        crate::sounds::play(sound, self.settings.output_device.clone(), (self.settings.output_volume * 0.6).min(1.0));
    }

    /// Opens the expression picker on `tab`, or closes it when that tab
    /// is already showing, as Discord's buttons do. `now` is the input's
    /// time, to tell the click that just closed it from outside.
    pub fn toggle_expressions(&mut self, tab: ExpressionTab, target: PickerTarget, now: f64) {
        if let Some(open) = &mut self.expressions {
            if open.tab == tab && open.target == target {
                self.expressions = None;
            } else {
                open.tab = tab;
                open.target = target;
                open.jump = None;
            }
            return;
        }
        if self.expressions_closed.is_some_and(|(at, closed)| closed == tab && now - at < 0.4) {
            self.expressions_closed = None;
            return;
        }
        self.expressions = Some(Expressions { tab, target, emoji_search: String::new(), sticker_search: String::new(), gifs: GifPicker::default(), jump: None });
        if !self.frecency_asked {
            self.frecency_asked = true;
            self.commands.send(crate::admin::get("frecency", "/users/@me/settings-proto/2".into()));
        }
    }

    /// Changes one of the account's settings here and on Discord.
    pub fn set_preloaded(&mut self, key: crate::discord::preloaded::Key, value: u64) {
        let patch = self.preloaded.set_number(key, value);
        self.commands.send(Command::SaveSettings(patch));
    }

    pub fn set_preloaded_text(&mut self, key: crate::discord::preloaded::Key, value: &str) {
        let patch = self.preloaded.set_text(key, value);
        self.commands.send(Command::SaveSettings(patch));
    }

    pub fn rich_presence(&self) -> bool {
        self.settings.plugins.get("rich-presence").copied().unwrap_or(true)
    }

    pub fn plugin(&self, plugin: Plugin) -> bool {
        self.settings.plugin(plugin)
    }

    /// A new window: fonts, emoji and the theme go into its context.
    pub fn attach(&mut self, ctx: &egui::Context) {
        crate::theme::install(ctx);
        self.has_window = true;
        self.theme_dirty = true;
        self.apply_theme(ctx);
    }

    pub fn window_gone(&mut self) {
        self.images.clear();
        self.has_window = false;
        self.hide = false;
        self.show = false;
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.theme_dirty) {
            return;
        }
        let palette = self
            .palettes
            .iter()
            .find(|(name, _)| *name == self.settings.theme)
            .map_or(crate::theme::DISCORD_DARK, |(_, p)| *p);
        crate::theme::apply(ctx, palette, self.settings.accent);
        // Zoom keys are rebindable shortcuts here, not egui's own.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        ctx.set_zoom_factor(self.settings.zoom.clamp(0.6, 2.0));
    }

    pub fn me_id(&self) -> Id {
        self.me.as_ref().map_or(0, |u| u.id)
    }

    pub fn channel(&self, id: Id) -> Option<&Channel> {
        self.dms.iter().find(|c| c.id == id).or_else(|| {
            self.guilds
                .iter()
                .flat_map(|g| g.channels.iter())
                .find(|c| c.id == id)
        })
    }

    pub fn guild(&self, id: Id) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == id)
    }

    /// How much a guild channel notifies: its own setting, its category's,
    /// the server's, or the server's default.
    pub fn notify_level(&self, channel: &Channel) -> u8 {
        let levels = &self.mutes.levels;
        levels
            .get(&channel.id)
            .or_else(|| channel.parent_id.and_then(|p| levels.get(&p)))
            .or_else(|| channel.guild_id.and_then(|g| levels.get(&g)))
            .copied()
            .unwrap_or_else(|| channel.guild_id.and_then(|g| self.guild(g)).map_or(1, |g| g.default_notifications))
    }

    pub fn is_muted(&self, channel: &Channel) -> bool {
        self.mutes.channels.contains(&channel.id)
            || channel.parent_id.is_some_and(|p| self.mutes.channels.contains(&p))
            || channel.guild_id.is_some_and(|g| self.mutes.guilds.contains(&g))
    }

    pub fn is_unread(&self, channel: Id) -> bool {
        match (self.latest.get(&channel), self.read.get(&channel)) {
            (Some(latest), Some(read)) => *latest > read.last_message_id,
            _ => false,
        }
    }

    pub fn mentions(&self, channel: Id) -> u32 {
        self.read.get(&channel).map_or(0, |r| r.mention_count)
    }

    /// Unread (muted channels left out) and mentions over a guild.
    pub fn guild_unread(&self, guild: &Guild) -> (bool, u32) {
        let me = self.me_id();
        let muted_guild = self.mutes.guilds.contains(&guild.id);
        let mut unread = false;
        let mut mentions = 0;
        for channel in &guild.channels {
            if channel.kind.is_text() && guild.can_view(me, channel) {
                if !muted_guild && !self.is_muted(channel) {
                    unread |= self.is_unread(channel.id);
                }
                mentions += self.mentions(channel.id);
            }
        }
        (unread, mentions)
    }

    pub fn dm_mentions(&self) -> u32 {
        self.dms.iter().map(|c| self.mentions(c.id)).sum()
    }

    /// How a user's name shows in a guild: nickname, display name or
    /// username, and the colour of their highest coloured role.
    pub fn display(&mut self, guild: Option<Id>, user: &User) -> (String, Option<egui::Color32>) {
        let Some(guild_id) = guild else {
            return (user.display_name().to_owned(), None);
        };
        let key = (guild_id, user.id);
        match self.members.get(&key) {
            Some(member) => {
                let name = member
                    .nick
                    .clone()
                    .unwrap_or_else(|| user.display_name().to_owned());
                // Settings → Accessibility → Role Colors: only "in names"
                // paints them.
                let color = self
                    .guild(guild_id)
                    .filter(|_| self.settings.role_colors == 0)
                    .map(|g| g.role_color(&member.roles))
                    .and_then(crate::theme::role_color);
                (name, color)
            }
            None => {
                if self.requested_members.insert(key) {
                    self.member_requests.push(key);
                }
                (user.display_name().to_owned(), None)
            }
        }
    }

    /// Asks the gateway for a member once, batched with the frame's others.
    pub fn request_member(&mut self, guild: Id, user: Id) {
        if self.requested_members.insert((guild, user)) {
            self.member_requests.push((guild, user));
        }
    }

    /// A gradient role's two colours for this user, when their highest
    /// coloured role has one.
    pub fn gradient(&self, guild: Option<Id>, user: Id) -> Option<(egui::Color32, egui::Color32)> {
        if self.settings.role_colors != 0 {
            return None;
        }
        let guild = self.guild(guild?)?;
        let member = self.members.get(&(guild.id, user))?;
        let (first, second) = guild.role_colors(&member.roles);
        Some((crate::theme::role_color(first)?, crate::theme::role_color(second?)?))
    }

    /// The display name for `<@id>`, `<#id>` and `<@&id>` tokens.
    pub fn name_for(&self, kind: char, id: Id) -> Option<String> {
        match kind {
            '@' => {
                let user = self.users.get(&id)?;
                let nick = self
                    .selected_guild
                    .and_then(|g| self.members.get(&(g, id)))
                    .and_then(|m| m.nick.clone());
                Some(nick.unwrap_or_else(|| user.display_name().to_owned()))
            }
            '#' => self.channel(id).map(Channel::title),
            '&' => self
                .guilds
                .iter()
                .flat_map(|g| g.roles.iter())
                .find(|r| r.id == id)
                .map(|r| r.name.clone()),
            _ => None,
        }
    }

    fn learn(&mut self, user: &User) {
        self.users.insert(user.id, user.clone());
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// The server voice channel someone is in, when we may see it (the
    /// bundle's rule for "In voice": a channel hidden from us shows nothing).
    pub fn user_voice_channel(&self, user: Id) -> Option<&Channel> {
        let me = self.me_id();
        self.guilds.iter().find_map(|guild| {
            let (_, channel) = guild.voice_states.iter().find(|(u, _)| *u == user)?;
            self.channel(*channel).filter(|channel| guild.can_view(me, channel))
        })
    }

    /// Who is in a voice channel (guild channels and DM calls), in
    /// Discord's order: by the server nickname, or else the name, in lower
    /// case and compared as JavaScript compares strings (UTF-16 units),
    /// then by id (the bundle's ChannelVoiceStates comparator; who is
    /// streaming would come first, which CordFast does not track yet).
    pub fn voice_members(&self, channel: &Channel) -> Vec<Id> {
        match channel.guild_id.and_then(|g| self.guild(g)) {
            Some(guild) => {
                let mut members: Vec<(Vec<u16>, Id)> = guild
                    .voice_states
                    .iter()
                    .filter(|(_, c)| *c == channel.id)
                    .map(|(u, _)| {
                        let name = self
                            .members
                            .get(&(guild.id, *u))
                            .and_then(|m| m.nick.clone())
                            .or_else(|| self.users.get(u).map(|user| user.display_name().to_owned()))
                            .unwrap_or_default();
                        (name.to_lowercase().encode_utf16().collect(), *u)
                    })
                    .collect();
                members.sort();
                members.into_iter().map(|(_, u)| u).collect()
            }
            None => self
                .dm_voice
                .iter()
                .filter(|(_, c)| **c == channel.id)
                .map(|(u, _)| *u)
                .collect(),
        }
    }

    pub fn incoming_call(&self) -> Option<Id> {
        let me = self.me_id();
        self.calls
            .iter()
            .find(|(channel, call)| {
                call.ringing.contains(&me) && self.voice.channel.map(|(_, c)| c) != Some(**channel)
            })
            .map(|(c, _)| *c)
    }

    // ---- events ----------------------------------------------------------

    /// Applies what the runtime sent. `ctx` is `None` without a window.
    pub fn pump(&mut self, ctx: Option<&egui::Context>) {
        while let Ok(event) = self.backend.events.try_recv() {
            self.apply(ctx, event);
        }
        if let Some(ctx) = ctx {
            self.apply_theme(ctx);
        }
        let now = Instant::now();
        self.typing.retain(|_, list| {
            list.retain(|(_, at)| now.duration_since(*at) < Duration::from_secs(10));
            !list.is_empty()
        });
        if let Some(ctx) = ctx {
            if !self.typing.is_empty() {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
            if self.voice.channel.is_some() && self.plugin(Plugin::CallTimer) {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
            if let Some((_, at)) = &self.notice {
                let left = Duration::from_secs(6).saturating_sub(at.elapsed());
                if left.is_zero() {
                    self.notice = None;
                } else {
                    ctx.request_repaint_after(left);
                }
            }
        }
        match self.keybinds.pressed.swap(0, std::sync::atomic::Ordering::Relaxed) {
            1 => self.actions.push(Action::ToggleMute),
            2 => self.actions.push(Action::ToggleDeaf),
            _ => {}
        }
        // Timed mutes end on their own, as in Discord.
        self.mutes.expire(now_ms());
        if self.status_until.is_some_and(|until| now_ms() >= until) {
            self.status_until = None;
            self.actions.push(Action::SetStatus("online", None));
        }
        // Nicknames and role colours for authors on screen, batched.
        let mut by_guild: HashMap<Id, Vec<Id>> = HashMap::new();
        for (guild, user) in self.member_requests.drain(..) {
            by_guild.entry(guild).or_default().push(user);
        }
        for (guild, users) in by_guild {
            self.commands.send(Command::RequestMembers { guild, users });
        }
        self.process_tray();
    }

    fn process_tray(&mut self) {
        let Some(tray) = &self.tray else { return };
        for event in tray.events() {
            match event {
                fastframe_tray::Event::Show => self.show = true,
                fastframe_tray::Event::Toggle => {
                    if self.has_window {
                        self.hide = true;
                    } else {
                        self.show = true;
                    }
                }
                fastframe_tray::Event::Menu("show") => self.show = true,
                fastframe_tray::Event::Menu("mute") => self.actions.push(Action::ToggleMute),
                fastframe_tray::Event::Menu("quit") => self.quit = true,
                fastframe_tray::Event::Menu(_) => {}
            }
        }
        if !self.actions.is_empty() {
            self.run_actions();
        }
    }

    fn update_tray(&mut self) {
        let unread: u32 = self.dm_mentions()
            + self
                .guilds
                .iter()
                .map(|g| self.guild_unread(g).1)
                .sum::<u32>();
        let mute_label = if self.voice.mute { t!("Unmute Microphone") } else { t!("Mute Microphone") };
        let in_voice = self.voice.channel.is_some();
        if let Some(tray) = &mut self.tray {
            tray.set_tooltip(if unread > 0 {
                tn!(unread, "CordFast — {count} mention", "CordFast — {count} mentions")
            } else {
                "CordFast".into()
            });
            // The labels again, in case the language changed.
            tray.set_label("show", t!("Open CordFast"));
            tray.set_label("quit", t!("Quit CordFast"));
            tray.set_label("mute", mute_label);
            tray.set_visible("mute", in_voice);
        }
    }

    fn apply(&mut self, ctx: Option<&egui::Context>, event: Event) {
        match event {
            Event::Auth(auth) => self.apply_auth(auth),
            Event::Gateway(dispatch) => self.apply_dispatch(dispatch),
            Event::Messages { channel, before, messages } => {
                for m in &messages {
                    self.learn(&m.author.clone());
                }
                if before.is_some()
                    && let Some((c, _, arrived)) = &mut self.scroll_anchor
                    && *c == channel
                {
                    *arrived = true;
                }
                let history = self.histories.entry(channel).or_default();
                history.loading = false;
                history.error = None;
                history.complete = messages.len() < 50;
                let mut older: Vec<Message> = messages.into_iter().rev().collect();
                if before.is_none() {
                    // Keeps what arrived through the gateway meanwhile.
                    let newest = older.last().map_or(0, |m| m.id);
                    older.extend(
                        history
                            .messages
                            .drain(..)
                            .filter(|m| m.id > newest || m.pending || m.deleted),
                    );
                } else {
                    older.append(&mut history.messages);
                }
                history.messages = older;
            }
            Event::MessagesFailed { channel, error } => {
                if self.scroll_anchor.is_some_and(|(c, _, _)| c == channel) {
                    self.scroll_anchor = None;
                }
                let history = self.histories.entry(channel).or_default();
                history.loading = false;
                history.error = Some(error);
            }
            Event::SendFailed { channel, nonce, error } => {
                if let Some(history) = self.histories.get_mut(&channel) {
                    history
                        .messages
                        .retain(|m| !(m.pending && m.nonce.as_deref() == Some(&nonce)));
                }
                self.notify(t!("Message not sent: {error}", error = error));
            }
            Event::Error(error) => self.notify(error),
            Event::Image { url, image } => match ctx {
                Some(ctx) => self.images.loaded(ctx, url, image),
                None => self.commands.send(Command::Forget { url }),
            },
            Event::Profile { user, profile } => {
                if let Ok(profile) = &profile {
                    self.profiles.insert(user, profile.clone());
                }
                if let Some(view) = &mut self.profile
                    && view.user.id == user
                {
                    view.state = match profile {
                        Ok(profile) => {
                            if let (Some(guild), Some(member)) = (view.guild, &profile.member) {
                                self.members.insert((guild, user), member.clone());
                            }
                            ProfileState::Ready(profile)
                        }
                        Err(error) => ProfileState::Failed(error),
                    };
                }
            }
            Event::DmOpened(channel) => {
                let id = channel.id;
                if !self.dms.iter().any(|c| c.id == id) {
                    self.dms.push(channel);
                }
                self.profile = None;
                self.select_guild(None);
                self.open_channel(id);
                // A message typed in a profile's box goes now.
                if let Some((user, text)) = self.pending_dm.take() {
                    if self.channel(id).is_some_and(|c| c.recipients.iter().any(|u| u.id == user)) {
                        self.send_text(text);
                    } else {
                        self.pending_dm = Some((user, text));
                    }
                }
            }
            Event::ForumPosts { forum, posts } => {
                for (thread, _) in &posts {
                    if let Some(guild) = self
                        .guilds
                        .iter_mut()
                        .find(|g| g.channels.iter().any(|c| c.id == forum))
                        && !guild.channels.iter().any(|c| c.id == thread.id)
                    {
                        let mut thread = thread.clone();
                        thread.guild_id = Some(guild.id);
                        guild.channels.push(thread);
                    }
                }
                self.forums.insert(forum, Some(posts));
            }
            Event::Voice(event) => self.apply_voice(event),
            Event::NotificationClicked { channel } => {
                self.show = true;
                let guild = self.channel(channel).and_then(|c| c.guild_id);
                self.select_guild(guild);
                self.open_channel(channel);
            }
            Event::Activity(activity, full) => {
                self.activity = activity;
                // Each game played lands in Registered Games, newest first.
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                for game in full.iter().filter(|a| a.kind == 0) {
                    let Some(id) = game.application.map(|id| id.to_string()) else { continue };
                    self.settings.played_games.retain(|g| g.id != id);
                    self.settings.played_games.insert(0, crate::settings::PlayedGame { id, name: game.name.clone(), last_played: now });
                    self.settings.played_games.truncate(100);
                    self.settings.save();
                }
                self.own_activities = full;
            }
            Event::Api { key, result } if !key.starts_with("do:") && !matches!(key.as_str(), "guild" | "members" | "invites" | "bans" | "emojis" | "audit") => {
                self.api_answer(&key, result);
            }
            Event::Api { key, result } => {
                if let Some(admin) = &mut self.admin {
                    let (notice, reload) = admin.apply(&key, result);
                    let command = reload.and_then(|tab| admin.load(tab));
                    if let Some(notice) = notice {
                        self.notify(notice);
                    }
                    if let Some(command) = command {
                        self.commands.send(command);
                    }
                }
            }
        }
    }

    fn apply_voice(&mut self, event: VoiceEvent) {
        match event {
            VoiceEvent::Channel(channel) => {
                self.voice.channel = channel;
                self.voice.connected = false;
                self.voice.error = None;
                self.voice.since = channel.map(|_| Instant::now());
                if channel.is_none() {
                    self.voice.speaking.clear();
                }
                self.update_tray();
            }
            VoiceEvent::Connected => {
                self.voice.connected = true;
                self.voice.error = None;
                self.voice_notice = None;
            }
            // Songbird reconnects by itself meanwhile; the reason is in the log.
            VoiceEvent::Disconnected => {
                self.voice.connected = false;
                self.voice.error = Some(t!("Voice Disconnected").into());
            }
            VoiceEvent::Ended { elsewhere, guild, channel } => {
                self.voice_notice = Some(VoiceNotice { elsewhere, guild, channel });
                self.sound(crate::sounds::Sound::Disconnect);
            }
            VoiceEvent::Speaking(users) => self.voice.speaking = users,
            VoiceEvent::MicLevel(db) => self.voice.mic_level = db,
            VoiceEvent::Regions(regions) => self.voice.regions = regions,
        }
    }

    fn apply_auth(&mut self, auth: Auth) {
        self.auth = match auth {
            Auth::LoggedOut => {
                self.clear_account();
                self.commands.send(Command::StartQrLogin);
                AuthView::Generating
            }
            Auth::Starting => AuthView::Checking,
            Auth::Generating => AuthView::Generating,
            Auth::Code(url) => match qrcode::QrCode::new(url.as_bytes()) {
                Ok(code) => AuthView::Code {
                    width: code.width(),
                    modules: code
                        .to_colors()
                        .into_iter()
                        .map(|c| c == qrcode::Color::Dark)
                        .collect(),
                },
                Err(error) => AuthView::Failed(error.to_string()),
            },
            Auth::Scanned { username } => AuthView::Scanned(username),
            Auth::Failed(error) => AuthView::Failed(error),
            Auth::Mfa { totp, sms, backup } => {
                let form = &mut self.login_form;
                form.busy = false;
                form.error = None;
                form.mfa = Some((totp, sms, backup));
                form.mfa_kind = if totp { "totp" } else if sms { "sms" } else { "backup" };
                form.code.clear();
                return;
            }
            Auth::SmsSent(phone) => {
                self.login_form.sms_phone = Some(phone);
                self.login_form.busy = false;
                return;
            }
            Auth::LoginError(error) => {
                self.login_form.busy = false;
                self.login_form.error = Some(error);
                return;
            }
        };
    }

    fn clear_account(&mut self) {
        self.me = None;
        self.guilds.clear();
        self.dms.clear();
        self.read.clear();
        self.latest.clear();
        self.histories.clear();
        self.subscribed.clear();
        self.members.clear();
        self.requested_members.clear();
        self.folders = None;
        self.relationships.clear();
        self.selected_guild = None;
        self.selected_channel = None;
        self.composer.clear();
        self.replying = None;
        self.editing = None;
        self.profile = None;
        self.page = None;
    }

    fn apply_dispatch(&mut self, dispatch: Dispatch) {
        match dispatch {
            Dispatch::Connecting => self.connection = Connection::Connecting,
            Dispatch::Connected => self.connection = Connection::Connected,
            Dispatch::Disconnected => self.connection = Connection::Disconnected,
            Dispatch::AuthFailed => {
                self.notify(t!("The session expired. Log in again."));
                self.apply_auth(Auth::LoggedOut);
            }
            Dispatch::Ready(ready) => {
                let ready = *ready;
                self.preloaded = crate::discord::preloaded::Preloaded::new(ready.settings_proto.clone());
                self.learn(&ready.me);
                self.me = Some(ready.me);
                for channel in &ready.private_channels {
                    for user in &channel.recipients {
                        self.users.insert(user.id, user.clone());
                    }
                }
                self.latest.clear();
                for channel in ready
                    .private_channels
                    .iter()
                    .chain(ready.guilds.iter().flat_map(|g| g.channels.iter()))
                {
                    if let Some(last) = channel.last_message_id {
                        self.latest.insert(channel.id, last);
                    }
                }
                let me = self.me_id();
                for guild in &ready.guilds {
                    self.members.insert(
                        (guild.id, me),
                        Member { nick: None, roles: guild.my_roles.clone() },
                    );
                }
                self.read = ready.read_states.into_iter().map(|r| (r.channel, r)).collect();
                for guild in &ready.guilds {
                    for (user, mute, deaf) in &guild.voice_flags {
                        self.voice_flags.insert(*user, (*mute, *deaf));
                    }
                }
                self.guilds = ready.guilds;
                self.dms = ready.private_channels;
                if ready.folders.is_some() {
                    self.folders = ready.folders;
                }
                self.mutes = ready.mutes;
                self.status = ready.status.clone();
                let me = self.me.clone().unwrap_or_default();
                let account = crate::settings::Account {
                    id: me.id.to_string(),
                    name: me.display_name().to_owned(),
                    username: me.username.clone(),
                    avatar: me.avatar_url(),
                };
                match self.settings.accounts.iter_mut().find(|a| a.id == account.id) {
                    Some(existing) => *existing = account,
                    None => self.settings.accounts.push(account),
                }
                self.settings.save();
                self.custom_status = ready.custom_status.clone();
                self.relationships = ready
                    .relationships
                    .into_iter()
                    .map(|r| (r.user.id, r))
                    .collect();
                for r in self.relationships.values() {
                    self.users.insert(r.user.id, r.user.clone());
                }
                self.subscribed.clear();
                self.histories.clear();
                if let Some(channel) = self.selected_channel {
                    self.open_channel(channel);
                }
                if let Some(guild) = self.selected_guild {
                    self.subscribe(guild);
                }
                self.auth = AuthView::LoggedIn;
                self.commands.send(Command::Voice(VoiceCommand::Settings(
                    crate::backend::audio_settings(&self.settings),
                )));
                self.commands.send(Command::Voice(VoiceCommand::RankRegions));
                self.update_tray();
                self.preview();
            }
            Dispatch::GuildCreate(guild) => {
                for channel in &guild.channels {
                    if let Some(last) = channel.last_message_id {
                        self.latest.insert(channel.id, last);
                    }
                }
                match self.guilds.iter_mut().find(|g| g.id == guild.id) {
                    Some(existing) => *existing = *guild,
                    None => self.guilds.push(*guild),
                }
            }
            Dispatch::GuildDelete(id) => {
                if let Some(guild) = self.guild(id)
                    && self.plugin(Plugin::RelationshipNotifier)
                {
                    let name = guild.name.clone();
                    self.system_notify(t!("Server"), &t!("You are no longer in {name}", name = name), 0);
                }
                self.guilds.retain(|g| g.id != id);
                if self.selected_guild == Some(id) {
                    self.select_guild(None);
                }
            }
            Dispatch::ChannelUpsert(mut channel) => {
                // Threads arrive without their guild in some events.
                if channel.guild_id.is_none()
                    && let Some(parent) = channel.parent_id
                {
                    channel.guild_id = self.channel(parent).and_then(|c| c.guild_id);
                }
                let list = match channel.guild_id {
                    Some(guild) => match self.guilds.iter_mut().find(|g| g.id == guild) {
                        Some(g) => &mut g.channels,
                        None => return,
                    },
                    None => &mut self.dms,
                };
                match list.iter_mut().find(|c| c.id == channel.id) {
                    Some(existing) => {
                        let recipients = std::mem::take(&mut existing.recipients);
                        let joined = existing.joined;
                        let status = existing.status.take();
                        *existing = channel;
                        if existing.recipients.is_empty() {
                            existing.recipients = recipients;
                        }
                        existing.joined |= joined;
                        // Channel updates carry no status: it comes on its own.
                        if existing.status.is_none() {
                            existing.status = status;
                        }
                    }
                    None => list.push(channel),
                }
            }
            Dispatch::Autocomplete { nonce, choices } => {
                if let Some(draft) = &mut self.slash
                    && draft.asked.as_ref().is_some_and(|(asked, ..)| *asked == nonce)
                {
                    draft.suggestions = choices;
                }
            }
            Dispatch::InteractionFailed { nonce } => {
                if self.sent_commands.contains(&nonce) {
                    self.notify(t!("The application did not respond."));
                }
            }
            Dispatch::VoiceChannelStatus { channel, status } => {
                if let Some(c) = self.guilds.iter_mut().flat_map(|g| g.channels.iter_mut()).find(|c| c.id == channel) {
                    c.status = status;
                }
            }
            Dispatch::ChannelDelete(id) => {
                self.dms.retain(|c| c.id != id);
                for guild in &mut self.guilds {
                    guild.channels.retain(|c| c.id != id);
                }
                if self.selected_channel == Some(id) {
                    self.selected_channel = None;
                }
            }
            Dispatch::MessageCreate(message) => self.message_created(*message),
            Dispatch::MessageUpdate(message) => {
                let logger = self.plugin(Plugin::MessageLogger);
                if let Some(history) = self.histories.get_mut(&message.channel_id)
                    && let Some(existing) = history.messages.iter_mut().find(|m| m.id == message.id)
                {
                    let mut edits = std::mem::take(&mut existing.edits);
                    if logger && existing.content != message.content && !existing.content.is_empty() {
                        edits.push(existing.content.clone());
                    }
                    *existing = *message;
                    existing.edits = edits;
                }
            }
            Dispatch::MessageDelete { channel, id } => {
                let logger = self.plugin(Plugin::MessageLogger);
                if let Some(history) = self.histories.get_mut(&channel) {
                    if logger {
                        if let Some(m) = history.messages.iter_mut().find(|m| m.id == id)
                            && crate::plugins::keeps(m)
                        {
                            m.deleted = true;
                        }
                    } else {
                        history.messages.retain(|m| m.id != id);
                    }
                }
            }
            Dispatch::Ack { channel, message } => {
                let entry = self.read.entry(channel).or_insert(ReadState {
                    channel,
                    last_message_id: 0,
                    mention_count: 0,
                });
                entry.last_message_id = entry.last_message_id.max(message);
                entry.mention_count = 0;
                self.update_tray();
            }
            Dispatch::Typing { channel, user } => {
                if user != self.me_id() {
                    let list = self.typing.entry(channel).or_default();
                    list.retain(|(u, _)| *u != user);
                    list.push((user, Instant::now()));
                }
            }
            Dispatch::Members { guild, members } => {
                for (user, member) in members {
                    self.users.insert(user.id, user.clone());
                    if user.id == self.me_id()
                        && let Some(g) = self.guilds.iter_mut().find(|g| g.id == guild)
                    {
                        g.my_roles = member.roles.clone();
                    }
                    self.members.insert((guild, user.id), member);
                }
            }
            Dispatch::ReactionAdd { channel, message, user, emoji } => {
                self.reaction(channel, message, user, emoji, true);
            }
            Dispatch::ReactionRemove { channel, message, user, emoji } => {
                self.reaction(channel, message, user, emoji, false);
            }
            Dispatch::ReactionsCleared { channel, message } => {
                if let Some(m) = self
                    .histories
                    .get_mut(&channel)
                    .and_then(|h| h.messages.iter_mut().find(|m| m.id == message))
                {
                    m.reactions.clear();
                }
            }
            Dispatch::RelationshipAdd(relationship) => {
                self.users.insert(relationship.user.id, relationship.user.clone());
                self.relationships.insert(relationship.user.id, relationship);
            }
            Dispatch::RelationshipRemove { user, kind } => {
                let removed = self.relationships.remove(&user);
                if kind == 1
                    && self.plugin(Plugin::RelationshipNotifier)
                    && let Some(r) = removed
                {
                    let name = r.user.display_name().to_owned();
                    self.system_notify(t!("Friendship"), &t!("{name} is no longer your friend", name = name), 0);
                }
            }
            Dispatch::GuildSettings(entry) => self.mutes.add(&entry),
            Dispatch::Folders(folders) => self.folders = Some(folders),
            Dispatch::Preloaded { bytes, partial } => self.preloaded.update(bytes, partial),
            Dispatch::VoiceState(state) => {
                let Some(user) = state.get("user_id").and_then(sf) else { return };
                let channel = state.get("channel_id").and_then(sf);
                // Discord's sounds: someone joining or leaving our call, and
                // our own arrival and disconnect.
                let before = self
                    .guilds
                    .iter()
                    .flat_map(|g| g.voice_states.iter())
                    .find(|(u, _)| *u == user)
                    .map(|(_, c)| *c)
                    .or_else(|| self.dm_voice.get(&user).copied());
                if before != channel {
                    let ours = self.voice.channel.map(|(_, c)| c);
                    let sound = if user == self.me_id() {
                        if channel.is_some() { Some(crate::sounds::Sound::Join) } else { Some(crate::sounds::Sound::Disconnect) }
                    } else if ours.is_some() && channel == ours {
                        Some(crate::sounds::Sound::Join)
                    } else if ours.is_some() && before == ours {
                        Some(crate::sounds::Sound::Leave)
                    } else {
                        None
                    };
                    if let Some(sound) = sound {
                        self.sound(sound);
                    }
                }
                self.voice_flags.insert(user, (crate::discord::model::voice_muted(&state), crate::discord::model::voice_deaf(&state)));
                match state.get("guild_id").and_then(sf) {
                    Some(guild_id) => {
                        if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
                            guild.voice_states.retain(|(u, _)| *u != user);
                            if let Some(channel) = channel {
                                guild.voice_states.push((user, channel));
                            }
                        }
                    }
                    None => {
                        self.dm_voice.remove(&user);
                        if let Some(channel) = channel {
                            self.dm_voice.insert(user, channel);
                        }
                    }
                }
                if let Some(member) = state.get("member") {
                    if let Some(u) = member.get("user").and_then(User::parse) {
                        self.users.insert(u.id, u);
                    }
                }
            }
            Dispatch::VoiceServer(_) => {}
            Dispatch::Call(call) => {
                let Some(channel) = call.get("channel_id").and_then(sf) else { return };
                let ringing: Vec<Id> = call
                    .get("ringing")
                    .and_then(serde_json::Value::as_array)
                    .map(|l| l.iter().filter_map(sf).collect())
                    .unwrap_or_default();
                let me = self.me_id();
                let newly_ringing = ringing.contains(&me)
                    && !self.calls.get(&channel).is_some_and(|c| c.ringing.contains(&me));
                for state in call
                    .get("voice_states")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(user) = state.get("user_id").and_then(sf) {
                        self.dm_voice.insert(user, channel);
                    }
                }
                self.calls.insert(
                    channel,
                    Call {
                        ringing,
                        region: call
                            .get("region")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                    },
                );
                if newly_ringing {
                    let title = self
                        .channel(channel)
                        .map(Channel::title)
                        .unwrap_or_else(|| t!("Someone").into());
                    self.show = true;
                    self.system_notify(t!("Incoming Call"), &t!("{name} is calling you", name = title), channel);
                }
            }
            Dispatch::MemberList(update) => self.member_list_update(update),
            Dispatch::PollVote { channel, message, user, answer, add } => {
                let me = self.me_id();
                if let Some(poll) = self
                    .histories
                    .get_mut(&channel)
                    .and_then(|h| h.messages.iter_mut().find(|m| m.id == message))
                    .and_then(|m| m.poll.as_mut())
                    && let Some(a) = poll.answers.iter_mut().find(|a| a.0 == answer)
                {
                    a.2 = if add { a.2 + 1 } else { a.2.saturating_sub(1) };
                    if user == me {
                        a.3 = add;
                    }
                }
            }
            Dispatch::GuildEmojis { guild, emojis } => {
                if let Some(g) = self.guilds.iter_mut().find(|g| g.id == guild) {
                    g.emojis = emojis;
                }
            }
            Dispatch::Presences(list) => {
                let me = self.me_id();
                for presence in list {
                    if presence.user == me {
                        continue;
                    }
                    self.presences.insert(presence.user, presence);
                }
            }
            Dispatch::RoleUpsert { guild, role } => {
                if let (Some(g), Some(role)) = (
                    self.guilds.iter_mut().find(|g| g.id == guild),
                    crate::discord::model::Role::parse(&role),
                ) {
                    match g.roles.iter_mut().find(|r| r.id == role.id) {
                        Some(existing) => *existing = role,
                        None => g.roles.push(role),
                    }
                }
            }
            Dispatch::RoleDelete { guild, role } => {
                if let Some(g) = self.guilds.iter_mut().find(|g| g.id == guild) {
                    g.roles.retain(|r| r.id != role);
                }
            }
            Dispatch::GuildUpdate(update) => {
                if let Some(id) = update.get("id").and_then(sf)
                    && let Some(g) = self.guilds.iter_mut().find(|g| g.id == id)
                {
                    if let Some(name) = update.get("name").and_then(serde_json::Value::as_str) {
                        g.name = name.to_owned();
                    }
                    if let Some(icon) = update.get("icon") {
                        g.icon = icon.as_str().map(str::to_owned);
                    }
                    if let Some(owner) = update.get("owner_id").and_then(sf) {
                        g.owner_id = Some(owner);
                    }
                }
            }
            Dispatch::CallEnded(channel) => {
                self.calls.remove(&channel);
                self.dm_voice.retain(|_, c| *c != channel);
            }
        }
    }

    fn member_list_update(&mut self, update: crate::discord::gateway::MemberListUpdate) {
        if std::env::var_os("CORDFAST_DUMP").is_some() {
            log::info!("dump member list guild={} list={} ops={} online={} total={} state={:?}", update.guild, update.list, update.ops.len(), update.online, update.total, self.member_list.as_ref().map(|m| (m.guild, m.list.clone(), m.rows.len())));
        }
        use crate::discord::gateway::{ListItem, ListOp};
        let Some(state) = &mut self.member_list else { return };
        if state.guild != update.guild {
            return;
        }
        let syncs = update
            .ops
            .iter()
            .any(|op| matches!(op, crate::discord::gateway::ListOp::Sync { start: 0, .. }));
        if state.list.is_empty() {
            state.list = update.list.clone();
        } else if state.list != update.list {
            // Another list: the newly opened channel's, when it starts over.
            if !syncs {
                return;
            }
            state.list = update.list.clone();
            state.rows.clear();
            state.groups.clear();
        }
        state.online = update.online;
        state.total = update.total;
        if !update.groups.is_empty() {
            state.groups = update.groups.iter().cloned().collect();
        }
        let mut learned = Vec::new();
        for op in update.ops {
            match op {
                ListOp::Sync { start, items } => {
                    if state.rows.len() < start + items.len() {
                        state.rows.resize(start + items.len(), None);
                    }
                    for (i, item) in items.into_iter().enumerate() {
                        learned.push(item.clone());
                        state.rows[start + i] = Some(item);
                    }
                }
                ListOp::Insert { index, item } => {
                    learned.push(item.clone());
                    let index = index.min(state.rows.len());
                    state.rows.insert(index, Some(item));
                }
                ListOp::Update { index, item } => {
                    learned.push(item.clone());
                    if index < state.rows.len() {
                        state.rows[index] = Some(item);
                    }
                }
                ListOp::Delete { index } => {
                    if index < state.rows.len() {
                        state.rows.remove(index);
                    }
                }
                ListOp::Invalidate { start, end } => {
                    for row in state.rows.iter_mut().skip(start).take(end.saturating_sub(start) + 1) {
                        *row = None;
                    }
                }
            }
        }
        let guild = update.guild;
        for item in learned {
            if let ListItem::Member { user, member, presence } = item {
                self.requested_members.insert((guild, user.id));
                self.members.insert((guild, user.id), member);
                if let Some(presence) = presence {
                    self.presences.insert(user.id, presence);
                }
                self.users.insert(user.id, user);
            }
        }
    }

    /// CORDFAST_PREVIEW opens a screen at start, for screenshots:
    /// `guild[:N]` (the Nth server), `dm:N`, `profile:N` and `member:N` (a
    /// friend's profile from a DM or a server), `me`, `appearance`, `voice`,
    /// `plugins` or `server:PAGE` (the first server's settings).
    fn preview(&mut self) {
        let Ok(target) = std::env::var("CORDFAST_PREVIEW") else { return };
        let (what, n) = target.split_once(':').unwrap_or((target.as_str(), "0"));
        match what {
            // The loading screen, held as a session starting shows it.
            "loading" => self.auth = AuthView::Checking,
            // The first server with these files waiting in its box.
            "uploads" => {
                if let Some(id) = self.guilds.first().map(|g| g.id) {
                    self.select_guild(Some(id));
                }
                self.files = n.split(',').filter(|p| !p.is_empty()).map(|p| crate::backend::Upload::new(p.into())).collect();
            }
            "guild" => {
                let id = match n.strip_prefix("name=") {
                    Some(name) => self.guilds.iter().find(|g| g.name == name).map(|g| g.id),
                    None => self.guilds.get(n.parse::<usize>().unwrap_or(0)).map(|g| g.id),
                };
                if let Some(id) = id {
                    if std::env::var_os("CORDFAST_DUMP").is_some()
                        && let Some(g) = self.guild(id)
                    {
                        let me = self.me_id();
                        for c in &g.channels {
                            if !c.name.is_ascii() {
                                log::info!("dump channel {:?} = {}", c.name, c.name.escape_unicode());
                            }
                        }
                        log::info!("dump my roles {:?}", g.roles_of(&g.my_roles).iter().map(|r| (r.name.clone(), format!("{:06x}", r.color), r.color2.map(|c| format!("{c:06x}")))).collect::<Vec<_>>());
                        log::info!("dump me member {:?}", self.members.get(&(id, me)).map(|m| m.nick.clone()));
                    }
                    self.select_guild(Some(id));
                }
            }
            "dm" => {
                let index: usize = n.parse().unwrap_or(0);
                let mut dms: Vec<(Id, Id)> = self
                    .dms
                    .iter()
                    .map(|c| (self.latest.get(&c.id).copied().unwrap_or(0), c.id))
                    .collect();
                dms.sort_unstable_by(|a, b| b.cmp(a));
                if let Some((_, id)) = dms.get(index) {
                    self.open_channel(*id);
                }
            }
            "profile" | "member" => {
                // A friend's profile, as from a DM, or (member) as from the
                // first server's chat.
                let index: usize = n.parse().unwrap_or(0);
                let mut friends: Vec<User> = self.relationships.values().filter(|r| r.kind == 1).map(|r| r.user.clone()).collect();
                friends.sort_by_key(|u| u.display_name().to_lowercase());
                let guild = (what == "member").then(|| self.guilds.first().map(|g| g.id)).flatten();
                if let Some(guild) = guild {
                    self.select_guild(Some(guild));
                }
                if let Some(user) = friends.get(index).cloned() {
                    self.run(Action::OpenProfile { user, guild, at: egui::pos2(500.0, 120.0) });
                }
            }
            "me" => {
                if let Some(user) = self.me.clone() {
                    self.run(Action::OpenProfile { user, guild: None, at: egui::pos2(80.0, 200.0) });
                }
            }
            // A server's settings on one page: `server:overview`, `server:roles`…
            "server" => {
                let Some(guild) = self.guilds.first().map(|g| g.id) else { return };
                self.run(Action::SelectGuild(Some(guild)));
                self.run(Action::OpenGuildSettings(guild));
                let tab = match n {
                    "safety" => crate::admin::Tab::Safety,
                    "roles" | "role" | "everyone" => crate::admin::Tab::Roles,
                    "emojis" => crate::admin::Tab::Emojis,
                    "channels" => crate::admin::Tab::Channels,
                    "members" => crate::admin::Tab::Members,
                    "invites" => crate::admin::Tab::Invites,
                    "bans" => crate::admin::Tab::Bans,
                    "audit" => crate::admin::Tab::Audit,
                    _ => crate::admin::Tab::Overview,
                };
                self.run(Action::GuildTab(tab));
                // A role open in its editor: one with a colour, or @everyone.
                let role = self.guild(guild).and_then(|g| match n {
                    "role" => g.roles.iter().filter(|r| r.id != guild).max_by_key(|r| r.position).cloned(),
                    "everyone" => g.roles.iter().find(|r| r.id == guild).cloned(),
                    _ => None,
                });
                if let (Some(role), Some(admin)) = (role, &mut self.admin) {
                    let draft = crate::admin::RoleDraft {
                        id: role.id,
                        name: role.name.clone(),
                        color: [(role.color >> 16) as u8, (role.color >> 8) as u8, role.color as u8],
                        hoist: role.hoist,
                        mentionable: role.mentionable,
                        permissions: role.permissions,
                    };
                    admin.role_saved = Some(draft.clone());
                    admin.role = Some(draft);
                    admin.role_tab = if role.id == guild { 1 } else { 0 };
                }
            }
            // The account card as during a call (no voice is joined).
            "call" => {
                let Some(guild) = self.guilds.first() else { return };
                let (guild, channel) = (guild.id, guild.channels.iter().find(|c| c.kind == ChannelKind::Voice).map(|c| c.id));
                self.run(Action::SelectGuild(Some(guild)));
                if let Some(channel) = channel {
                    self.voice.channel = Some((Some(guild), channel));
                    self.voice.connected = true;
                    self.voice.since = Some(Instant::now());
                    self.voice.regions = vec![("brazil".into(), 39)];
                }
            }
            // The first server with the expression picker open on a tab.
            "emoji" | "gifs" | "stickers" => {
                if let Some(id) = self.guilds.first().map(|g| g.id) {
                    self.select_guild(Some(id));
                }
                let tab = match what {
                    "gifs" => ExpressionTab::Gifs,
                    "stickers" => ExpressionTab::Stickers,
                    _ => ExpressionTab::Emoji,
                };
                self.toggle_expressions(tab, PickerTarget::Composer, 0.0);
            }
            "keybinds" | "system" => self.run(Action::OpenSettings(SettingsTab::System)),
            "account" => self.run(Action::OpenSettings(SettingsTab::Account)),
            "privacy" => self.run(Action::OpenSettings(SettingsTab::DataPrivacy)),
            "messaging" => self.run(Action::OpenSettings(SettingsTab::MessagingPermissions)),
            "notifications" => self.run(Action::OpenSettings(SettingsTab::Notifications)),
            "accessibility" => self.run(Action::OpenSettings(SettingsTab::Accessibility)),
            "activity" => self.run(Action::OpenSettings(SettingsTab::ActivityPrivacy)),
            "games" => self.run(Action::OpenSettings(SettingsTab::RegisteredGames)),
            "connections" => self.run(Action::OpenSettings(SettingsTab::ConnectedApps)),
            "nitro" => self.run(Action::OpenSettings(SettingsTab::Nitro)),
            "boost" => self.run(Action::OpenSettings(SettingsTab::ServerBoost)),
            "appearance" => self.run(Action::OpenSettings(SettingsTab::Appearance)),
            "language" => self.run(Action::OpenSettings(SettingsTab::Language)),
            "voice" => self.run(Action::OpenSettings(SettingsTab::Voice)),
            "plugins" => self.run(Action::OpenSettings(SettingsTab::Plugins)),
            _ => {}
        }
    }

    /// An application's icon for activity cards, asked for once.
    /// The slash commands of a channel's place, asked for the first time
    /// they are wanted; none while they load.
    pub fn command_index(&mut self, channel: &Channel) -> Option<&crate::discord::commands::Index> {
        let (key, path) = match channel.guild_id {
            Some(guild) => (guild, format!("/guilds/{guild}/application-command-index")),
            None => (channel.id, format!("/channels/{}/application-command-index", channel.id)),
        };
        if !self.command_index.contains_key(&key) {
            self.command_index.insert(key, None);
            self.commands.send(crate::admin::get(&format!("command-index:{key}"), path));
        }
        self.command_index.get(&key).and_then(Option::as_ref)
    }

    pub fn app_icon(&mut self, application: Id) -> Option<String> {
        match self.app_icons.get(&application) {
            Some(icon) => icon.clone(),
            None => {
                self.app_icons.insert(application, None);
                self.commands.send(crate::admin::get(&format!("app:{application}"), format!("/applications/{application}/rpc")));
                None
            }
        }
    }

    /// Answers to the API calls the chat makes (pins, search, joining).
    fn api_answer(&mut self, key: &str, result: Result<serde_json::Value, String>) {
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                if let Some(id) = key.strip_prefix("app:").and_then(|k| k.parse::<Id>().ok()) {
                    self.app_icons.insert(id, None);
                    return;
                }
                if let Some(id) = key.strip_prefix("guild-profile:").and_then(|k| k.parse::<Id>().ok()) {
                    self.guild_profiles.insert(id, Some(Err(error)));
                    return;
                }
                if let Some((key, _)) = self.settings_data.get_key_value(key) {
                    let key: &'static str = key;
                    self.settings_data.insert(key, Some(Err(error)));
                    return;
                }
                // No apps to list (or no access): the built-in commands stay.
                if let Some(id) = key.strip_prefix("command-index:").and_then(|k| k.parse::<Id>().ok()) {
                    self.command_index.insert(id, Some(crate::discord::commands::Index::built_in_only()));
                    return;
                }
                if key == "join" && error.contains("captcha") {
                    self.notify(t!("Discord asked for a captcha: opening the invite in the browser."));
                    if let Some(Modal::AddServer { invite, .. }) = &self.modal {
                        let _ = open::that_detached(format!("https://discord.gg/{}", invite_code(invite)));
                    }
                } else {
                    self.notify(error);
                }
                return;
            }
        };
        if let Some((key, _)) = self.settings_data.get_key_value(key) {
            let key: &'static str = key;
            self.settings_data.insert(key, Some(Ok(value)));
            return;
        }
        let messages = |v: &serde_json::Value| -> Vec<Message> {
            v.as_array().map(|l| l.iter().filter_map(Message::parse).collect()).unwrap_or_default()
        };
        if let Some(id) = key.strip_prefix("command-index:").and_then(|k| k.parse::<Id>().ok()) {
            self.command_index.insert(id, Some(crate::discord::commands::Index::parse(&value)));
            return;
        }
        if let Some(id) = key.strip_prefix("guild-profile:").and_then(|k| k.parse::<Id>().ok()) {
            let card = crate::discord::model::GuildProfile::parse(&value).ok_or_else(|| t!("This server has no public profile.").to_owned());
            self.guild_profiles.insert(id, Some(card));
            return;
        }
        match key {
            "pins" => {
                // The newer endpoint wraps each pin: { items: [{ message }] }.
                let list = match value.get("items").and_then(serde_json::Value::as_array) {
                    Some(items) => items.iter().filter_map(|i| Message::parse(i.get("message")?)).collect(),
                    None => messages(&value),
                };
                self.side = Some(Side::Pins(Some(list)));
            }
            "search" => {
                if value.get("retry_after").is_some() {
                    self.notify(t!("Discord is still indexing this channel. Try again in a moment."));
                    return;
                }
                let total = value.get("total_results").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let list: Vec<Message> = value
                    .get("messages")
                    .and_then(serde_json::Value::as_array)
                    .map(|groups| groups.iter().filter_map(|g| Message::parse(g.as_array()?.first()?)).collect())
                    .unwrap_or_default();
                if let Some(Side::Search { results, .. }) = &mut self.side {
                    *results = Some((total, list));
                }
            }
            "join" => {
                self.modal = None;
                let name = value.pointer("/guild/name").and_then(serde_json::Value::as_str).unwrap_or(t!("the server"));
                self.notify(t!("You joined {name}.", name = name));
            }
            "create" => {
                self.modal = None;
                self.notify(t!("Server created."));
            }
            "thread" => {
                self.modal = None;
                if let Some(id) = value.get("id").and_then(sf) {
                    let guild = self.selected_guild;
                    if let Some(mut channel) = crate::discord::model::Channel::parse(&value, guild, &Default::default()) {
                        channel.joined = true;
                        if let Some(g) = guild.and_then(|g| self.guilds.iter_mut().find(|x| x.id == g))
                            && !g.channels.iter().any(|c| c.id == id)
                        {
                            g.channels.push(channel);
                        }
                    }
                    self.open_channel(id);
                }
            }
            "pin" => self.notify(t!("Done.")),
            "gifs" => {
                let gifs = value
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(|g| {
                                Some(Gif {
                                    url: g.get("url")?.as_str()?.to_owned(),
                                    preview: g.get("preview").or_else(|| g.get("gif_src"))?.as_str()?.to_owned(),
                                    width: g.get("width").and_then(serde_json::Value::as_u64).unwrap_or(200) as u32,
                                    height: g.get("height").and_then(serde_json::Value::as_u64).unwrap_or(150) as u32,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(picker) = &mut self.expressions {
                    picker.gifs.results = Some(gifs);
                }
            }
            "gif-trending" => {
                let categories = value
                    .get("categories")
                    .and_then(serde_json::Value::as_array)
                    .map(|l| l.iter().filter_map(|c| Some((c.get("name")?.as_str()?.to_owned(), c.get("src")?.as_str()?.to_owned()))).collect())
                    .unwrap_or_default();
                let preview = value.pointer("/gifs/0").and_then(|g| g.get("preview").or_else(|| g.get("gif_src")).or_else(|| g.get("src"))).and_then(serde_json::Value::as_str).map(str::to_owned);
                if let Some(picker) = &mut self.expressions {
                    picker.gifs.trending = Some((categories, preview));
                }
            }
            "frecency" => {
                use base64::Engine;
                let bytes = value
                    .get("settings")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
                    .unwrap_or_default();
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                self.frecency = Some(crate::discord::frecency::Frecency::decode(&bytes, now));
            }
            "inbox" => {
                if let Some(inbox) = &mut self.inbox {
                    inbox.mentions = Some(messages(&value));
                }
            }
            "invite" => {
                if let Some(code) = value.get("code").and_then(serde_json::Value::as_str) {
                    self.notify(t!("Invite copied: discord.gg/{code}", code = code));
                    self.clipboard = Some(format!("https://discord.gg/{code}"));
                }
            }
            "profile-saved" => self.notify(t!("Profile saved.")),
            "kicked" => self.notify(t!("Member kicked.")),
            "timeout" => self.notify(t!("Member timed out.")),
            "banned" => self.notify(t!("Member banned.")),
            "nick" => self.notify(t!("Nickname changed.")),
            "note" => self.notify(t!("Note saved.")),
            key if key.starts_with("app:") => {
                if let Ok(id) = key[4..].parse::<Id>() {
                    let icon = value
                        .get("icon")
                        .and_then(serde_json::Value::as_str)
                        .map(|hash| format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=128"));
                    self.app_icons.insert(id, icon);
                    if let Some(name) = value.get("name").and_then(serde_json::Value::as_str) {
                        self.app_names.insert(id, name.to_owned());
                    }
                }
            }
            "saved-image" => {
                let path = value.get("path").and_then(serde_json::Value::as_str).unwrap_or_default();
                self.notify(t!("Image saved to {path}", path = path));
            }
            "copied-image" => self.notify(t!("Image copied.")),
            "tag-join" => {
                let name = value.get("name").and_then(serde_json::Value::as_str).unwrap_or(t!("the server"));
                self.notify(t!("You joined {name}.", name = name));
            }
            "dm-created" => {
                if let Some(channel) = crate::discord::model::Channel::parse(&value, None, &Default::default()) {
                    let id = channel.id;
                    if !self.dms.iter().any(|c| c.id == id) {
                        self.dms.push(channel);
                    }
                    self.select_guild(None);
                    self.open_channel(id);
                }
            }
            "my-profile" => {
                if let Some(draft) = &mut self.profile_draft {
                    let text = |key: &str| {
                        value
                            .pointer(&format!("/user_profile/{key}"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    draft.bio = text("bio");
                    draft.pronouns = text("pronouns");
                }
            }
            _ => {}
        }
    }

    /// Opens (or reopens) the member list of a guild channel.
    pub fn watch_members(&mut self, guild: Id, channel: Id, rows: u64) {
        match &mut self.member_list {
            Some(state) if state.guild == guild => {
                if state.channel != channel {
                    // Channels with the same permissions share one list, which
                    // Discord does not send again: keep it until a new one comes.
                    state.channel = channel;
                    state.requested = 0;
                }
            }
            _ => self.member_list = Some(MemberList { guild, channel, ..MemberList::default() }),
        }
        if let Some(state) = &mut self.member_list
            && rows > state.requested
        {
            state.requested = rows;
            self.commands.send(Command::MemberList { guild, channel, rows });
        }
    }

    fn reaction(&mut self, channel: Id, message: Id, user: Id, emoji: crate::discord::model::Emoji, adding: bool) {
        let me = self.me_id();
        let Some(m) = self
            .histories
            .get_mut(&channel)
            .and_then(|h| h.messages.iter_mut().find(|m| m.id == message))
        else {
            return;
        };
        match m.reactions.iter().position(|r| r.emoji == emoji) {
            Some(index) => {
                let r = &mut m.reactions[index];
                if adding {
                    // Our own add was counted when we clicked.
                    if !(user == me && r.me) {
                        r.count += 1;
                    }
                    r.me |= user == me;
                } else {
                    if !(user == me && !r.me) {
                        r.count = r.count.saturating_sub(1);
                    }
                    if user == me {
                        r.me = false;
                    }
                    if r.count == 0 {
                        m.reactions.remove(index);
                    }
                }
            }
            None if adding => m.reactions.push(crate::discord::model::Reaction {
                emoji,
                count: 1,
                me: user == me,
            }),
            None => {}
        }
    }

    fn system_notify(&mut self, title: &str, body: &str, channel: Id) {
        if self.settings.notifications {
            self.commands.send(Command::Notify {
                title: title.into(),
                body: body.into(),
                channel,
                sound: self.settings.notify_sound,
            });
        }
    }

    fn message_created(&mut self, mut message: Message) {
        let me = self.me_id();
        let channel = message.channel_id;
        self.learn(&message.author);
        for user in &message.mentions {
            self.learn(user);
        }
        if let (Some(guild), Some(member)) = (message.guild_id, message.member.take()) {
            self.members.insert((guild, message.author.id), member);
        }
        if let Some(list) = self.typing.get_mut(&channel) {
            list.retain(|(u, _)| *u != message.author.id);
        }
        self.latest.insert(channel, message.id);
        let blocked = self
            .relationships
            .get(&message.author.id)
            .is_some_and(|r| r.kind == 2);

        let mine = message.author.id == me;
        let looking = self.selected_channel == Some(channel) && self.has_window && self.focused;
        if mine || looking {
            self.mark_read(channel, message.id, !mine);
        } else if !blocked {
            let is_dm = message.guild_id.is_none() && self.dms.iter().any(|c| c.id == channel);
            let my_roles: Vec<Id> = message
                .guild_id
                .and_then(|g| self.guild(g))
                .map(|g| g.my_roles.clone())
                .unwrap_or_default();
            let direct = message.mentions.iter().any(|u| u.id == me)
                || message.mention_roles.iter().any(|r| my_roles.contains(r));
            let mentioned = is_dm || message.mention_everyone || direct;
            if mentioned {
                let entry = self.read.entry(channel).or_insert(ReadState {
                    channel,
                    last_message_id: 0,
                    mention_count: 0,
                });
                entry.mention_count += 1;
            }
            let muted = self.channel(channel).is_some_and(|c| self.is_muted(c));
            // As Discord: a muted DM is quiet; in a server, "All Messages"
            // hears everything unless muted, "Only @mentions" your
            // mentions (and @everyone unless muted), "Nothing" nothing.
            let level = self.channel(channel).map_or(1, |c| self.notify_level(c));
            let wanted = if is_dm {
                self.settings.notify_dms && !muted
            } else {
                self.settings.notify_mentions
                    && match level {
                        0 => !muted || direct,
                        1 => direct || (message.mention_everyone && !muted),
                        _ => false,
                    }
            };
            if wanted && self.settings.notify_sound {
                // Discord's own message sound, in the app; the system
                // notification then stays silent.
                self.sound(crate::sounds::Sound::Message);
            }
            if self.settings.notifications && wanted {
                self.notify_message(&message);
            }
            self.update_tray();
        }

        if let Some(history) = self.histories.get_mut(&channel) {
            let pending = message.nonce.as_ref().and_then(|nonce| {
                history
                    .messages
                    .iter()
                    .position(|m| m.pending && m.nonce.as_ref() == Some(nonce))
            });
            match pending {
                Some(index) => history.messages[index] = message,
                None if !history.messages.iter().any(|m| m.id == message.id) => {
                    history.messages.push(message)
                }
                None => {}
            }
            let keep = if self.selected_channel == Some(channel) { KEEP_OPEN } else { KEEP_HIDDEN };
            if history.messages.len() > keep {
                let excess = history.messages.len() - keep;
                history.messages.drain(..excess);
                history.complete = false;
            }
        }
    }

    fn notify_message(&mut self, message: &Message) {
        let author = message.author.display_name().to_owned();
        let title = match message.guild_id.and_then(|g| self.guild(g)) {
            Some(guild) => {
                let channel = self
                    .channel(message.channel_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                format!("{author} (#{channel}, {})", guild.name)
            }
            None => author,
        };
        let body = if !self.settings.notify_preview {
            t!("New message").to_owned()
        } else if !message.content.is_empty() {
            let text = crate::text::resolve(&message.content, |k, id| self.name_for(k, id));
            text.chars().take(200).collect()
        } else if !message.attachments.is_empty() {
            format!("📎 {}", t!("Attachment"))
        } else {
            t!("New message").to_owned()
        };
        self.commands.send(Command::Notify {
            title,
            body,
            channel: message.channel_id,
            // The app plays Discord's sound itself.
            sound: false,
        });
    }

    fn mark_read(&mut self, channel: Id, message: Id, tell_discord: bool) {
        let entry = self.read.entry(channel).or_insert(ReadState {
            channel,
            last_message_id: 0,
            mention_count: 0,
        });
        let changed = entry.last_message_id < message || entry.mention_count > 0;
        entry.last_message_id = entry.last_message_id.max(message);
        entry.mention_count = 0;
        if changed && tell_discord {
            self.commands.send(Command::Ack { channel, message });
        }
    }

    // ---- actions ---------------------------------------------------------

    pub fn run_actions(&mut self) {
        for action in std::mem::take(&mut self.actions) {
            self.run(action);
        }
        self.record_place();
    }

    /// Puts a channel in its new place, as Discord's drag and drop: among
    /// the channels of its kind (text or voice, since voice ones always go
    /// after) in the category it was dropped in, numbered again from zero,
    /// at once here and then through PATCH /guilds/{id}/channels.
    fn move_channel(&mut self, guild_id: Id, moved: Id, parent: Option<Id>, anchor: Anchor) {
        let me = self.me_id();
        let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) else { return };
        if guild.my_permissions(me) & (1 << 4) == 0 {
            return;
        }
        let voice = |k: ChannelKind| matches!(k, ChannelKind::Voice | ChannelKind::Stage);
        let Some(kind) = guild.channels.iter().find(|c| c.id == moved).map(|c| c.kind) else { return };
        let mut siblings: Vec<&Channel> = guild
            .channels
            .iter()
            .filter(|c| c.parent_id == parent && c.id != moved && !matches!(c.kind, ChannelKind::Category | ChannelKind::Thread) && voice(c.kind) == voice(kind))
            .collect();
        siblings.sort_by_key(|c| (c.position, c.id));
        let mut order: Vec<Id> = siblings.iter().map(|c| c.id).collect();
        // An anchor of the other kind means the start or the end: a text
        // channel dropped among voice ones goes after the texts.
        let other = |id: Id| guild.channels.iter().find(|c| c.id == id).is_some_and(|c| voice(c.kind) != voice(kind));
        let index = match anchor {
            Anchor::Start => 0,
            Anchor::Before(id) | Anchor::After(id) if other(id) => if voice(kind) { 0 } else { order.len() },
            Anchor::Before(id) => order.iter().position(|c| *c == id).unwrap_or(0),
            Anchor::After(id) => order.iter().position(|c| *c == id).map_or(order.len(), |i| i + 1),
        };
        order.insert(index.min(order.len()), moved);
        let mut body = Vec::new();
        for (position, id) in order.iter().enumerate() {
            if let Some(channel) = guild.channels.iter_mut().find(|c| c.id == *id) {
                if *id == moved {
                    channel.parent_id = parent;
                }
                channel.position = position as i64;
            }
            let mut entry = serde_json::json!({ "id": id.to_string(), "position": position });
            if *id == moved {
                entry["parent_id"] = parent.map_or(serde_json::Value::Null, |p| serde_json::Value::String(p.to_string()));
                entry["lock_permissions"] = serde_json::Value::Bool(false);
            }
            body.push(entry);
        }
        self.commands.send(Command::Api { key: "positions".into(), method: "PATCH", path: format!("/guilds/{guild_id}/channels"), body: Some(serde_json::Value::Array(body)) });
    }

    /// Puts a category before another one, or last, numbering them all again.
    fn move_category(&mut self, guild_id: Id, moved: Id, before: Option<Id>) {
        let me = self.me_id();
        let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) else { return };
        if guild.my_permissions(me) & (1 << 4) == 0 {
            return;
        }
        let mut categories: Vec<&Channel> = guild.channels.iter().filter(|c| c.kind == ChannelKind::Category && c.id != moved).collect();
        categories.sort_by_key(|c| (c.position, c.id));
        let mut order: Vec<Id> = categories.iter().map(|c| c.id).collect();
        let index = before.and_then(|b| order.iter().position(|c| *c == b)).unwrap_or(order.len());
        order.insert(index, moved);
        let mut body = Vec::new();
        for (position, id) in order.iter().enumerate() {
            if let Some(channel) = guild.channels.iter_mut().find(|c| c.id == *id) {
                channel.position = position as i64;
            }
            body.push(serde_json::json!({ "id": id.to_string(), "position": position }));
        }
        self.commands.send(Command::Api { key: "positions".into(), method: "PATCH", path: format!("/guilds/{guild_id}/channels"), body: Some(serde_json::Value::Array(body)) });
    }

    /// The pinned messages beside the open channel, or away again.
    pub fn toggle_pins(&mut self) {
        let Some(channel) = self.selected_channel else { return };
        if matches!(self.side, Some(Side::Pins(_))) {
            self.side = None;
        } else {
            self.side = Some(Side::Pins(None));
            self.commands.send(crate::admin::get("pins", format!("/channels/{channel}/messages/pins?limit=50")));
        }
    }

    /// The search beside the open channel, or away again.
    pub fn toggle_search(&mut self) {
        if self.selected_channel.is_none() {
            return;
        }
        self.side = if matches!(self.side, Some(Side::Search { .. })) { None } else { Some(Side::Search { query: String::new(), results: None }) };
    }

    /// The member list of a server's channel, shown or hidden.
    pub fn toggle_members(&mut self) {
        let Some(channel) = self.selected_channel else { return };
        let Some(guild) = self.channel(channel).and_then(|c| c.guild_id) else { return };
        self.show_members = !self.show_members;
        if self.show_members {
            self.watch_members(guild, channel, 100);
        }
    }

    /// The title bar's inbox, opened on "Unreads" or closed.
    pub fn toggle_inbox(&mut self) {
        self.inbox = if self.inbox.is_some() { None } else { Some(Inbox { tab: InboxTab::Unread, mentions: None }) };
    }

    /// Where the window is now.
    pub fn place(&self) -> Place {
        Place { guild: self.selected_guild, channel: self.selected_channel }
    }

    /// Remembers the place just left for "Back", once per move, whatever
    /// made it (a click, a link, a notification). A new move drops what
    /// "Forward" had, as a browser does.
    fn record_place(&mut self) {
        if !matches!(self.auth, AuthView::LoggedIn) {
            return;
        }
        let now = self.place();
        if let Some(before) = self.place.replace(now)
            && before != now
        {
            self.back.push(before);
            if self.back.len() > 100 {
                self.back.remove(0);
            }
            self.forward.clear();
        }
    }

    /// Goes to a place from the history, without recording the move. A
    /// channel gone since falls back to its server's first.
    fn go(&mut self, place: Place) {
        self.forums.retain(|_, _| false);
        let channel = place.channel.and_then(|c| self.channel(c)).map(|c| (c.id, c.kind));
        match channel {
            Some((channel, kind)) => {
                self.selected_guild = place.guild;
                if let Some(guild) = place.guild {
                    self.subscribe(guild);
                }
                if kind == ChannelKind::Forum {
                    self.run(Action::OpenForum(channel));
                } else {
                    self.open_channel(channel);
                }
            }
            None if place.guild.is_some_and(|g| self.guild(g).is_some()) => self.select_guild(place.guild),
            None => {
                self.selected_guild = None;
                self.selected_channel = None;
            }
        }
        self.place = Some(self.place());
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::SelectGuild(guild) => {
                self.forums.retain(|_, _| false);
                self.select_guild(guild);
            }
            Action::SelectChannel(channel) => {
                self.remembered.insert(self.selected_guild, channel);
                self.open_channel(channel);
            }
            Action::JumpTo { channel, message } => {
                self.jump = Some(Jump { channel, message, at: Instant::now(), scrolled: false, pages: 0 });
            }
            Action::LoadOlder(channel) => {
                let history = self.histories.entry(channel).or_default();
                if !history.loading {
                    history.loading = true;
                    history.error = None;
                    let before = history.messages.iter().find(|m| !m.pending).map(|m| m.id);
                    self.commands.send(Command::LoadMessages { channel, before });
                }
            }
            Action::Send => self.send(),
            Action::Reply { id, author } => {
                self.editing = None;
                self.replying = Some((id, author));
                self.focus_composer = true;
            }
            Action::StartEdit { id, content } => {
                // Edited in place, in the message, as Discord does.
                self.editing = Some(id);
                self.edit_text = content;
                self.focus_edit = true;
            }
            Action::SaveEdit => {
                let (Some(id), Some(channel)) = (self.editing.take(), self.selected_channel) else { return };
                let content = std::mem::take(&mut self.edit_text).trim().to_owned();
                if content.is_empty() {
                    self.commands.send(Command::Delete { channel, message: id });
                    return;
                }
                if let Some(m) = self
                    .histories
                    .get_mut(&channel)
                    .and_then(|h| h.messages.iter_mut().find(|m| m.id == id))
                {
                    if m.content == content {
                        return;
                    }
                    m.content = content.clone();
                    m.edited = true;
                }
                self.commands.send(Command::Edit { channel, message: id, content });
                self.focus_composer = true;
            }
            Action::CancelEdit => {
                self.editing = None;
                self.edit_text.clear();
                self.focus_composer = true;
            }
            Action::NewDm(users) => {
                let recipients: Vec<String> = users.iter().map(ToString::to_string).collect();
                self.commands.send(Command::Api {
                    key: "dm-created".into(),
                    method: "POST",
                    path: "/users/@me/channels".into(),
                    body: Some(serde_json::json!({ "recipients": recipients })),
                });
                self.modal = None;
            }
            Action::Delete { channel, id } => {
                self.commands.send(Command::Delete { channel, message: id });
            }
            Action::React { channel, message, emoji, add } => {
                // Shown at once; the gateway's echo is not counted twice.
                if let Some(m) = self
                    .histories
                    .get_mut(&channel)
                    .and_then(|h| h.messages.iter_mut().find(|m| m.id == message))
                {
                    match m.reactions.iter_mut().position(|r| r.emoji.api_name() == emoji) {
                        Some(i) => {
                            let r = &mut m.reactions[i];
                            if add && !r.me {
                                r.count += 1;
                                r.me = true;
                            } else if !add && r.me {
                                r.count = r.count.saturating_sub(1);
                                r.me = false;
                                if r.count == 0 {
                                    m.reactions.remove(i);
                                }
                            }
                        }
                        None if add => {
                            let parsed = match emoji.split_once(':') {
                                Some((name, id)) => crate::discord::model::Emoji {
                                    id: id.parse().ok(),
                                    name: name.into(),
                                    animated: false,
                                },
                                None => crate::discord::model::Emoji {
                                    id: None,
                                    name: emoji.clone(),
                                    animated: false,
                                },
                            };
                            m.reactions.push(crate::discord::model::Reaction {
                                emoji: parsed,
                                count: 1,
                                me: true,
                            });
                        }
                        None => {}
                    }
                }
                self.commands.send(Command::React { channel, message, emoji, add });
            }
            Action::CancelCompose => {
                self.replying = None;
                self.files.clear();
            }
            Action::Logout => {
                let me = self.me_id().to_string();
                self.settings.accounts.retain(|a| a.id != me);
                self.settings.save();
                self.commands.send(Command::Voice(VoiceCommand::Leave));
                self.commands.send(Command::Logout);
            }
            Action::NewQr => self.commands.send(Command::StartQrLogin),
            Action::PasswordLogin => {
                let form = &mut self.login_form;
                if form.login.trim().is_empty() || form.password.is_empty() {
                    form.error = Some(t!("Fill in the email and the password.").into());
                    return;
                }
                form.busy = true;
                form.error = None;
                self.commands.send(Command::PasswordLogin {
                    login: form.login.trim().to_owned(),
                    password: std::mem::take(&mut form.password),
                });
            }
            Action::MfaCode => {
                let form = &mut self.login_form;
                if form.code.trim().is_empty() {
                    return;
                }
                form.busy = true;
                form.error = None;
                self.commands.send(Command::MfaCode { kind: form.mfa_kind, code: std::mem::take(&mut form.code) });
            }
            Action::MfaKind(kind) => {
                self.login_form.mfa_kind = kind;
                self.login_form.code.clear();
                if kind == "sms" {
                    self.login_form.busy = true;
                    self.commands.send(Command::SendSms);
                }
            }
            Action::TokenLogin => {
                let token = std::mem::take(&mut self.token_input);
                if !token.trim().is_empty() {
                    self.auth = AuthView::Checking;
                    self.commands.send(Command::LoginWithToken(token));
                }
            }
            Action::OpenProfile { user, guild, at } => {
                self.commands.send(Command::Profile { user: user.id, guild });
                self.profile = Some(ProfileView {
                    user,
                    guild,
                    state: ProfileState::Loading,
                    at,
                    above: false,
                });
            }
            Action::OpenTagCard { guild, at } => {
                if !self.guild_profiles.contains_key(&guild) {
                    self.guild_profiles.insert(guild, None);
                    self.commands.send(crate::admin::get(&format!("guild-profile:{guild}"), format!("/guilds/{guild}/profile")));
                }
                self.tag_card = Some(TagCard { guild, at });
            }
            Action::JoinTagGuild(guild) => {
                self.tag_card = None;
                self.commands.send(Command::Api { key: "tag-join".into(), method: "PUT", path: format!("/guilds/{guild}/members/@me"), body: Some(serde_json::json!({})) });
            }
            Action::OpenOwnProfile { at } => {
                let Some(user) = self.me.clone() else { return };
                self.commands.send(Command::Profile { user: user.id, guild: None });
                self.profile = Some(ProfileView {
                    user,
                    guild: None,
                    state: ProfileState::Loading,
                    at,
                    above: true,
                });
            }
            Action::MessageUser(user) => {
                match self
                    .dms
                    .iter()
                    .find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == user))
                {
                    Some(channel) => {
                        let id = channel.id;
                        self.profile = None;
                        self.select_guild(None);
                        self.open_channel(id);
                    }
                    None => self.commands.send(Command::OpenDm { user }),
                }
            }
            Action::SendDm { user, text } => {
                let existing = self.dms.iter().find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == user)).map(|c| c.id);
                match existing {
                    Some(id) => {
                        self.profile = None;
                        self.select_guild(None);
                        self.open_channel(id);
                        self.send_text(text);
                    }
                    None => {
                        self.pending_dm = Some((user, text));
                        self.commands.send(Command::OpenDm { user });
                    }
                }
            }
            Action::CallUser(user) => {
                if let Some(channel) = self
                    .dms
                    .iter()
                    .find(|c| c.kind == ChannelKind::Dm && c.recipients.iter().any(|u| u.id == user))
                {
                    let id = channel.id;
                    self.profile = None;
                    self.commands.send(Command::Call { guild: None, channel: id, ring: true });
                } else {
                    self.notify(t!("Open a conversation with them before calling."));
                }
            }
            Action::JoinVoice { guild, channel, ring } => {
                // Discord's switch warning: asked before leaving one voice
                // channel for another.
                if self.settings.switch_voice_warning
                    && let Some((_, current)) = self.voice.channel
                    && current != channel
                {
                    // Discord's "You sure?" (its voice channel change
                    // confirmation), with "Don't ask again" under it.
                    let name = self.channel(channel).map(Channel::title).unwrap_or_default();
                    let body = t!("Looks like you're in another voice channel. Are you sure you want to switch to {channel}?", channel = name);
                    self.modal = Some(Modal::Confirm { title: t!("You sure?").into(), body, button: t!("Confirm").into(), then: Confirmed::SwitchVoice { guild, channel } });
                    return;
                }
                let ring = ring && !self.calls.contains_key(&channel);
                self.commands.send(Command::Call { guild, channel, ring });
            }
            Action::LeaveVoice => self.commands.send(Command::Voice(VoiceCommand::Leave)),
            Action::ToggleMute => {
                self.voice.mute = !self.voice.mute;
                self.sound(if self.voice.mute { crate::sounds::Sound::Mute } else { crate::sounds::Sound::Unmute });
                if !self.voice.mute && self.voice.deaf {
                    self.voice.deaf = false;
                    self.commands.send(Command::Voice(VoiceCommand::Deaf(false)));
                }
                self.commands.send(Command::Voice(VoiceCommand::Mute(self.voice.mute)));
                self.update_tray();
            }
            Action::ToggleDeaf => {
                self.voice.deaf = !self.voice.deaf;
                self.sound(if self.voice.deaf { crate::sounds::Sound::Deafen } else { crate::sounds::Sound::Undeafen });
                self.voice.mute = self.voice.deaf;
                self.commands.send(Command::Voice(VoiceCommand::Deaf(self.voice.deaf)));
                self.commands.send(Command::Voice(VoiceCommand::Mute(self.voice.mute)));
                self.update_tray();
            }
            Action::AcceptCall(channel) => {
                let me = self.me_id();
                self.calls.entry(channel).or_default().ringing.retain(|u| *u != me);
                self.commands.send(Command::Call { guild: None, channel, ring: false });
                self.select_guild(None);
                self.open_channel(channel);
            }
            Action::DeclineCall(channel) => {
                let me = self.me_id();
                self.calls.entry(channel).or_default().ringing.retain(|u| *u != me);
                self.commands.send(Command::DeclineCall { channel });
            }
            Action::SetRegion(region) => {
                if let Some((guild, channel)) = self.voice.channel {
                    self.commands.send(Command::CallRegion { guild, channel, region });
                }
            }
            Action::OpenSettings(tab) => {
                if tab == SettingsTab::System {
                    self.refresh_autostart();
                }
                // What each page shows from the API, asked for when it opens.
                let wanted: &[(&str, &str)] = match tab {
                    SettingsTab::Account => &[("settings:me", "/users/@me"), ("settings:sessions", "/auth/sessions")],
                    SettingsTab::Nitro => &[("settings:me", "/users/@me")],
                    SettingsTab::DataPrivacy => &[("settings:consent", "/users/@me/consent")],
                    SettingsTab::ServerBoost => &[("settings:boosts", "/users/@me/guilds/premium/subscription-slots")],
                    SettingsTab::Subscriptions => &[("settings:subscriptions", "/users/@me/billing/subscriptions")],
                    SettingsTab::GiftInventory => &[("settings:gifts", "/users/@me/entitlements/gifts"), ("settings:gift-codes", "/users/@me/entitlements/gift-codes")],
                    SettingsTab::Billing => &[("settings:payment-sources", "/users/@me/billing/payment-sources"), ("settings:payments", "/users/@me/billing/payments?limit=20")],
                    SettingsTab::ConnectedApps => &[("settings:connections", "/users/@me/connections"), ("settings:apps", "/oauth2/tokens")],
                    _ => &[],
                };
                for (key, path) in wanted {
                    self.settings_data.insert(key, None);
                    self.commands.send(crate::admin::get(key, (*path).to_owned()));
                }
                if tab == SettingsTab::Profile
                    && let Some(me) = self.me.clone()
                {
                    self.profile_draft = Some(ProfileDraft {
                        display_name: me.global_name.clone().unwrap_or_default(),
                        ..ProfileDraft::default()
                    });
                    self.commands.send(crate::admin::get("my-profile", format!("/users/{}/profile", me.id)));
                }
                if tab == SettingsTab::Voice {
                    self.devices = crate::voice::devices();
                }
                if tab == SettingsTab::Appearance {
                    self.palettes = crate::theme::catalog();
                }
                self.page = Some(tab);
                self.profile = None;
            }
            Action::CloseSettings => {
                self.page = None;
                if std::mem::take(&mut self.mic_test) {
                    self.commands.send(Command::Voice(VoiceCommand::MicTest(false)));
                }
            }
            Action::SettingsChanged => {
                self.sync_global_keybinds();
                // The tray's menu speaks the interface's language too.
                self.update_tray();
                crate::discord::rest::NO_TRACK.store(
                    self.plugin(Plugin::NoTrack),
                    std::sync::atomic::Ordering::Relaxed,
                );
                self.settings.save();
                self.theme_dirty = true;
                self.commands.send(Command::RichPresence(self.rich_presence()));
                self.commands.send(Command::Voice(VoiceCommand::Settings(
                    crate::backend::audio_settings(&self.settings),
                )));
            }
            Action::AttachFiles => {
                if let Some(files) = self.settings.file_dialog().set_title(t!("Upload Files")).pick_files() {
                    if let Some(first) = files.first() {
                        self.settings.picked(first);
                    }
                    self.files.extend(files.into_iter().map(crate::backend::Upload::new));
                    self.files.truncate(10);
                    self.focus_composer = true;
                }
            }
            Action::MarkRead(channels) => {
                let reads: Vec<(Id, Id)> = channels
                    .into_iter()
                    .filter_map(|c| Some((c, *self.latest.get(&c)?)))
                    .collect();
                for (channel, latest) in &reads {
                    self.mark_read(*channel, *latest, false);
                    self.unread_from.remove(channel);
                }
                if !reads.is_empty() {
                    self.commands.send(Command::AckBulk(reads));
                }
                self.update_tray();
            }
            Action::Mute { guild, channel, seconds } => {
                let mute = seconds.map(|s| (s > 0).then(|| now_ms() + s as u64 * 1000));
                match (guild, channel) {
                    (_, Some(c)) => self.mutes.set(c, mute, false),
                    (Some(g), None) => self.mutes.set(g, mute, true),
                    (None, None) => {}
                }
                self.commands.send(Command::Mute { guild, channel, seconds });
            }
            Action::MarkUnread { channel, message } => {
                let before = message.saturating_sub(1);
                let me = self.me_id();
                let mentions = self
                    .histories
                    .get(&channel)
                    .map_or(0, |h| h.messages.iter().filter(|m| m.id >= message && m.mentions.iter().any(|u| u.id == me)).count() as u32);
                let entry = self.read.entry(channel).or_insert(ReadState { channel, last_message_id: 0, mention_count: 0 });
                entry.last_message_id = before;
                entry.mention_count = mentions;
                self.unread_from.insert(channel, before);
                self.commands.send(Command::Api {
                    key: "unread".into(),
                    method: "POST",
                    path: format!("/channels/{channel}/messages/{before}/ack"),
                    body: Some(serde_json::json!({ "manual": true, "mention_count": mentions })),
                });
                self.update_tray();
            }
            Action::NotifyLevel { guild, channel, level } => {
                let id = channel.or(guild);
                if let Some(id) = id {
                    if level <= 2 {
                        self.mutes.levels.insert(id, level);
                    } else {
                        self.mutes.levels.remove(&id);
                    }
                }
                self.commands.send(Command::NotifyLevel { guild, channel, level });
            }
            Action::CloseDm(channel) => {
                self.dms.retain(|c| c.id != channel);
                if self.selected_channel == Some(channel) {
                    self.selected_channel = None;
                }
                self.commands.send(Command::CloseDm(channel));
            }
            Action::LeaveGuild(guild) => self.commands.send(Command::LeaveGuild(guild)),
            Action::SetStatus(status, minutes) => {
                self.status = status.to_owned();
                let until = minutes.map(|m| now_ms() + m * 60_000);
                self.status_until = until;
                self.commands.send(Command::SetStatus { status: status.to_owned(), until });
            }
            Action::SetCustomStatus { text, minutes } => {
                let text = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
                self.custom_status = text.clone();
                let until = minutes.map(|m| now_ms() + m * 60_000);
                self.modal = None;
                self.commands.send(Command::SetCustomStatus { text, until });
            }
            Action::MoveChannel { guild, channel, parent, anchor } => self.move_channel(guild, channel, parent, anchor),
            Action::MoveCategory { guild, category, before } => self.move_category(guild, category, before),
            Action::Back => {
                if let Some(place) = self.back.pop() {
                    self.forward.push(self.place());
                    self.go(place);
                }
            }
            Action::Forward => {
                if let Some(place) = self.forward.pop() {
                    self.back.push(self.place());
                    self.go(place);
                }
            }
            Action::OpenFriends => {
                self.selected_guild = None;
                self.selected_channel = None;
                self.remembered.remove(&None);
            }
            Action::AddFriend => {
                let name = std::mem::take(&mut self.friend_input);
                if !name.trim().is_empty() {
                    self.commands.send(Command::AddFriend(name.trim().to_owned()));
                }
            }
            Action::Relationship { user, kind } => {
                match kind {
                    Some(1) => {
                        if let Some(r) = self.relationships.get_mut(&user) {
                            r.kind = 1;
                        }
                    }
                    Some(2) => {
                        if let Some(r) = self.relationships.get_mut(&user) {
                            r.kind = 2;
                        }
                    }
                    _ => {
                        self.relationships.remove(&user);
                    }
                }
                self.commands.send(Command::Relationship { user, kind });
            }
            Action::OpenGuildSettings(guild) => {
                let name = self.guild(guild).map(|g| g.name.clone()).unwrap_or_default();
                let admin = crate::admin::GuildAdmin::new(guild, name);
                if let Some(command) = admin.load(crate::admin::Tab::Overview) {
                    self.commands.send(command);
                }
                self.admin = Some(admin);
                self.profile = None;
            }
            Action::GuildTab(tab) => {
                if let Some(admin) = &mut self.admin {
                    // Unsaved changes hold the page, as in Discord: the bar
                    // shakes instead.
                    if admin.unsaved() && admin.tab != tab {
                        admin.nag = Some(Instant::now());
                        return;
                    }
                    admin.tab = tab;
                    admin.error = None;
                    if let Some(command) = admin.load(tab) {
                        self.commands.send(command);
                    }
                }
            }
            Action::CloseGuildSettings => match &mut self.admin {
                Some(admin) if admin.unsaved() => admin.nag = Some(Instant::now()),
                _ => self.admin = None,
            },
            Action::SwitchAccount(id) => {
                if id != self.me_id() {
                    self.clear_account();
                    self.auth = AuthView::Checking;
                    self.commands.send(Command::SwitchAccount(id));
                }
            }
            Action::AddAccount => self.commands.send(Command::AddAccount),
            Action::RemoveAccount(id) => {
                self.settings.accounts.retain(|a| a.id != id);
                self.settings.save();
                if let Ok(id) = id.parse() {
                    self.commands.send(Command::RemoveAccount(id));
                }
            }
            Action::InstallUpdate => match crate::updates::restart(&self.updates) {
                Ok(()) => self.quit = true,
                Err(error) => self.notify(t!("Couldn't update: {error}", error = format!("{error:#}"))),
            },
            Action::OpenForum(forum) => {
                self.remembered.insert(self.selected_guild, forum);
                self.selected_channel = Some(forum);
                self.forums.insert(forum, None);
                self.commands.send(Command::ForumPosts { forum });
            }
        }
    }

    fn subscribe(&mut self, guild: Id) {
        if self.subscribed.insert(guild) {
            self.commands.send(Command::Subscribe { guild });
        }
    }

    pub fn select_guild(&mut self, guild: Option<Id>) {
        self.selected_guild = guild;
        if let Some(guild) = guild {
            self.subscribe(guild);
        }
        let me = self.me_id();
        let next = self.remembered.get(&guild).copied().or_else(|| {
            let guild = self.guild(guild?)?;
            let mut channels: Vec<&Channel> = guild
                .channels
                .iter()
                .filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement))
                .filter(|c| guild.can_view(me, c))
                .collect();
            channels.sort_by_key(|c| {
                let parent = c
                    .parent_id
                    .and_then(|p| guild.channels.iter().find(|x| x.id == p))
                    .map_or(-1, |p| p.position);
                (parent, c.position)
            });
            channels.first().map(|c| c.id)
        });
        match next {
            Some(channel) if self.channel(channel).is_some_and(|c| c.kind == ChannelKind::Forum) => {
                self.run(Action::OpenForum(channel));
            }
            Some(channel) => self.open_channel(channel),
            None => self.selected_channel = None,
        }
    }

    pub fn open_channel(&mut self, channel: Id) {
        if let Some(previous) = self.selected_channel
            && previous != channel
            && let Some(history) = self.histories.get_mut(&previous)
            && history.messages.len() > KEEP_HIDDEN
        {
            let excess = history.messages.len() - KEEP_HIDDEN;
            history.messages.drain(..excess);
            history.complete = false;
        }
        if self.selected_channel != Some(channel) {
            self.composer.clear();
            self.replying = None;
            self.editing = None;
            self.files.clear();
        }
        self.selected_channel = Some(channel);
        self.focus_composer = true;
        let guild = self.channel(channel).and_then(|c| c.guild_id);
        match guild {
            Some(guild) if self.show_members => self.watch_members(guild, channel, 100),
            _ => self.member_list = None,
        }
        let history = self.histories.entry(channel).or_default();
        if history.messages.is_empty() && !history.loading {
            history.loading = true;
            history.error = None;
            self.commands.send(Command::LoadMessages { channel, before: None });
        }
        if let Some(&latest) = self.latest.get(&channel) {
            match self.read.get(&channel).map(|r| r.last_message_id) {
                Some(read) if read < latest && read > 0 => {
                    self.unread_from.insert(channel, read);
                }
                _ => {
                    self.unread_from.remove(&channel);
                }
            }
            self.mark_read(channel, latest, true);
        }
        self.update_tray();
    }

    /// Sends `text` to the open channel and leaves what the chat's own box
    /// holds (text, files, the reply) as it was: the box under a profile.
    fn send_text(&mut self, text: String) {
        let draft = std::mem::replace(&mut self.composer, text);
        let files = std::mem::take(&mut self.files);
        let replying = self.replying.take();
        let tokens = std::mem::take(&mut self.mention_tokens);
        self.send();
        self.composer = draft;
        self.files = files;
        self.replying = replying;
        self.mention_tokens = tokens;
    }

    fn send(&mut self) {
        let Some(channel) = self.selected_channel else { return };
        let mut content = self.composer.trim().to_owned();
        let files = std::mem::take(&mut self.files);
        if content.is_empty() && files.is_empty() {
            return;
        }
        // Autocompleted names become the tokens Discord reads, longest first
        // so "@Ana Clara" is not taken for "@Ana".
        let mut tokens = std::mem::take(&mut self.mention_tokens);
        tokens.sort_by_key(|(shown, _)| std::cmp::Reverse(shown.len()));
        for (shown, token) in tokens {
            content = content.replace(&shown, &token);
        }
        if self.plugin(Plugin::ClearUrls) {
            content = crate::plugins::clear_urls(&content);
        }
        self.composer.clear();
        let nonce = now_snowflake().to_string();
        let reply_to = self.replying.take().map(|(id, _)| id);
        if let Some(me) = self.me.clone() {
            let label = if files.is_empty() {
                content.clone()
            } else {
                let names: Vec<String> = files.iter().map(|f| format!("📎 {}", f.name)).collect();
                format!("{content}\n{}", names.join("\n")).trim().to_owned()
            };
            let history = self.histories.entry(channel).or_default();
            history.messages.push(Message {
                id: now_snowflake(),
                channel_id: channel,
                guild_id: self.selected_guild,
                author: me,
                content: label,
                edited: false,
                attachments: Vec::new(),
                embeds: Vec::new(),
                mentions: Vec::new(),
                mention_roles: Vec::new(),
                mention_everyone: false,
                reply: None,
                reply_id: None,
                reply_author: None,
                nonce: Some(nonce.clone()),
                pending: true,
                kind: 0,
                reactions: Vec::new(),
                member: None,
                stickers: Vec::new(),
                deleted: false,
                edits: Vec::new(),
                pinned: false,
                poll: None,
                forwarded: false,
                components: Vec::new(),
                origin: None,
                interaction: None,
            });
        }
        self.last_typing = None;
        let ping_reply = !self.plugin(Plugin::NoReplyMention);
        self.commands.send(Command::Send { channel, content, nonce, reply_to, ping_reply, files });
    }

    /// Tells the channel we are typing, at most every eight seconds.
    pub fn typed(&mut self) {
        let Some(channel) = self.selected_channel else { return };
        if self.editing.is_some()
            || self.composer.trim().is_empty()
            || self.plugin(Plugin::SilentTyping)
        {
            return;
        }
        if self
            .last_typing
            .is_none_or(|at| at.elapsed() > Duration::from_secs(8))
        {
            self.last_typing = Some(Instant::now());
            self.commands.send(Command::Typing { channel });
        }
    }

    // ---- window ----------------------------------------------------------

    /// Work that must happen whatever state the window is in.
    pub fn window_logic(&mut self, ctx: &egui::Context) {
        self.focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        self.keybinds.focused.store(self.focused, std::sync::atomic::Ordering::Relaxed);
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit
            && self.settings.keep_running_in_tray
            && self.tray.as_ref().is_some_and(fastframe_tray::Tray::is_shown)
        {
            self.hide = true;
        }
        if self.hide {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if std::mem::take(&mut self.show) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.pump(Some(&ctx));
        // Files dropped on the window become attachments.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if !dropped.is_empty() && self.selected_channel.is_some() {
            self.files.extend(dropped.into_iter().map(crate::backend::Upload::new));
            self.files.truncate(10);
        }
        if let Some(text) = self.clipboard.take() {
            ctx.copy_text(text);
        }
        crate::ui::show(self, ui);
        self.run_actions();
        self.images.end_frame();
    }
}

impl fastframe_shell::Resident for App {
    fn closed(&self) -> fastframe_shell::Closed {
        if !self.quit && self.hide {
            fastframe_shell::Closed::Hide
        } else {
            fastframe_shell::Closed::Quit
        }
    }

    fn window_gone(&mut self) {
        App::window_gone(self);
    }

    fn headless_frame(&mut self, _ctx: &egui::Context) -> fastframe_shell::Headless {
        self.pump(None);
        self.run_actions();
        if self.quit {
            fastframe_shell::Headless::Quit
        } else if self.show {
            fastframe_shell::Headless::Show
        } else {
            fastframe_shell::Headless::WaitFor(Duration::from_secs(30))
        }
    }

    fn start_hidden(&mut self) -> bool {
        self.tray.is_some()
    }

    fn shutdown(&mut self) {
        self.commands.send(Command::Voice(VoiceCommand::Leave));
        self.settings.save();
    }
}


/// The code in "discord.gg/abc", "https://discord.com/invite/abc" or "abc".
pub fn invite_code(text: &str) -> String {
    text.trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Now, in Unix milliseconds.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
