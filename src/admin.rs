//! Server settings state: what the pages have loaded and are editing.
//! Each page asks the API through `Command::Api` with a key naming what
//! the answer is; `apply` files the answer under that key. Keys starting
//! with `do:` are changes: on success they reload the list they touched.

use serde_json::{Value, json};

use crate::backend::Command;
use crate::discord::model::Id;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Roles,
    Channels,
    Members,
    Invites,
    Bans,
    Emojis,
    Audit,
    Safety,
}

/// The side column as Discord groups it: the server's own pages, then
/// moderation, then people. Each page with the permission it needs (none
/// for 0). "Channels" is CordFast's: every channel's settings in one place.
pub const SECTIONS: &[(Option<&str>, &[(Tab, &str, u64)])] = &[
    (None, &[(Tab::Overview, n!("Overview"), 1 << 5), (Tab::Roles, n!("Roles"), 1 << 28), (Tab::Emojis, n!("Emoji"), 1 << 30), (Tab::Channels, n!("Channels"), 1 << 4)]),
    (Some(n!("Moderation")), &[(Tab::Safety, n!("Safety Setup"), 1 << 5), (Tab::Audit, n!("Audit Log"), 1 << 7), (Tab::Bans, n!("Bans"), 1 << 2)]),
    (Some(n!("User Management")), &[(Tab::Members, n!("Members"), 0), (Tab::Invites, n!("Invites"), 1 << 5)]),
];

/// What the overview and safety pages change, saved together through the
/// unsaved changes bar, as Discord's.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct GuildDraft {
    pub name: String,
    pub description: String,
    /// A new icon as a data URI, or "" to remove it; none while unchanged.
    pub icon: Option<String>,
    pub afk_channel: Option<Id>,
    pub afk_timeout: u64,
    pub system_channel: Option<Id>,
    /// Discord's system_channel_flags: each bit turns a kind of message off.
    pub system_flags: u64,
    pub notifications: u64,
    pub verification: u64,
    pub content_filter: u64,
}

impl GuildDraft {
    pub fn from(value: &Value) -> Self {
        let id = |key: &str| value.get(key).and_then(crate::discord::model::sf);
        let number = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
        Self {
            name: value.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
            description: value.get("description").and_then(Value::as_str).unwrap_or_default().to_owned(),
            icon: None,
            afk_channel: id("afk_channel_id"),
            afk_timeout: value.get("afk_timeout").and_then(Value::as_u64).unwrap_or(300),
            system_channel: id("system_channel_id"),
            system_flags: number("system_channel_flags"),
            notifications: number("default_message_notifications"),
            verification: number("verification_level"),
            content_filter: number("explicit_content_filter"),
        }
    }

    /// The PATCH body: only what differs from what was saved.
    pub fn changes(&self, saved: &Self) -> Value {
        let mut body = serde_json::Map::new();
        let snowflake = |id: Option<Id>| id.map_or(Value::Null, |i| Value::String(i.to_string()));
        if self.name != saved.name {
            body.insert("name".into(), json!(self.name.trim()));
        }
        if self.description != saved.description {
            body.insert("description".into(), json!(self.description));
        }
        if let Some(icon) = &self.icon {
            body.insert("icon".into(), if icon.is_empty() { Value::Null } else { json!(icon) });
        }
        if self.afk_channel != saved.afk_channel {
            body.insert("afk_channel_id".into(), snowflake(self.afk_channel));
        }
        if self.afk_timeout != saved.afk_timeout {
            body.insert("afk_timeout".into(), json!(self.afk_timeout));
        }
        if self.system_channel != saved.system_channel {
            body.insert("system_channel_id".into(), snowflake(self.system_channel));
        }
        if self.system_flags != saved.system_flags {
            body.insert("system_channel_flags".into(), json!(self.system_flags));
        }
        if self.notifications != saved.notifications {
            body.insert("default_message_notifications".into(), json!(self.notifications));
        }
        if self.verification != saved.verification {
            body.insert("verification_level".into(), json!(self.verification));
        }
        if self.content_filter != saved.content_filter {
            body.insert("explicit_content_filter".into(), json!(self.content_filter));
        }
        Value::Object(body)
    }
}

