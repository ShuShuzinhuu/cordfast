//! The app's own types, parsed from Discord's JSON. Views never touch a
//! `serde_json::Value`; everything crosses the bridge as these.
//!
//! Parsing is lenient: the user gateway sends guilds either flat or with
//! their fields under `properties` (client state v2), and users either
//! inline or deduplicated into `users`. Both shapes are accepted.

use std::collections::HashMap;

use serde_json::Value;

pub type Id = u64;

/// Discord's epoch, 2015-01-01, in Unix milliseconds.
pub const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

const VIEW_CHANNEL: u64 = 1 << 10;
const ADMINISTRATOR: u64 = 1 << 3;

/// Reads a snowflake, which the API sends as a string.
pub fn sf(value: &Value) -> Option<Id> {
    match value {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

fn field_sf(value: &Value, key: &str) -> Option<Id> {
    value.get(key).and_then(sf)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

fn bits(value: &Value, key: &str) -> u64 {
    match value.get(key) {
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        _ => 0,
    }
}

/// A sticker's picture by its format: Lottie's JSON (Discord's own packs,
/// drawn here), the GIF, or the PNG (an APNG's first frame).
#[cfg(test)]
#[test]
fn tenor_videos_map_to_their_gif() {
    assert_eq!(tenor_gif("https://media.tenor.com/x8v1oNUOmg4AAAPo/rickroll-roll.mp4").as_deref(), Some("https://media.tenor.com/x8v1oNUOmg4AAAAd/rickroll-roll.gif"));
    assert_eq!(tenor_gif("https://example.com/a.mp4"), None);
}

/// "Play animated emoji", the account's setting, for the code without the app.
pub static ANIMATE_EMOJI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// A custom emoji's picture, moving if it is animated and the setting allows.
pub fn emoji_url(id: Id, animated: bool, size: u32) -> String {
    let moving = animated && ANIMATE_EMOJI.load(std::sync::atomic::Ordering::Relaxed);
    format!("https://cdn.discordapp.com/emojis/{id}.{}?size={size}", if moving { "gif" } else { "png" })
}

/// The GIF of a Tenor video: the same id with the format's suffix. Link
/// embeds (`gifv`) bring an mp4, which this client does not play yet.
pub fn tenor_gif(video: &str) -> Option<String> {
    let rest = video.strip_prefix("https://media.tenor.com/")?;
    let (id, name) = rest.split_once('/')?;
    let id = id.strip_suffix("AAAPo")?;
    Some(format!("https://media.tenor.com/{id}AAAAd/{}.gif", name.strip_suffix(".mp4")?))
}

pub fn sticker_url(id: Id, format: u64) -> String {
    match format {
        3 => format!("https://cdn.discordapp.com/stickers/{id}.json"),
        4 => format!("https://media.discordapp.net/stickers/{id}.gif?size=160"),
        _ => format!("https://media.discordapp.net/stickers/{id}.png?size=160"),
    }
}

/// Unix milliseconds at which a snowflake was made.
pub fn snowflake_ms(id: Id) -> u64 {
    (id >> 22) + DISCORD_EPOCH_MS
}

/// A snowflake for now, used as a message nonce.
pub fn now_snowflake() -> Id {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(DISCORD_EPOCH_MS);
    ms.saturating_sub(DISCORD_EPOCH_MS) << 22
}

#[derive(Clone, Debug, Default)]
pub struct User {
    pub id: Id,
    pub username: String,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
    pub discriminator: String,
    pub bot: bool,
    /// Public flags: 1 << 16 is a verified bot.
    pub flags: u64,
    /// The server tag shown beside the name: (guild, tag, badge hash).
    pub tag: Option<(Id, String, String)>,
    /// The avatar decoration's asset (Nitro, the shop).
    pub decoration: Option<String>,
    /// Nitro: 0 none, 1 Classic, 2 Nitro, 3 Basic.
    pub premium: u64,
    /// The nameplate (a shop collectible) behind the name in lists.
    pub nameplate: Option<Nameplate>,
}

/// A nameplate from Discord's shop: its picture is served by SKU, and the
/// palette names the colour of the gradient drawn under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Nameplate {
    pub sku: String,
    /// The asset's folder, such as "nameplates/nameplates/twilight/".
    pub asset: String,
    pub palette: String,
}

impl Nameplate {
    /// `collectibles.nameplate` of a user or a server member.
    pub fn parse(value: &Value) -> Option<Self> {
        let plate = value.pointer("/collectibles/nameplate")?;
        Some(Self { sku: string(plate, "sku_id")?, asset: string(plate, "asset").unwrap_or_default(), palette: string(plate, "palette").unwrap_or_default() })
    }

    /// The still picture: the asset's `static.png` on the CDN, or by SKU, as
    /// the bundle's `getCollectiblesItemAssetUrl`, when there is no asset.
    pub fn url(&self) -> String {
        match self.asset.as_str() {
            "" => format!("https://cdn.discordapp.com/media/v1/collectibles-shop/{}/static", self.sku),
            asset => format!("https://cdn.discordapp.com/assets/collectibles/{asset}static.png"),
        }
    }

    /// The palette's colours (dark theme, light theme), from the bundle's
    /// table; an unknown palette draws no gradient.
    pub fn colors(&self) -> Option<([u8; 3], [u8; 3])> {
        Some(match self.palette.as_str() {
            "crimson" => ([0x90, 0x00, 0x07], [0xE7, 0x04, 0x0F]),
            "berry" => ([0x89, 0x3A, 0x99], [0xB1, 0x1F, 0xCF]),
            "sky" => ([0x00, 0x80, 0xB7], [0x56, 0xCC, 0xFF]),
            "teal" => ([0x08, 0x64, 0x60], [0x7D, 0xEE, 0xD7]),
            "forest" => ([0x2D, 0x54, 0x01], [0x6A, 0xA6, 0x24]),
            "bubble_gum" => ([0xDC, 0x3E, 0x97], [0xF9, 0x57, 0xB3]),
            "violet" => ([0x73, 0x0B, 0xC8], [0x97, 0x2F, 0xED]),
            "cobalt" => ([0x01, 0x31, 0xC2], [0x42, 0x78, 0xFF]),
            "clover" => ([0x04, 0x7B, 0x20], [0x63, 0xCD, 0x5A]),
            "lemon" => ([0xF6, 0xCD, 0x12], [0xFE, 0xD4, 0x00]),
            "white" => ([0xFF; 3], [0xFF; 3]),
            "black" => ([0x00; 3], [0x00; 3]),
            _ => return None,
        })
    }
}

impl User {
    pub fn parse(value: &Value) -> Option<Self> {
        Some(Self {
            id: field_sf(value, "id")?,
            username: string(value, "username").unwrap_or_default(),
            global_name: string(value, "global_name").filter(|n| !n.is_empty()),
            avatar: string(value, "avatar"),
            discriminator: string(value, "discriminator").unwrap_or_default(),
            bot: value.get("bot").and_then(Value::as_bool).unwrap_or(false),
            flags: value.get("public_flags").and_then(Value::as_u64).unwrap_or(0),
            tag: value
                .get("primary_guild")
                .or_else(|| value.get("clan"))
                .filter(|g| g.get("identity_enabled").and_then(Value::as_bool) != Some(false))
                .and_then(|g| {
                    Some((
                        field_sf(g, "identity_guild_id")?,
                        string(g, "tag")?,
                        string(g, "badge").unwrap_or_default(),
                    ))
                }),
            decoration: value.pointer("/avatar_decoration_data/asset").and_then(Value::as_str).map(str::to_owned),
            premium: value.get("premium_type").and_then(Value::as_u64).unwrap_or(0),
            nameplate: Nameplate::parse(value),
        })
    }

    pub fn verified_bot(&self) -> bool {
        self.flags & (1 << 16) != 0
    }

    pub fn tag_badge_url(&self) -> Option<String> {
        let (guild, _, badge) = self.tag.as_ref()?;
        (!badge.is_empty()).then(|| format!("https://cdn.discordapp.com/clan-badges/{guild}/{badge}.png?size=32"))
    }

    pub fn decoration_url(&self) -> Option<String> {
        self.decoration
            .as_ref()
            .map(|asset| format!("https://cdn.discordapp.com/avatar-decoration-presets/{asset}.png?size=160&passthrough=false"))
    }

    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }

    /// The avatar at `size`, moving when the account has an animated one: for
    /// the profile, and a row under the pointer, where Discord plays it.
    pub fn avatar_moving(&self, size: u32) -> String {
        match &self.avatar {
            Some(hash) if hash.starts_with("a_") => format!("https://cdn.discordapp.com/avatars/{}/{hash}.gif?size={size}", self.id),
            _ => self.avatar_url().replace("size=64", &format!("size={size}")),
        }
    }

    /// A 64-pixel avatar, or Discord's default one for the account.
    pub fn avatar_url(&self) -> String {
        match &self.avatar {
            Some(hash) => format!(
                "https://cdn.discordapp.com/avatars/{}/{hash}.png?size=64",
                self.id
            ),
            None => {
                let index = match self.discriminator.parse::<u64>() {
                    Ok(d) if d != 0 => d % 5,
                    _ => (self.id >> 22) % 6,
                };
                format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Text,
    Dm,
    Voice,
    GroupDm,
    Category,
    Announcement,
    Thread,
    Stage,
    Forum,
    Other,
}

impl ChannelKind {
    fn from_code(code: u64) -> Self {
        match code {
            0 => Self::Text,
            1 => Self::Dm,
            2 => Self::Voice,
            3 => Self::GroupDm,
            4 => Self::Category,
            5 => Self::Announcement,
            10..=12 => Self::Thread,
            13 => Self::Stage,
            15 | 16 => Self::Forum,
            _ => Self::Other,
        }
    }

    /// Whether the channel has a message history we can open.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Dm | Self::GroupDm | Self::Announcement | Self::Thread
        )
    }
}

#[derive(Clone, Debug)]
pub struct Overwrite {
    pub id: Id,
    /// 0 for a role, 1 for a member.
    pub kind: u64,
    pub allow: u64,
    pub deny: u64,
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub id: Id,
    pub guild_id: Option<Id>,
    pub kind: ChannelKind,
    pub name: String,
    pub position: i64,
    pub parent_id: Option<Id>,
    pub recipients: Vec<User>,
    pub last_message_id: Option<Id>,
    pub overwrites: Vec<Overwrite>,
    /// For a thread: whether we joined it, which shows it in the list.
    pub joined: bool,
    pub archived: bool,
    pub topic: Option<String>,
    /// A voice channel's limit of people (0 for none).
    pub user_limit: u64,
    /// Age-restricted.
    pub nsfw: bool,
    /// Slow mode: seconds between one person's messages.
    pub slowmode: u32,
    /// A voice channel's status, the line under its name ("Ranked games").
    pub status: Option<String>,
    /// A group's owner, or who started a thread.
    pub owner: Option<Id>,
    pub locked: bool,
    /// Someone who is no friend wrote: waits for Accept or Ignore.
    pub message_request: bool,
    /// A message request Discord's filter took for spam: under Message
    /// Requests' Spam tab instead.
    pub spam: bool,
}

impl Channel {
    pub fn parse(value: &Value, guild_id: Option<Id>, users: &HashMap<Id, User>) -> Option<Self> {
        let mut recipients: Vec<User> = value
            .get("recipients")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(User::parse).collect())
            .unwrap_or_default();
        if recipients.is_empty()
            && let Some(ids) = value.get("recipient_ids").and_then(Value::as_array)
        {
            recipients = ids
                .iter()
                .filter_map(sf)
                .map(|id| {
                    users.get(&id).cloned().unwrap_or(User {
                        id,
                        username: "?".into(),
                        ..User::default()
                    })
                })
                .collect();
        }
        let overwrites = value
            .get("permission_overwrites")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|o| {
                        Some(Overwrite {
                            id: field_sf(o, "id")?,
                            kind: match o.get("type") {
                                Some(Value::String(s)) => u64::from(s == "member" || s == "1"),
                                Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
                                _ => 0,
                            },
                            allow: bits(o, "allow"),
                            deny: bits(o, "deny"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            id: field_sf(value, "id")?,
            guild_id: guild_id.or_else(|| field_sf(value, "guild_id")),
            kind: ChannelKind::from_code(value.get("type").and_then(Value::as_u64).unwrap_or(0)),
            name: string(value, "name").unwrap_or_default(),
            position: value.get("position").and_then(Value::as_i64).unwrap_or(0),
            parent_id: field_sf(value, "parent_id"),
            recipients,
            last_message_id: field_sf(value, "last_message_id"),
            overwrites,
            topic: string(value, "topic").filter(|t| !t.trim().is_empty()),
            user_limit: value.get("user_limit").and_then(Value::as_u64).unwrap_or(0),
            nsfw: value.get("nsfw").and_then(Value::as_bool).unwrap_or(false),
            slowmode: value.get("rate_limit_per_user").and_then(Value::as_u64).unwrap_or(0) as u32,
            status: string(value, "status").filter(|s| !s.trim().is_empty()),
            joined: value.get("member").is_some_and(|m| !m.is_null()),
            owner: field_sf(value, "owner_id"),
            locked: value.pointer("/thread_metadata/locked").and_then(Value::as_bool).unwrap_or(false),
            message_request: value.get("is_message_request").and_then(Value::as_bool).unwrap_or(false),
            spam: value.get("is_spam").and_then(Value::as_bool).unwrap_or(false),
            archived: value
                .pointer("/thread_metadata/archived")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// The channel's name, or its people's names for a DM.
    pub fn title(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        let names: Vec<&str> = self.recipients.iter().map(User::display_name).collect();
        if names.is_empty() {
            t!("Conversation").into()
        } else {
            names.join(", ")
        }
    }
}

#[derive(Clone, Debug)]
pub struct Role {
    pub id: Id,
    pub name: String,
    pub permissions: u64,
    pub color: u32,
    /// The second colour of a gradient role, which Discord draws as a
    /// gradient from `color`.
    pub color2: Option<u32>,
    pub position: i64,
    /// Shown apart in the member list.
    pub hoist: bool,
    /// Anyone may @mention it.
    pub mentionable: bool,
    /// Kept by a bot or an integration, not editable by hand.
    pub managed: bool,
}

#[derive(Clone, Debug)]
pub struct Guild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub owner_id: Option<Id>,
    pub channels: Vec<Channel>,
    pub roles: Vec<Role>,
    pub my_roles: Vec<Id>,
    /// Who is in which voice channel: (user, channel).
    pub voice_states: Vec<(Id, Id)>,
    /// Muted and deafened, by user, for those in voice.
    pub voice_flags: Vec<(Id, bool, bool)>,
    pub emojis: Vec<Emoji>,
    /// (id, name, format): 1 PNG, 2 APNG, 3 Lottie, 4 GIF.
    pub stickers: Vec<(Id, String, u64)>,
    /// What members hear about by default: 0 every message, 1 @mentions.
    pub default_notifications: u8,
    /// Discord's feature flags ("COMMUNITY", "NEWS"…).
    pub features: Vec<String>,
    /// Scheduled events READY listed: the sidebar's "Events" row shows
    /// while there are any.
    pub events: usize,
    /// When we joined, as a snowflake: a forum never read counts its posts
    /// as new from then on.
    pub joined: Option<Id>,
    /// The banner over the channel list.
    pub banner: Option<String>,
    /// Boosts applied, and whether the server shows its boost goal bar.
    pub boosts: u64,
    pub boost_bar: bool,
}

impl Guild {
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }

    /// The banner as the channel list shows it.
    pub fn banner_url(&self) -> Option<String> {
        self.banner.as_ref().map(|hash| format!("https://cdn.discordapp.com/banners/{}/{hash}.png?size=480", self.id))
    }

    /// The boost goal's end (the bundle's `tg`): Level 3's 14 boosts, none
    /// with PREMIUM_TIER_3_OVERRIDE, plus the price of every perk bought
    /// apart (server tags 3, enhanced role colours 3, the badge packs 3, 5,
    /// 3 and 2).
    pub fn boost_goal(&self) -> u64 {
        (if self.has("PREMIUM_TIER_3_OVERRIDE") { 0 } else { 14 }) + 3 + 3 + 3 + 5 + 3 + 2
    }
}

impl Role {
    pub fn parse(r: &Value) -> Option<Self> {
        Some(Role {
            id: field_sf(r, "id")?,
            name: string(r, "name").unwrap_or_default(),
            permissions: bits(r, "permissions"),
            color: r
                .pointer("/colors/primary_color")
                .or_else(|| r.get("color"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            color2: r
                .pointer("/colors/secondary_color")
                .and_then(Value::as_u64)
                .map(|c| c as u32),
            position: r.get("position").and_then(Value::as_i64).unwrap_or(0),
            hoist: r.get("hoist").and_then(Value::as_bool).unwrap_or(false),
            mentionable: r.get("mentionable").and_then(Value::as_bool).unwrap_or(false),
            managed: r.get("managed").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

impl Guild {
    /// Our permissions across the guild (no channel overwrites).
    pub fn my_permissions(&self, me: Id) -> u64 {
        if self.owner_id == Some(me) {
            return u64::MAX;
        }
        let mut perms = self.roles.iter().find(|r| r.id == self.id).map_or(0, |r| r.permissions);
        for role in &self.my_roles {
            perms |= self.roles.iter().find(|r| r.id == *role).map_or(0, |r| r.permissions);
        }
        if perms & ADMINISTRATOR != 0 { u64::MAX } else { perms }
    }

    /// `my_member` is our own member object when the gateway sent it apart
    /// (READY's `merged_members`).
    pub fn parse(value: &Value, me: Id, my_member: Option<&Value>) -> Option<Self> {
        if value.get("unavailable").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let id = field_sf(value, "id")?;
        let props = value.get("properties").unwrap_or(value);
        let empty = HashMap::new();
        let mut channels: Vec<Channel> = value
            .get("channels")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|c| Channel::parse(c, Some(id), &empty))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(threads) = value.get("threads").and_then(Value::as_array) {
            channels.extend(threads.iter().filter_map(|c| Channel::parse(c, Some(id), &empty)));
        }
        let roles = value
            .get("roles")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Role::parse)
                    .collect()
            })
            .unwrap_or_default();
        let member_roles = |m: &Value| -> Vec<Id> {
            m.get("roles")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(sf).collect())
                .unwrap_or_default()
        };
        let my_roles = my_member.map(member_roles).or_else(|| {
            value
                .get("members")
                .and_then(Value::as_array)?
                .iter()
                .find(|m| {
                    m.get("user").and_then(|u| field_sf(u, "id")) == Some(me)
                        || field_sf(m, "user_id") == Some(me)
                })
                .map(member_roles)
        });
        Some(Self {
            id,
            name: string(props, "name").unwrap_or_default(),
            icon: string(props, "icon"),
            owner_id: field_sf(props, "owner_id"),
            default_notifications: props.get("default_message_notifications").and_then(Value::as_u64).unwrap_or(1).min(1) as u8,
            features: props.get("features").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(),
            events: value.get("guild_scheduled_events").and_then(Value::as_array).map_or(0, Vec::len),
            banner: string(props, "banner"),
            // READY keeps the count beside `properties`, not in it.
            boosts: value.get("premium_subscription_count").or_else(|| props.get("premium_subscription_count")).and_then(Value::as_u64).unwrap_or(0),
            boost_bar: props.get("premium_progress_bar_enabled").or_else(|| value.get("premium_progress_bar_enabled")).and_then(Value::as_bool).unwrap_or(false),
            joined: string(value, "joined_at")
                .and_then(|t| t.parse::<jiff::Timestamp>().ok())
                .map(|t| (t.as_millisecond().max(0) as u64).saturating_sub(DISCORD_EPOCH_MS) << 22),
            channels,
            roles,
            my_roles: my_roles.unwrap_or_default(),
            stickers: value
                .get("stickers")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|s| {
                            Some((
                                field_sf(s, "id")?,
                                string(s, "name").unwrap_or_default(),
                                s.get("format_type").and_then(Value::as_u64).unwrap_or(1),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            emojis: value
                .get("emojis")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(Emoji::parse).collect())
                .unwrap_or_default(),
            voice_flags: value
                .get("voice_states")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(|v| Some((field_sf(v, "user_id")?, voice_muted(v), voice_deaf(v)))).collect())
                .unwrap_or_default(),
            voice_states: value
                .get("voice_states")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|v| Some((field_sf(v, "user_id")?, field_sf(v, "channel_id")?)))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    pub fn icon_url(&self) -> Option<String> {
        self.icon.as_ref().map(|hash| {
            format!(
                "https://cdn.discordapp.com/icons/{}/{hash}.png?size=96",
                self.id
            )
        })
    }

    /// Whether we may see a channel, from the roles and overwrites the
    /// gateway sent. Discord's own order: base permissions, then the
    /// @everyone overwrite, the role overwrites together, and our own.
    pub fn can_view(&self, me: Id, channel: &Channel) -> bool {
        self.channel_permissions(me, channel) & VIEW_CHANNEL != 0
    }

    /// Our permissions in a channel: the roles', then the channel's
    /// overwrites for @everyone, our roles and us, as Discord computes
    /// them. A thread has its parent's.
    pub fn channel_permissions(&self, me: Id, channel: &Channel) -> u64 {
        if self.owner_id == Some(me) {
            return u64::MAX;
        }
        if channel.kind == ChannelKind::Thread
            && let Some(parent) = channel
                .parent_id
                .and_then(|p| self.channels.iter().find(|c| c.id == p))
        {
            return self.channel_permissions(me, parent);
        }
        let role_bits = |id: Id| {
            self.roles
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.permissions)
        };
        let mut base = role_bits(self.id);
        for role in &self.my_roles {
            base |= role_bits(*role);
        }
        if base & ADMINISTRATOR != 0 {
            return u64::MAX;
        }
        let mut perms = base;
        if let Some(o) = channel.overwrites.iter().find(|o| o.id == self.id) {
            perms &= !o.deny;
            perms |= o.allow;
        }
        let (mut allow, mut deny) = (0, 0);
        for o in &channel.overwrites {
            if o.kind == 0 && self.my_roles.contains(&o.id) {
                allow |= o.allow;
                deny |= o.deny;
            }
        }
        perms &= !deny;
        perms |= allow;
        if let Some(o) = channel
            .overwrites
            .iter()
            .find(|o| o.kind == 1 && o.id == me)
        {
            perms &= !o.deny;
            perms |= o.allow;
        }
        perms
    }

    /// The colour of the highest coloured role among `roles`.
    pub fn role_color(&self, roles: &[Id]) -> u32 {
        self.role_colors(roles).0
    }

    /// The highest coloured role's colour, and its gradient's second colour.
    pub fn role_colors(&self, roles: &[Id]) -> (u32, Option<u32>) {
        self.roles
            .iter()
            .filter(|r| r.color != 0 && roles.contains(&r.id))
            .max_by_key(|r| r.position)
            .map_or((0, None), |r| (r.color, r.color2))
    }

    /// `roles` as role objects, highest first.
    pub fn roles_of(&self, roles: &[Id]) -> Vec<&Role> {
        let mut list: Vec<&Role> = self.roles.iter().filter(|r| roles.contains(&r.id)).collect();
        list.sort_by_key(|r| std::cmp::Reverse(r.position));
        list
    }
}

/// Whether a voice state is muted, by themselves or the server.
pub fn voice_muted(v: &Value) -> bool {
    ["self_mute", "mute"].iter().any(|k| v.get(*k).and_then(Value::as_bool) == Some(true))
}

pub fn voice_deaf(v: &Value) -> bool {
    ["self_deaf", "deaf"].iter().any(|k| v.get(*k).and_then(Value::as_bool) == Some(true))
}

/// A guild member's own look: nickname and roles.
#[derive(Clone, Debug, Default)]
pub struct Member {
    pub nick: Option<String>,
    /// This server's own nameplate, over the account's.
    pub nameplate: Option<Nameplate>,
    pub roles: Vec<Id>,
    /// Has not finished the server's verification form yet.
    pub pending: bool,
}

impl Member {
    pub fn parse(value: &Value) -> Self {
        Self {
            pending: value.get("pending").and_then(Value::as_bool).unwrap_or(false),
            nameplate: Nameplate::parse(value),
            nick: string(value, "nick").filter(|n| !n.is_empty()),
            roles: value
                .get("roles")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(sf).collect())
                .unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Attachment {
    pub filename: String,
    pub url: String,
    pub proxy_url: String,
    pub content_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        self.width.is_some()
            && self
                .content_type
                .as_deref()
                .is_none_or(|t| t.starts_with("image/"))
    }
}

#[derive(Clone, Debug)]
pub struct Embed {
    pub title: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub image: Option<(String, u32, u32)>,
    /// The bar on the left, as the embed sets it.
    pub color: Option<u32>,
    /// The site the link is from ("YouTube"), over the author.
    pub provider: Option<String>,
    pub author: Option<String>,
    pub author_url: Option<String>,
    pub author_icon: Option<String>,
    pub footer: Option<String>,
    pub footer_icon: Option<String>,
    /// When the embed says it happened (Unix milliseconds), in the footer.
    pub timestamp: Option<u64>,
    /// (name, value, inline)
    pub fields: Vec<(String, String, bool)>,
    /// A small picture at the top right (a thumbnail beside an image).
    pub thumbnail: Option<(String, u32, u32)>,
}

/// A poll: its question, answers with their votes, and whether it ended.
#[derive(Clone, Debug)]
pub struct Poll {
    pub question: String,
    /// (answer id, text, votes, voted by us)
    pub answers: Vec<(u64, String, u64, bool)>,
    pub multiselect: bool,
    pub finalized: bool,
    pub expiry: Option<String>,
}

impl Poll {
    fn parse(value: &Value) -> Option<Self> {
        let counts: Vec<(u64, u64, bool)> = value
            .pointer("/results/answer_counts")
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .filter_map(|c| {
                        Some((
                            c.get("id")?.as_u64()?,
                            c.get("count").and_then(Value::as_u64).unwrap_or(0),
                            c.get("me_voted").and_then(Value::as_bool).unwrap_or(false),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            question: value.pointer("/question/text")?.as_str()?.to_owned(),
            answers: value
                .get("answers")?
                .as_array()?
                .iter()
                .filter_map(|a| {
                    let id = a.get("answer_id")?.as_u64()?;
                    let media = a.get("poll_media")?;
                    let emoji = media.pointer("/emoji/name").and_then(Value::as_str).map(|e| format!("{e} ")).unwrap_or_default();
                    let text = format!("{emoji}{}", media.get("text").and_then(Value::as_str).unwrap_or_default());
                    let (votes, mine) = counts.iter().find(|c| c.0 == id).map_or((0, false), |c| (c.1, c.2));
                    Some((id, text, votes, mine))
                })
                .collect(),
            multiselect: value.get("allow_multiselect").and_then(Value::as_bool).unwrap_or(false),
            finalized: value.pointer("/results/is_finalized").and_then(Value::as_bool).unwrap_or(false),
            expiry: string(value, "expiry"),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub guild_id: Option<Id>,
    pub author: User,
    pub content: String,
    pub edited: bool,
    pub attachments: Vec<Attachment>,
    pub embeds: Vec<Embed>,
    pub mentions: Vec<User>,
    pub mention_roles: Vec<Id>,
    pub mention_everyone: bool,
    /// The quoted message's author and text, for replies.
    pub reply: Option<(String, String)>,
    /// The message this one answers, to jump to it.
    pub reply_id: Option<Id>,
    /// Who wrote the message replied to.
    pub reply_author: Option<User>,
    pub nonce: Option<String>,
    /// Sent from here and not yet confirmed by the gateway.
    pub pending: bool,
    pub kind: u64,
    pub reactions: Vec<Reaction>,
    /// The author's member object, which gateway messages carry.
    pub member: Option<Member>,
    pub stickers: Vec<(Id, String, u64)>,
    /// MessageLogger: deleted here, kept for the record.
    pub deleted: bool,
    /// MessageLogger: earlier texts, oldest first.
    pub edits: Vec<String>,
    pub pinned: bool,
    /// 1 crossposted, 4 embeds suppressed, 4096 silent, 8192 a voice message.
    pub flags: u64,
    pub poll: Option<Poll>,
    pub forwarded: bool,
    /// Buttons, menus, and the whole layout of a Components V2 message.
    pub components: Vec<super::components::Component>,
    /// For an app's answer to a slash command: who ran it and its name, as
    /// Discord's "Ana used /play" over the message.
    pub interaction: Option<(User, String)>,
    /// What an interaction on those components needs.
    pub origin: Option<super::components::Origin>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emoji {
    pub id: Option<Id>,
    pub name: String,
    pub animated: bool,
}

impl Emoji {
    pub fn parse(value: &Value) -> Option<Self> {
        Some(Self {
            id: field_sf(value, "id"),
            name: string(value, "name")?,
            animated: value.get("animated").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// The form the reactions endpoint takes: `name:id` or the character.
    pub fn api_name(&self) -> String {
        match self.id {
            Some(id) => format!("{}:{id}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn url(&self) -> Option<String> {
        self.id.map(|id| emoji_url(id, self.animated, 48))
    }
}

#[derive(Clone, Debug)]
pub struct Reaction {
    pub emoji: Emoji,
    pub count: u32,
    pub me: bool,
}

impl Message {
    /// A message; a forwarded one shows what it forwards, which arrives in
    /// `message_snapshots` while its own text is empty.
    pub fn parse(value: &Value) -> Option<Self> {
        if let Some(snapshot) = value.pointer("/message_snapshots/0/message") {
            let mut merged = value.clone();
            for key in ["content", "attachments", "embeds", "sticker_items"] {
                if let Some(v) = snapshot.get(key) {
                    merged[key] = v.clone();
                }
            }
            let mut message = Self::parse_one(&merged)?;
            message.forwarded = true;
            return Some(message);
        }
        let mut message = Self::parse_one(value)?;
        if !message.components.is_empty() {
            message.origin = super::components::Origin::parse(value, message.channel_id, message.guild_id, message.id);
        }
        Some(message)
    }

    fn parse_one(value: &Value) -> Option<Self> {
        let attachments = value
            .get("attachments")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|a| {
                        Some(Attachment {
                            filename: string(a, "filename").unwrap_or_default(),
                            url: string(a, "url")?,
                            proxy_url: string(a, "proxy_url")
                                .or_else(|| string(a, "url"))?,
                            content_type: string(a, "content_type"),
                            width: a.get("width").and_then(Value::as_u64).map(|w| w as u32),
                            height: a.get("height").and_then(Value::as_u64).map(|h| h as u32),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let embeds = value
            .get("embeds")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|e| {
                        let media = |p: &Value| -> Option<(String, u32, u32)> {
                            Some((
                                string(p, "proxy_url").or_else(|| string(p, "url"))?,
                                p.get("width").and_then(Value::as_u64)? as u32,
                                p.get("height").and_then(Value::as_u64)? as u32,
                            ))
                        };
                        // A link preview (no fields, no image) shows its
                        // thumbnail large; a rich embed keeps it small.
                        let rich = e.get("type").and_then(Value::as_str) == Some("rich");
                        let image = e.get("image").and_then(media);
                        let thumbnail = e.get("thumbnail").and_then(media);
                        // A Tenor link moves in the message, as its GIF.
                        let moving = e.pointer("/video/url").and_then(Value::as_str).filter(|_| e.get("type").and_then(Value::as_str) == Some("gifv")).and_then(tenor_gif).map(|url| {
                            let side = |key: &str| e.pointer(&format!("/video/{key}")).and_then(Value::as_u64).unwrap_or(0) as u32;
                            (url, side("width"), side("height"))
                        });
                        let thumbnail = if moving.is_some() { None } else { thumbnail };
                        let image = moving.or(image);
                        let (image, thumbnail) = match (image, thumbnail) {
                            (None, Some(t)) if !rich => (Some(t), None),
                            other => other,
                        };
                        Embed {
                            title: string(e, "title"),
                            url: string(e, "url"),
                            description: string(e, "description"),
                            image,
                            thumbnail,
                            color: e.get("color").and_then(Value::as_u64).map(|c| c as u32),
                            provider: e.pointer("/provider/name").and_then(Value::as_str).map(str::to_owned),
                            author: e.pointer("/author/name").and_then(Value::as_str).map(str::to_owned),
                            author_url: e.pointer("/author/url").and_then(Value::as_str).map(str::to_owned),
                            author_icon: e.get("author").and_then(|a| string(a, "proxy_icon_url").or_else(|| string(a, "icon_url"))),
                            footer: e.pointer("/footer/text").and_then(Value::as_str).map(str::to_owned),
                            footer_icon: e.get("footer").and_then(|f| string(f, "proxy_icon_url").or_else(|| string(f, "icon_url"))),
                            timestamp: e
                                .get("timestamp")
                                .and_then(Value::as_str)
                                .and_then(|t| t.parse::<jiff::Timestamp>().ok())
                                .map(|t| t.as_millisecond().max(0) as u64),
                            fields: e
                                .get("fields")
                                .and_then(Value::as_array)
                                .map(|l| {
                                    l.iter()
                                        .filter_map(|f| {
                                            Some((
                                                string(f, "name")?,
                                                string(f, "value").unwrap_or_default(),
                                                f.get("inline").and_then(Value::as_bool).unwrap_or(false),
                                            ))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let users = |key: &str| -> Vec<User> {
            value
                .get(key)
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(User::parse).collect())
                .unwrap_or_default()
        };
        let reply = value
            .get("referenced_message")
            .filter(|r| !r.is_null())
            .map(|r| {
                let author = r
                    .get("author")
                    .and_then(User::parse)
                    .map(|u| u.display_name().to_owned())
                    .unwrap_or_default();
                (author, string(r, "content").unwrap_or_default())
            });
        Some(Self {
            id: field_sf(value, "id")?,
            channel_id: field_sf(value, "channel_id")?,
            guild_id: field_sf(value, "guild_id"),
            author: value.get("author").and_then(User::parse)?,
            content: string(value, "content").unwrap_or_default(),
            edited: value.get("edited_timestamp").is_some_and(|t| !t.is_null()),
            attachments,
            embeds,
            mentions: users("mentions"),
            mention_roles: value
                .get("mention_roles")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(sf).collect())
                .unwrap_or_default(),
            mention_everyone: value
                .get("mention_everyone")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            reply_author: value.get("referenced_message").and_then(|r| r.get("author")).and_then(User::parse),
            reply_id: value.pointer("/message_reference/message_id").and_then(sf).filter(|_| reply.is_some()),
            reply,
            nonce: match value.get("nonce") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                _ => None,
            },
            pending: false,
            kind: value.get("type").and_then(Value::as_u64).unwrap_or(0),
            reactions: value
                .get("reactions")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|r| {
                            Some(Reaction {
                                emoji: Emoji::parse(r.get("emoji")?)?,
                                count: r.get("count").and_then(Value::as_u64).unwrap_or(1) as u32,
                                me: r.get("me").and_then(Value::as_bool).unwrap_or(false),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            member: value.get("member").filter(|m| !m.is_null()).map(Member::parse),
            stickers: value
                .get("sticker_items")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|s| {
                            Some((
                                field_sf(s, "id")?,
                                string(s, "name").unwrap_or_default(),
                                s.get("format_type").and_then(Value::as_u64).unwrap_or(1),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            deleted: false,
            edits: Vec::new(),
            pinned: value.get("pinned").and_then(Value::as_bool).unwrap_or(false),
            flags: value.get("flags").and_then(Value::as_u64).unwrap_or(0),
            poll: value.get("poll").and_then(Poll::parse),
            forwarded: false,
            components: super::components::Component::parse_all(value),
            origin: None,
            interaction: value
                .get("interaction_metadata")
                .filter(|m| m.get("type").and_then(Value::as_u64) == Some(2))
                .or_else(|| value.get("interaction"))
                .and_then(|m| {
                    let user = User::parse(m.get("user")?)?;
                    let name = m.get("name").or_else(|| value.pointer("/interaction/name"))?.as_str()?.to_owned();
                    Some((user, name))
                }),
        })
    }

    pub fn timestamp_ms(&self) -> u64 {
        snowflake_ms(self.id)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReadState {
    pub channel: Id,
    pub last_message_id: Id,
    pub mention_count: u32,
}

/// Everything READY gives the interface.
#[derive(Debug)]
pub struct Ready {
    pub me: User,
    pub guilds: Vec<Guild>,
    pub private_channels: Vec<Channel>,
    pub read_states: Vec<ReadState>,
    pub folders: Option<Folders>,
    pub mutes: Mutes,
    pub relationships: Vec<Relationship>,
    /// online, idle, dnd or invisible, from the settings protobuf.
    pub status: String,
    pub custom_status: Option<String>,
    /// The gateway session, which interactions name.
    pub session_id: String,
    /// The account's PreloadedUserSettings, whole.
    pub settings_proto: Vec<u8>,
    /// The email is verified (Discord's "Please check your email" bar).
    pub verified: bool,
    pub email: Option<String>,
}

/// `PreloadedUserSettings.status.custom_status.text` (11 → 2 → 1), when
/// it has not expired.
pub fn proto_custom_status(settings: &[u8]) -> Option<String> {
    let status = proto::fields(settings)
        .into_iter()
        .find_map(|(n, v)| (n == 11).then_some(v))?
        .bytes()?;
    let custom = proto::fields(status)
        .into_iter()
        .find_map(|(n, v)| (n == 2).then_some(v))?
        .bytes()?;
    let fields = proto::fields(custom);
    let expires = fields.iter().find_map(|(n, v)| match (n, v) {
        (4, proto::Field::Fixed64(x)) => Some(*x),
        _ => None,
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if expires.is_some_and(|e| e != 0 && e < now) {
        return None;
    }
    let text = fields.into_iter().find_map(|(n, v)| (n == 1).then_some(v))?.bytes()?;
    Some(String::from_utf8_lossy(text).into_owned()).filter(|t| !t.is_empty())
}

/// `PreloadedUserSettings.status.status` (fields 11 → 1, a StringValue).
pub fn proto_status(settings: &[u8]) -> Option<String> {
    let status = proto::fields(settings)
        .into_iter()
        .find_map(|(n, v)| (n == 11).then_some(v))?
        .bytes()?;
    let wrapper = proto::fields(status)
        .into_iter()
        .find_map(|(n, v)| (n == 1).then_some(v))?
        .bytes()?;
    let value = proto::fields(wrapper)
        .into_iter()
        .find_map(|(n, v)| (n == 1).then_some(v))?
        .bytes()?;
    Some(String::from_utf8_lossy(value).into_owned()).filter(|s| !s.is_empty())
}

/// The server list's folders and order, from the settings protobuf.
#[derive(Clone, Debug, Default)]
pub struct Folders {
    pub folders: Vec<Folder>,
    pub positions: Vec<Id>,
}

#[derive(Clone, Debug, Default)]
pub struct Folder {
    /// `None` for a lone guild the list places outside any folder.
    pub id: Option<i64>,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<Id>,
}

impl Folders {
    /// Decodes `PreloadedUserSettings.guild_folders` (field 14). Field
    /// numbers come from the web client's protobuf descriptors:
    /// GuildFolders { 1: repeated GuildFolder folders, 2: repeated fixed64
    /// guild_positions }, GuildFolder { 1: repeated fixed64 guild_ids,
    /// 2: Int64Value id, 3: StringValue name, 4: UInt64Value color }.
    pub fn from_proto(settings: &[u8]) -> Option<Self> {
        let folders_bytes = proto::fields(settings)
            .into_iter()
            .find_map(|(n, v)| (n == 14).then_some(v))?
            .bytes()?;
        let mut out = Folders::default();
        for (number, value) in proto::fields(folders_bytes) {
            match number {
                1 => {
                    let Some(bytes) = value.bytes() else { continue };
                    let mut folder = Folder::default();
                    for (n, v) in proto::fields(bytes) {
                        match n {
                            1 => folder.guild_ids.extend(v.fixed64s()),
                            2 => folder.id = v.wrapped_varint().map(|x| x as i64),
                            3 => {
                                folder.name = v
                                    .bytes()
                                    .and_then(|b| {
                                        proto::fields(b).into_iter().find(|(n, _)| *n == 1)
                                    })
                                    .and_then(|(_, v)| v.bytes())
                                    .map(|b| String::from_utf8_lossy(b).into_owned())
                                    .filter(|n| !n.is_empty());
                            }
                            4 => folder.color = v.wrapped_varint().map(|c| c as u32),
                            _ => {}
                        }
                    }
                    out.folders.push(folder);
                }
                2 => out.positions.extend(value.fixed64s()),
                _ => {}
            }
        }
        Some(out)
    }

    /// The JSON shape older sessions send in `user_settings`.
    pub fn from_json(value: &Value) -> Option<Self> {
        let list = value.get("guild_folders")?.as_array()?;
        Some(Folders {
            folders: list
                .iter()
                .map(|f| Folder {
                    id: f.get("id").and_then(Value::as_i64),
                    name: string(f, "name").filter(|n| !n.is_empty()),
                    color: f.get("color").and_then(Value::as_u64).map(|c| c as u32),
                    guild_ids: f
                        .get("guild_ids")
                        .and_then(Value::as_array)
                        .map(|ids| ids.iter().filter_map(sf).collect())
                        .unwrap_or_default(),
                })
                .collect(),
            positions: Vec::new(),
        })
    }
}

/// Muted guilds and channels, and how much each notifies, from
/// `user_guild_settings`.
#[derive(Clone, Debug, Default)]
pub struct Mutes {
    pub guilds: std::collections::HashSet<Id>,
    pub channels: std::collections::HashSet<Id>,
    /// When a timed mute ends (Unix ms), by guild or channel.
    pub ends: HashMap<Id, u64>,
    /// Discord's "Notification Settings", by guild and by channel:
    /// 0 every message, 1 only @mentions, 2 nothing. A channel that follows
    /// its category or server is absent.
    pub levels: HashMap<Id, u8>,
    /// DMs pinned to the top of the list: the `FAVORITED` flag (2048) of
    /// their override in the settings without a server.
    pub favorites: std::collections::HashSet<Id>,
    /// Those overrides' flags, kept so a change sets one bit only.
    pub dm_flags: HashMap<Id, u64>,
    /// Servers whose list shows only the channels picked in "Channels &
    /// Roles" (`OPT_IN_CHANNELS_ON`, 16384, in the server's flags).
    pub opt_in: std::collections::HashSet<Id>,
    /// The channels and categories picked there (`OPT_IN_ENABLED`, 4096).
    pub opted: std::collections::HashSet<Id>,
}

/// `ChannelNotificationSettingsFlags.FAVORITED`.
pub const FAVORITED: u64 = 2048;

/// A mute's end, when it has one, in Unix ms.
fn mute_end(value: &Value) -> Option<u64> {
    let end = value.pointer("/mute_config/end_time").and_then(Value::as_str)?;
    end.parse::<jiff::Timestamp>().ok().map(|t| t.as_millisecond().max(0) as u64)
}

fn level(value: &Value) -> Option<u8> {
    value.get("message_notifications").and_then(Value::as_u64).filter(|l| *l <= 2).map(|l| l as u8)
}

fn muted(value: &Value) -> bool {
    if value.get("muted").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    // A timed mute that has run out no longer counts.
    match value.pointer("/mute_config/end_time").and_then(Value::as_str) {
        Some(end) => end
            .parse::<jiff::Timestamp>()
            .is_ok_and(|end| end > jiff::Timestamp::now()),
        None => true,
    }
}

impl Mutes {
    pub fn add(&mut self, entry: &Value) {
        if let Some(guild) = field_sf(entry, "guild_id") {
            if entry.get("flags").and_then(Value::as_u64).unwrap_or(0) & 16384 != 0 {
                self.opt_in.insert(guild);
            } else {
                self.opt_in.remove(&guild);
            }
            self.set(guild, muted(entry).then(|| mute_end(entry)), true);
            match level(entry) {
                Some(level) => self.levels.insert(guild, level),
                None => self.levels.remove(&guild),
            };
        }
        for o in entry
            .get("channel_overrides")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(channel) = field_sf(o, "channel_id") {
                if o.get("flags").and_then(Value::as_u64).unwrap_or(0) & 4096 != 0 {
                    self.opted.insert(channel);
                } else {
                    self.opted.remove(&channel);
                }
                if field_sf(entry, "guild_id").is_none() {
                    let flags = o.get("flags").and_then(Value::as_u64).unwrap_or(0);
                    self.dm_flags.insert(channel, flags);
                    if flags & FAVORITED != 0 {
                        self.favorites.insert(channel);
                    } else {
                        self.favorites.remove(&channel);
                    }
                }
                self.set(channel, muted(o).then(|| mute_end(o)), false);
                match level(o) {
                    Some(level) => self.levels.insert(channel, level),
                    None => self.levels.remove(&channel),
                };
            }
        }
    }

    /// Mutes (until `end`, or for good with `Some(None)`) or unmutes (`None`)
    /// a guild or a channel.
    pub fn set(&mut self, id: Id, mute: Option<Option<u64>>, guild: bool) {
        let set = if guild { &mut self.guilds } else { &mut self.channels };
        match mute {
            Some(end) => {
                set.insert(id);
                match end {
                    Some(end) => self.ends.insert(id, end),
                    None => self.ends.remove(&id),
                };
            }
            None => {
                set.remove(&id);
                self.ends.remove(&id);
            }
        }
    }

    /// Lifts the timed mutes that have run out; true when any did.
    pub fn expire(&mut self, now: u64) -> bool {
        let ended: Vec<Id> = self.ends.iter().filter(|(_, end)| **end <= now).map(|(id, _)| *id).collect();
        for id in &ended {
            self.ends.remove(id);
            self.guilds.remove(id);
            self.channels.remove(id);
        }
        !ended.is_empty()
    }

    pub fn parse(value: Option<&Value>) -> Self {
        let mut mutes = Self::default();
        let entries = match value {
            Some(Value::Array(list)) => Some(list),
            Some(object) => object.get("entries").and_then(Value::as_array),
            None => None,
        };
        for entry in entries.into_iter().flatten() {
            mutes.add(entry);
        }
        mutes
    }
}

#[derive(Clone, Debug)]
pub struct Relationship {
    pub user: User,
    /// 1 friend, 2 blocked, 3 incoming request, 4 outgoing request.
    pub kind: u64,
    pub since: Option<String>,
}

impl Relationship {
    pub fn parse(value: &Value, users: &HashMap<Id, User>) -> Option<Self> {
        let user = value.get("user").and_then(User::parse).or_else(|| {
            let id = field_sf(value, "user_id").or_else(|| field_sf(value, "id"))?;
            users.get(&id).cloned()
        })?;
        Some(Self {
            user,
            kind: value.get("type").and_then(Value::as_u64).unwrap_or(0),
            since: string(value, "since"),
        })
    }
}

/// A sound of the soundboard: a server's, or one of the defaults.
#[derive(Clone, Debug, Default)]
pub struct SoundboardSound {
    pub id: Id,
    pub name: String,
    /// Its own volume, 0 to 1.
    pub volume: f32,
    pub emoji_id: Option<Id>,
    pub emoji_name: Option<String>,
    /// None for the defaults, which Discord files under "0".
    pub guild: Option<Id>,
    pub available: bool,
}

impl SoundboardSound {
    pub fn parse(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str);
        Some(Self {
            id: text("sound_id")?.parse().ok()?,
            name: text("name").unwrap_or_default().to_owned(),
            volume: value.get("volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
            emoji_id: text("emoji_id").and_then(|id| id.parse().ok()),
            emoji_name: text("emoji_name").map(str::to_owned),
            guild: text("guild_id").and_then(|id| id.parse().ok()).filter(|id| *id != 0),
            available: value.get("available").and_then(Value::as_bool).unwrap_or(true),
        })
    }
}

/// Just enough protobuf to read the settings blob.
pub mod proto {
    pub enum Field<'a> {
        Varint(u64),
        Fixed64(u64),
        Bytes(&'a [u8]),
        Fixed32(u32),
    }

    impl<'a> Field<'a> {
        pub fn bytes(&self) -> Option<&'a [u8]> {
            match self {
                Self::Bytes(b) => Some(b),
                _ => None,
            }
        }

        /// A repeated fixed64, packed or not.
        pub fn fixed64s(&self) -> Vec<u64> {
            match self {
                Self::Fixed64(x) => vec![*x],
                Self::Bytes(b) => b
                    .chunks_exact(8)
                    .map(|c| u64::from_le_bytes(c.try_into().unwrap_or_default()))
                    .collect(),
                _ => Vec::new(),
            }
        }

        /// The value of a google.protobuf wrapper (`{ 1: value }`).
        pub fn wrapped_varint(&self) -> Option<u64> {
            fields(self.bytes()?).into_iter().find_map(|(n, v)| match (n, v) {
                (1, Field::Varint(x)) => Some(x),
                _ => None,
            })
        }
    }

    fn varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *buf.get(*pos)?;
            *pos += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    /// A packed repeated varint.
    pub fn varints(buf: &[u8]) -> Vec<u64> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(v) = varint(buf, &mut pos) {
            out.push(v);
        }
        out
    }

    /// Every field in a message, stopping at the first malformed one.
    pub fn fields(buf: &[u8]) -> Vec<(u32, Field<'_>)> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < buf.len() {
            let Some(key) = varint(buf, &mut pos) else { break };
            let number = (key >> 3) as u32;
            let field = match key & 7 {
                0 => match varint(buf, &mut pos) {
                    Some(v) => Field::Varint(v),
                    None => break,
                },
                1 => {
                    let Some(bytes) = buf.get(pos..pos + 8) else { break };
                    pos += 8;
                    Field::Fixed64(u64::from_le_bytes(bytes.try_into().unwrap_or_default()))
                }
                2 => {
                    let Some(len) = varint(buf, &mut pos) else { break };
                    let Some(bytes) = buf.get(pos..pos + len as usize) else { break };
                    pos += len as usize;
                    Field::Bytes(bytes)
                }
                5 => {
                    let Some(bytes) = buf.get(pos..pos + 4) else { break };
                    pos += 4;
                    Field::Fixed32(u32::from_le_bytes(bytes.try_into().unwrap_or_default()))
                }
                _ => break,
            };
            out.push((number, field));
        }
        out
    }
}


impl Ready {
    pub fn parse(d: &Value) -> Option<Self> {
        let me = User::parse(d.get("user")?)?;
        let users: HashMap<Id, User> = d
            .get("users")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(User::parse)
                    .map(|u| (u.id, u))
                    .collect()
            })
            .unwrap_or_default();
        let merged = d.get("merged_members").and_then(Value::as_array);
        let guilds = d
            .get("guilds")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .enumerate()
                    .filter_map(|(i, g)| {
                        let mine = merged
                            .and_then(|m| m.get(i))
                            .and_then(Value::as_array)
                            .and_then(|members| {
                                members.iter().find(|m| field_sf(m, "user_id") == Some(me.id))
                            });
                        Guild::parse(g, me.id, mine)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let private_channels = d
            .get("private_channels")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|c| Channel::parse(c, None, &users))
                    .collect()
            })
            .unwrap_or_default();
        let read_entries = match d.get("read_state") {
            Some(Value::Array(list)) => Some(list),
            Some(object) => object.get("entries").and_then(Value::as_array),
            None => None,
        };
        let read_states = read_entries
            .map(|list| {
                list.iter()
                    .filter(|e| e.get("read_state_type").and_then(Value::as_u64).unwrap_or(0) == 0)
                    .filter_map(|e| {
                        Some(ReadState {
                            channel: field_sf(e, "id")?,
                            last_message_id: field_sf(e, "last_message_id").unwrap_or(0),
                            mention_count: e
                                .get("mention_count")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u32,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let settings = d
            .get("user_settings_proto")
            .and_then(Value::as_str)
            .and_then(|b| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(b).ok()
            });
        let folders = settings
            .as_deref()
            .and_then(Folders::from_proto)
            .or_else(|| d.get("user_settings").and_then(Folders::from_json));
        let status = settings
            .as_deref()
            .and_then(proto_status)
            .or_else(|| {
                d.pointer("/user_settings/status")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "online".into());
        let relationships = d
            .get("relationships")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| Relationship::parse(r, &users))
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            me,
            guilds,
            private_channels,
            read_states,
            folders,
            mutes: Mutes::parse(d.get("user_guild_settings")),
            relationships,
            status,
            custom_status: settings.as_deref().and_then(proto_custom_status),
            settings_proto: settings.clone().unwrap_or_default(),
            verified: d.pointer("/user/verified").and_then(Value::as_bool).unwrap_or(true),
            email: d.pointer("/user/email").and_then(Value::as_str).map(str::to_owned),
            session_id: d.get("session_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
        })
    }
}

/// An entry of Discord's activity feed (`/content-inventory/users/@me`):
/// a game played, music listened to, the top artist or game of the week…
#[derive(Clone, Debug)]
pub struct FeedEntry {
    pub id: String,
    pub author: Id,
    /// ContentInventoryEntryType: 1 played game, 2 watched media, 3 top game,
    /// 5 listened session, 6 top artist, 8 launched activity.
    pub kind: u64,
    pub title: String,
    /// A listening session's artists, under the track.
    pub detail: String,
    pub image: Option<String>,
    pub application: Option<Id>,
    /// Unix ms; a game still being played has no end.
    pub started: Option<u64>,
    pub ended: Option<u64>,
    /// Traits: AGGREGATE_COUNT, STREAK_DAYS, DURATION_SECONDS, IS_LIVE.
    pub count: Option<u64>,
    pub streak: Option<u64>,
    pub duration: Option<u64>,
    pub live: bool,
}

impl FeedEntry {
    pub fn parse(value: &Value) -> Option<Self> {
        let content = value.get("content").unwrap_or(value);
        let extra = content.get("extra").unwrap_or(&Value::Null);
        let kind = content.get("content_type").and_then(Value::as_u64)?;
        let time = |key: &str| string(content, key).and_then(|t| t.parse::<jiff::Timestamp>().ok()).map(|t| t.as_millisecond().max(0) as u64);
        let traits = content.get("traits").and_then(Value::as_array).cloned().unwrap_or_default();
        let trait_of = |kind: u64| traits.iter().find(|t| t.get("type").and_then(Value::as_u64) == Some(kind));
        // A listening session holds its tracks, the latest first.
        let media = if kind == 5 { extra.pointer("/entries/0/media") } else { extra.get("media") }.unwrap_or(&Value::Null);
        let title = match kind {
            1 | 3 => extra.get("game_name").and_then(Value::as_str),
            6 => extra.pointer("/artist/name").and_then(Value::as_str),
            8 => extra.get("activity_name").or_else(|| extra.get("application_name")).and_then(Value::as_str),
            _ => media.get("title").and_then(Value::as_str),
        }
        .unwrap_or_default()
        .to_owned();
        let detail = media.get("artists").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        Some(Self {
            id: string(content, "id")?,
            author: field_sf(content, "author_id")?,
            kind,
            title,
            detail,
            image: media.get("image_url").and_then(Value::as_str).map(str::to_owned),
            application: extra.get("application_id").and_then(sf),
            started: time("started_at"),
            ended: time("ended_at"),
            count: trait_of(12).and_then(|t| t.get("count")).and_then(Value::as_u64),
            streak: trait_of(8).and_then(|t| t.get("streak_days")).and_then(Value::as_u64),
            duration: trait_of(2).and_then(|t| t.get("duration_seconds")).and_then(Value::as_u64),
            live: trait_of(3).and_then(|t| t.get("is_live")).and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// Played and not over yet.
    pub fn playing(&self) -> bool {
        self.kind == 1 && (self.live || self.ended.is_none())
    }
}

/// A server's public card, the one its tag opens (`/guilds/{id}/profile`):
/// banner, icon, counts, description, games and traits.
#[derive(Clone, Debug, Default)]
pub struct GuildProfile {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub banner: Option<String>,
    pub description: String,
    pub members: u64,
    pub online: u64,
    /// The colour its banner takes without a picture.
    pub brand: Option<u32>,
    /// Its tag and badge, as members wear them.
    pub tag: Option<(String, String)>,
    pub games: Vec<Id>,
    /// Emoji (custom id or unicode) and label, in order.
    pub traits: Vec<(Option<Id>, Option<String>, String)>,
}

impl GuildProfile {
    pub fn parse(value: &Value) -> Option<Self> {
        let count = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
        let mut traits: Vec<(u64, (Option<Id>, Option<String>, String))> = value
            .get("traits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let label = string(t, "label")?;
                Some((t.get("position").and_then(Value::as_u64).unwrap_or(0), (field_sf(t, "emoji_id"), string(t, "emoji_name").filter(|n| !n.is_empty()), label)))
            })
            .collect();
        traits.sort_by_key(|(position, _)| *position);
        Some(Self {
            id: field_sf(value, "id")?,
            name: string(value, "name").unwrap_or_default(),
            icon: string(value, "icon_hash"),
            banner: string(value, "custom_banner_hash").or_else(|| string(value, "banner_hash")),
            description: string(value, "description").unwrap_or_default(),
            members: count("member_count"),
            online: count("online_count"),
            brand: string(value, "brand_color_primary").and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok()),
            tag: string(value, "tag").map(|tag| (tag, string(value, "badge").or_else(|| string(value, "badge_hash")).unwrap_or_default())),
            games: value.get("game_application_ids").and_then(Value::as_array).into_iter().flatten().filter_map(sf).collect(),
            traits: traits.into_iter().map(|(_, t)| t).collect(),
        })
    }

    pub fn icon_url(&self) -> Option<String> {
        self.icon.as_ref().map(|hash| format!("https://cdn.discordapp.com/icons/{}/{hash}.png?size=128", self.id))
    }

    pub fn banner_url(&self) -> Option<String> {
        // The card's banner is the server's discovery splash (the bundle's
        // getGuildDiscoverySplashURL with custom_banner_hash).
        self.banner.as_ref().map(|hash| format!("https://cdn.discordapp.com/discovery-splashes/{}/{hash}.jpg?size=600", self.id))
    }

    pub fn badge_url(&self) -> Option<String> {
        let (_, badge) = self.tag.as_ref()?;
        (!badge.is_empty()).then(|| format!("https://cdn.discordapp.com/clan-badges/{}/{badge}.png?size=32", self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_server_card_with_its_traits_in_order() {
        let profile = GuildProfile::parse(&serde_json::json!({
            "id": "1200000000000000000", "name": "Dress To Impress Brasil!", "icon_hash": "abc",
            "member_count": 17826, "online_count": 1650, "description": "Farms", "brand_color_primary": "#5865f2",
            "tag": "DTI", "badge_hash": "b1", "game_application_ids": ["363445589247131668"],
            "traits": [{ "emoji_id": null, "emoji_name": "", "label": "Roblox", "position": 1 },
                       { "emoji_id": null, "emoji_name": "✨", "label": "Dress To Impress", "position": 0 }]
        }))
        .unwrap();
        assert_eq!((profile.members, profile.online), (17826, 1650));
        assert_eq!(profile.brand, Some(0x5865f2));
        assert_eq!(profile.games, [363445589247131668]);
        assert_eq!(profile.traits[0], (None, Some("✨".to_owned()), "Dress To Impress".to_owned()));
        assert_eq!(profile.traits[1].1, None);
        assert_eq!(profile.badge_url().as_deref(), Some("https://cdn.discordapp.com/clan-badges/1200000000000000000/b1.png?size=32"));
    }

    #[test]
    fn decodes_guild_folders() {
        // GuildFolders { folders: [GuildFolder { guild_ids: [7, 9] packed,
        // id: { value: 5 }, name: { value: "Jogos" }, color: { value: 255 } }],
        // guild_positions: [9] packed } at PreloadedUserSettings field 14.
        let mut folder = vec![0x0a, 16];
        folder.extend(7u64.to_le_bytes());
        folder.extend(9u64.to_le_bytes());
        folder.extend([0x12, 2, 0x08, 5]);
        folder.extend([0x1a, 7, 0x0a, 5]);
        folder.extend(b"Jogos");
        folder.extend([0x22, 3, 0x08, 0xff, 0x01]);
        let mut folders = vec![0x0a, folder.len() as u8];
        folders.extend(&folder);
        folders.extend([0x12, 8]);
        folders.extend(9u64.to_le_bytes());
        let mut settings = vec![0x72, folders.len() as u8];
        settings.extend(&folders);

        let parsed = Folders::from_proto(&settings).unwrap();
        assert_eq!(parsed.folders.len(), 1);
        let f = &parsed.folders[0];
        assert_eq!(f.guild_ids, vec![7, 9]);
        assert_eq!(f.id, Some(5));
        assert_eq!(f.name.as_deref(), Some("Jogos"));
        assert_eq!(f.color, Some(255));
        assert_eq!(parsed.positions, vec![9]);
    }

    #[test]
    fn reads_timed_mutes_and_notification_levels() {
        let end = (jiff::Timestamp::now() + jiff::SignedDuration::from_secs(3600)).to_string();
        let mut mutes = Mutes::parse(Some(&serde_json::json!([{
            "guild_id": "1",
            "muted": true,
            "mute_config": { "selected_time_window": 3600, "end_time": end },
            "message_notifications": 2,
            "channel_overrides": [
                { "channel_id": "5", "muted": true, "mute_config": null, "message_notifications": 0 },
                { "channel_id": "6", "muted": false, "message_notifications": 3 }
            ]
        }])));
        assert!(mutes.guilds.contains(&1) && mutes.channels.contains(&5));
        assert_eq!(mutes.levels.get(&1), Some(&2));
        assert_eq!(mutes.levels.get(&5), Some(&0));
        assert_eq!(mutes.levels.get(&6), None, "3 follows the category or server");
        let ends = mutes.ends[&1];
        assert!(!mutes.expire(ends - 1));
        assert!(mutes.expire(ends));
        assert!(!mutes.guilds.contains(&1), "a timed mute ends");
        assert!(mutes.channels.contains(&5), "a mute until unmuted stays");
    }

    #[test]
    fn computes_view_permission() {
        let guild = Guild {
            id: 1,
            name: String::new(),
            icon: None,
            owner_id: Some(99),
            channels: Vec::new(),
            roles: vec![
                Role { id: 1, name: "@everyone".into(), permissions: VIEW_CHANNEL, color: 0, color2: None, position: 0, hoist: false, mentionable: false, managed: false },
                Role { id: 2, name: "Mod".into(), permissions: 0, color: 0xff0000, color2: None, position: 1, hoist: true, mentionable: false, managed: false },
            ],
            my_roles: vec![2],
            voice_states: Vec::new(),
            voice_flags: Vec::new(),
            emojis: Vec::new(),
            stickers: Vec::new(),
            default_notifications: 1,
            features: Vec::new(),
            events: 0,
            joined: None,
            banner: None,
            boosts: 0,
            boost_bar: false,
        };
        let mut channel =
            Channel::parse(&serde_json::json!({"id": "5", "type": 0}), Some(1), &HashMap::new())
                .unwrap();
        assert!(guild.can_view(3, &channel));
        channel.overwrites.push(Overwrite { id: 1, kind: 0, allow: 0, deny: VIEW_CHANNEL });
        assert!(!guild.can_view(3, &channel));
        channel.overwrites.push(Overwrite { id: 2, kind: 0, allow: VIEW_CHANNEL, deny: 0 });
        assert!(guild.can_view(3, &channel));
        assert_eq!(guild.role_color(&[2]), 0xff0000);
    }
}