/// Permissions as Discord's role editor groups them: (group, [(name, bit)]).
pub const PERMISSIONS: &[(&str, &[(&str, u32)])] = &[
    (n!("General Server Permissions"), &[
        (n!("View Channels"), 10),
        (n!("Manage Channels"), 4),
        (n!("Manage Roles"), 28),
        (n!("Manage Expressions"), 30),
        (n!("View Audit Log"), 7),
        (n!("Manage Webhooks"), 29),
        (n!("Manage Server"), 5),
    ]),
    (n!("Membership Permissions"), &[
        (n!("Create Invite"), 0),
        (n!("Change Nickname"), 26),
        (n!("Manage Nicknames"), 27),
        (n!("Kick Members"), 1),
        (n!("Ban Members"), 2),
        (n!("Timeout Members"), 40),
    ]),
    (n!("Text Channel Permissions"), &[
        (n!("Send Messages"), 11),
        (n!("Send Messages in Threads"), 38),
        (n!("Create Public Threads"), 35),
        (n!("Create Private Threads"), 36),
        (n!("Embed Links"), 14),
        (n!("Attach Files"), 15),
        (n!("Add Reactions"), 6),
        (n!("Use External Emoji"), 18),
        (n!("Mention @everyone, @here, and All Roles"), 17),
        (n!("Manage Messages"), 13),
        (n!("Manage Threads"), 34),
        (n!("Read Message History"), 16),
        (n!("Send Text-to-Speech Messages"), 12),
    ]),
    (n!("Voice Channel Permissions"), &[
        (n!("Connect"), 20),
        (n!("Speak"), 21),
        (n!("Video"), 9),
        (n!("Use Voice Activity"), 25),
        (n!("Priority Speaker"), 8),
        (n!("Mute Members"), 22),
        (n!("Deafen Members"), 23),
        (n!("Move Members"), 24),
    ]),
    (n!("Advanced Permissions"), &[(n!("Administrator"), 3)]),
];

#[derive(Clone, Default, PartialEq, Debug)]
pub struct RoleDraft {
    pub id: Id,
    pub name: String,
    pub color: [u8; 3],
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: u64,
}

#[derive(Clone, Default, PartialEq)]
pub struct ChannelDraft {
    pub id: Id,
    pub name: String,
    pub topic: String,
    pub nsfw: bool,
    pub slowmode: u32,
}

pub struct GuildAdmin {
    pub guild: Id,
    pub tab: Tab,
    pub loaded: Option<Value>,
    /// The overview and safety pages: as saved, and as being edited.
    pub saved: Option<GuildDraft>,
    pub draft: Option<GuildDraft>,
    /// The new icon, shown before it is saved.
    pub icon_preview: Option<egui::TextureHandle>,
    /// When leaving with unsaved changes was refused: the bar shakes.
    pub nag: Option<std::time::Instant>,
    pub role: Option<RoleDraft>,
    /// The role as saved, to tell what changed.
    pub role_saved: Option<RoleDraft>,
    /// The role editor's tab: 0 Display, 1 Permissions.
    pub role_tab: u8,
    pub role_query: String,
    pub permission_query: String,
    /// An emoji being renamed in place: its id and the name typed.
    pub emoji_edit: Option<(Id, String)>,
    pub ban_query: String,
    pub channel: Option<ChannelDraft>,
    pub new_channel: String,
    pub new_channel_kind: u8,
    pub query: String,
    pub members: Option<Vec<Value>>,
    pub invites: Option<Vec<Value>>,
    pub bans: Option<Vec<Value>>,
    pub emojis: Option<Vec<Value>>,
    pub audit: Option<Value>,
    pub error: Option<String>,
}

impl GuildAdmin {
    pub fn new(guild: Id, name: String) -> Self {
        let _ = name;
        Self {
            guild,
            tab: Tab::Overview,
            loaded: None,
            saved: None,
            draft: None,
            icon_preview: None,
            nag: None,
            role: None,
            role_saved: None,
            role_tab: 0,
            role_query: String::new(),
            permission_query: String::new(),
            emoji_edit: None,
            ban_query: String::new(),
            channel: None,
            new_channel: String::new(),
            new_channel_kind: 0,
            query: String::new(),
            members: None,
            invites: None,
            bans: None,
            emojis: None,
            audit: None,
            error: None,
        }
    }

    /// The request that fills a tab when it opens.
    pub fn load(&self, tab: Tab) -> Option<Command> {
        let g = self.guild;
        let (key, path) = match tab {
            Tab::Overview | Tab::Safety => ("guild", format!("/guilds/{g}")),
            Tab::Members => {
                // The member safety search the web client uses.
                let query = self.query.trim();
                let and_query = if query.len() > 1 {
                    json!({ "usernames": { "or_query": [query] } })
                } else {
                    json!({})
                };
                return Some(Command::Api {
                    key: "members".into(),
                    method: "POST",
                    path: format!("/guilds/{g}/members-search"),
                    body: Some(json!({ "limit": 100, "or_query": {}, "and_query": and_query })),
                });
            }
            Tab::Invites => ("invites", format!("/guilds/{g}/invites")),
            Tab::Bans => ("bans", format!("/guilds/{g}/bans?limit=1000")),
            Tab::Emojis => ("emojis", format!("/guilds/{g}/emojis")),
            Tab::Audit => ("audit", format!("/guilds/{g}/audit-logs?limit=50")),
            Tab::Roles | Tab::Channels => return None,
        };
        Some(get(key, path))
    }

    /// Files an answer. Returns a notice to show, if any, and what to
    /// reload after a change.
    pub fn apply(&mut self, key: &str, result: Result<Value, String>) -> (Option<String>, Option<Tab>) {
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.clone());
                return (Some(error), None);
            }
        };
        self.error = None;
        let list = |v: Value| v.as_array().cloned();
        match key {
            "guild" => {
                let saved = GuildDraft::from(&value);
                self.draft = Some(saved.clone());
                self.saved = Some(saved);
                self.icon_preview = None;
                self.loaded = Some(value);
            }
            "members" => {
                if value.get("retry_after").is_some() {
                    return (Some(t!("Discord is indexing the members. Try again in a moment.").into()), None);
                }
                self.members = value.get("members").and_then(Value::as_array).map(|l| {
                    l.iter().map(|e| e.get("member").cloned().unwrap_or_else(|| e.clone())).collect()
                });
            }
            "invites" => self.invites = list(value),
            "bans" => self.bans = list(value),
            "emojis" => self.emojis = list(value),
            "audit" => self.audit = Some(value),
            key => {
                if let Some(rest) = key.strip_prefix("do:") {
                    let (tab, message) = rest.split_once(':').unwrap_or((rest, t!("Done.")));
                    let tab = match tab {
                        "overview" => Some(Tab::Overview),
                        "roles" => {
                            // Saved: what is shown is what is saved now.
                            self.role_saved.clone_from(&self.role);
                            None
                        }
                        "members" => Some(Tab::Members),
                        "invites" => Some(Tab::Invites),
                        "bans" => Some(Tab::Bans),
                        "emojis" => Some(Tab::Emojis),
                        _ => None,
                    };
                    return (Some(message.to_owned()), tab);
                }
            }
        }
        (None, None)
    }
}

pub fn get(key: &str, path: String) -> Command {
    Command::Api { key: key.into(), method: "GET", path, body: None }
}

/// A change; `done` is "tab:message" for the reload and the notice.
pub fn change(method: &'static str, path: String, body: Option<Value>, done: &str) -> Command {
    Command::Api { key: format!("do:{done}"), method, path, body }
}

pub fn role_body(draft: &RoleDraft) -> Value {
    let color = u32::from(draft.color[0]) << 16 | u32::from(draft.color[1]) << 8 | u32::from(draft.color[2]);
    json!({
        "name": draft.name,
        "color": color,
        "hoist": draft.hoist,
        "mentionable": draft.mentionable,
        "permissions": draft.permissions.to_string(),
    })
}

/// A picked image as the data URI Discord takes for icons and emoji.
pub fn image_data(path: &std::path::Path) -> Option<String> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 10 << 20 {
        return None;
    }
    let mime = match path.extension()?.to_string_lossy().to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

impl GuildAdmin {
    /// Whether the overview, safety or role pages hold changes not saved yet.
    pub fn unsaved(&self) -> bool {
        let guild = matches!(self.tab, Tab::Overview | Tab::Safety) && self.draft.is_some() && self.draft != self.saved;
        let role = self.tab == Tab::Roles && self.role.is_some() && self.role != self.role_saved;
        guild || role
    }
}

/// What each permission allows, as Discord's role editor explains it, in
/// the interface's language (the bundle's server-wide wording).
pub fn permission_note(bit: u32) -> &'static str {
    match bit {
        10 => t!("Allows members to view channels by default (excluding private channels)."),
        4 => t!("Allows members to create, edit, or delete channels."),
        28 => t!("Allows members to create new roles and edit or delete roles lower than their highest role. Also allows members to change permissions of individual channels that they have access to."),
        30 => t!("Allows members to edit or remove custom emoji, stickers, and sounds in this server."),
        7 => t!("Allows members to view a record of who made which changes in this server."),
        29 => t!("Allows members to create, edit, or delete webhooks, which can post messages from other apps or sites into this server."),
        5 => t!("Allow members to change this server's name, switch regions, view all invites, add apps to this server and create and update AutoMod rules."),
        0 => t!("Allows members to invite new people to this server."),
        26 => t!("Allows members to change their own nickname, a custom name for just this server."),
        27 => t!("Allows members to change the nicknames of other members."),
        1 => t!("Kick will remove other members from this server. Kicked members will be able to rejoin if they have another invite. If the server enables Member Requirements, this permission enables the ability to approve or reject members who request to join."),
        2 => t!("Allows members to permanently ban and delete the message history of other members from this server."),
        40 => t!("When you put a user in timeout they will not be able to send messages in chat, reply within threads, react to messages, or speak in voice or Stage channels."),
        11 => t!("Allow members to send messages in text channels and create posts in forum channels."),
        38 => t!("Allow members to send messages in threads and in posts on forum channels."),
        35 => t!("Allow members to create threads that everyone in a channel can view."),
        36 => t!("Allow members to create invite-only threads."),
        14 => t!("Allows links that members share to show embedded content in text channels."),
        15 => t!("Allows members to upload files or media in text channels."),
        6 => t!("Allows members to add new emoji reactions to a message. If this permission is disabled, members can still react using any existing reactions on a message."),
        18 => t!("Allows members to use emoji from other servers, if they’re a Discord Nitro member."),
        17 => t!("Allows members to use @everyone (everyone in the server) or @here (only online members in that channel). They can also @mention all roles, even if the role’s “Allow anyone to mention this role” permission is disabled."),
        13 => t!("Allows members to delete or remove embeds from messages by other members."),
        34 => t!("Allows members to rename, delete, close, and turn on slow mode for threads and posts. They can also view private threads."),
        16 => t!("Allows members to read previous messages sent in channels. If this permission is disabled, members only see messages sent when they are online. This does not fully apply to threads and forum posts."),
        12 => t!("Allows members to send text-to-speech messages by starting a message with /tts. These messages can be heard by anyone focused on the channel."),
        20 => t!("Allows members to join voice channels and hear others."),
        21 => t!("Allows members to talk in voice channels. If this permission is disabled, members are default muted until somebody with the “Mute Members” permission un-mutes them."),
        9 => t!("Allows members to share their video, screen share, or stream a game in this server."),
        25 => t!("Allows members to speak in voice channels by simply talking. If this permission is disabled, members are required to use Push-to-talk. Good for controlling background noise or noisy members."),
        8 => t!("Allows members to be more easily heard in voice channels. When activated, the volume of others without this permission will be automatically lowered."),
        22 => t!("Allows members to mute other members in voice channels for everyone."),
        23 => t!("Allows members to deafen other members in voice channels, which means they won’t be able to speak or hear others."),
        24 => t!("Allows members to disconnect or move other members between voice channels that the member with this permission has access to."),
        3 => t!("Members with this permission will have every permission and will also bypass all channel specific permissions or restrictions (for example, these members would get access to all private channels). This is a dangerous permission to grant."),
        _ => "",
    }
}

/// What an audit log entry did, in the interface's language: who did it is
/// `{user}`, whom it was done to `{target}`, so each language orders them.
pub fn audit_action(code: u64) -> &'static str {
    match code {
        1 => t!("{user} updated the server"),
        10 => t!("{user} created a channel"),
        11 => t!("{user} updated a channel"),
        12 => t!("{user} deleted a channel"),
        13..=15 => t!("{user} changed a channel's permissions"),
        20 => t!("{user} kicked {target}"),
        21 => t!("{user} pruned inactive members"),
        22 => t!("{user} banned {target}"),
        23 => t!("{user} revoked the ban for {target}"),
        24 => t!("{user} updated {target}"),
        25 => t!("{user} updated the roles of {target}"),
        26 => t!("{user} moved members in voice"),
        27 => t!("{user} disconnected members from voice"),
        28 => t!("{user} added the bot {target}"),
        30 => t!("{user} created a role"),
        31 => t!("{user} updated a role"),
        32 => t!("{user} deleted a role"),
        40 => t!("{user} created an invite"),
        41 => t!("{user} updated an invite"),
        42 => t!("{user} deleted an invite"),
        50..=52 => t!("{user} changed a webhook"),
        60..=62 => t!("{user} changed an emoji"),
        72 => t!("{user} deleted a message by {target}"),
        73 => t!("{user} bulk deleted messages"),
        74 => t!("{user} pinned a message by {target}"),
        75 => t!("{user} unpinned a message by {target}"),
        80..=82 => t!("{user} changed an integration"),
        110..=112 => t!("{user} changed a thread"),
        140..=145 => t!("{user} changed AutoMod"),
        _ => t!("{user} made a change"),
    }
}
