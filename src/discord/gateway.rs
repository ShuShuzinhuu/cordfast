//! The real-time gateway: one WebSocket that identifies, heartbeats,
//! resumes after drops, and turns dispatches into [`Dispatch`]es.
//!
//! No transport compression: JSON is parsed straight from text frames, and
//! nothing but the current session is kept.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Message as WsMessage, client::IntoClientRequest};

use super::model::{Channel, Emoji, Folders, Guild, Id, Member, Message, Ready, Relationship, User, sf};
use super::rest::{USER_AGENT, super_properties};

const GATEWAY: &str = "wss://gateway.discord.gg";

/// LAZY_USER_NOTES | VERSIONED_READ_STATES | VERSIONED_USER_GUILD_SETTINGS |
/// DEDUPE_USER_OBJECTS | USER_SETTINGS_PROTO | CLIENT_STATE_V2.
/// [`Ready::parse`] reads both the deduplicated and the inline shapes. (The
/// web client sends 1734653, which adds bits whose payloads we do not read.)
const CAPABILITIES: u64 = 1 | 4 | 8 | 16 | 512 | 1024;

#[derive(Debug)]
pub enum Dispatch {
    Connecting,
    Connected,
    Disconnected,
    /// The token no longer works: logged out elsewhere or changed password.
    AuthFailed,
    Ready(Box<Ready>),
    GuildCreate(Box<Guild>),
    GuildDelete(Id),
    ChannelUpsert(Channel),
    ChannelDelete(Id),
    MessageCreate(Box<Message>),
    MessageUpdate(Box<Message>),
    MessageDelete { channel: Id, id: Id },
    /// Read on another device.
    Ack { channel: Id, message: Id },
    Typing { channel: Id, user: Id },
    Members { guild: Id, members: Vec<(User, Member)> },
    ReactionAdd { channel: Id, message: Id, user: Id, emoji: Emoji },
    ReactionRemove { channel: Id, message: Id, user: Id, emoji: Emoji },
    ReactionsCleared { channel: Id, message: Id },
    ReactionEmojiCleared { channel: Id, message: Id, emoji: Emoji },
    UserUpdate(User),
    MemberRemoved { guild: Id, user: Id },
    RelationshipAdd(Relationship),
    RelationshipRemove { user: Id, kind: u64 },
    GuildSettings(Value),
    Folders(Folders),
    /// The account's settings changed: a whole PreloadedUserSettings, or
    /// only what changed.
    Preloaded { bytes: Vec<u8>, partial: bool },
    /// Someone joined, left or changed state in a voice channel.
    VoiceState(Value),
    VoiceServer(Value),
    /// A DM or group call started, changed or ended.
    Call(Value),
    CallEnded(Id),
    RoleUpsert { guild: Id, role: Value },
    RoleDelete { guild: Id, role: Id },
    /// The guild's own fields changed (name, icon, owner...).
    GuildUpdate(Value),
    /// Online status and what people are doing: (user, status, activity).
    Presences(Vec<Presence>),
    GuildEmojis { guild: Id, emojis: Vec<Emoji> },
    MemberList(MemberListUpdate),
    PollVote { channel: Id, message: Id, user: Id, answer: u64, add: bool },
    /// A voice channel's status changed (none when cleared).
    VoiceChannelStatus { channel: Id, status: Option<String> },
    /// Someone in a call sent a sound or an emoji effect.
    VoiceEffect { channel: Id, user: Id, sound: Option<Id>, volume: f32, emoji_id: Option<Id>, emoji_name: Option<String> },
    /// A server's sounds, as answers to op 31.
    Soundboard { guild: Id, sounds: Vec<super::model::SoundboardSound> },
    SoundboardChanged(Id),
    /// An application's suggestions for the option being typed.
    Autocomplete { nonce: String, choices: Vec<(String, Value)> },
    /// A command or button got no answer from its application.
    InteractionFailed { nonce: String },
}

/// One row of a channel's member list.
#[derive(Clone, Debug)]
pub enum ListItem {
    /// A section: "online", "offline" or a hoisted role's id, and its size.
    Group { id: String, count: u64 },
    Member { user: User, member: Member, presence: Option<Presence> },
}

impl ListItem {
    fn parse(value: &Value) -> Option<Self> {
        if let Some(group) = value.get("group") {
            return Some(Self::Group {
                id: group.get("id")?.as_str()?.to_owned(),
                count: group.get("count").and_then(Value::as_u64).unwrap_or(0),
            });
        }
        let m = value.get("member")?;
        let user = User::parse(m.get("user")?)?;
        let presence = m.get("presence").and_then(|p| {
            let mut p = p.clone();
            p["user_id"] = Value::String(user.id.to_string());
            Presence::parse(&p)
        });
        Some(Self::Member { member: Member::parse(m), user, presence })
    }
}

#[derive(Clone, Debug)]
pub enum ListOp {
    Sync { start: usize, items: Vec<ListItem> },
    Insert { index: usize, item: ListItem },
    Update { index: usize, item: ListItem },
    Delete { index: usize },
    Invalidate { start: usize, end: usize },
}

#[derive(Clone, Debug)]
pub struct MemberListUpdate {
    pub guild: Id,
    pub list: String,
    pub online: u64,
    pub total: u64,
    pub groups: Vec<(String, u64)>,
    pub ops: Vec<ListOp>,
}

impl MemberListUpdate {
    pub fn parse(d: &Value) -> Option<Self> {
        let range = |op: &Value| -> Option<(usize, usize)> {
            let r = op.get("range")?.as_array()?;
            Some((r.first()?.as_u64()? as usize, r.get(1)?.as_u64()? as usize))
        };
        let ops = d
            .get("ops")?
            .as_array()?
            .iter()
            .filter_map(|op| {
                let index = || op.get("index").and_then(Value::as_u64).map(|i| i as usize);
                Some(match op.get("op")?.as_str()? {
                    "SYNC" => ListOp::Sync {
                        start: range(op)?.0,
                        items: op.get("items")?.as_array()?.iter().filter_map(ListItem::parse).collect(),
                    },
                    "INSERT" => ListOp::Insert { index: index()?, item: ListItem::parse(op.get("item")?)? },
                    "UPDATE" => ListOp::Update { index: index()?, item: ListItem::parse(op.get("item")?)? },
                    "DELETE" => ListOp::Delete { index: index()? },
                    "INVALIDATE" => {
                        let (start, end) = range(op)?;
                        ListOp::Invalidate { start, end }
                    }
                    _ => return None,
                })
            })
            .collect();
        Some(Self {
            guild: d.get("guild_id").and_then(sf)?,
            list: d.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
            online: d.get("online_count").and_then(Value::as_u64).unwrap_or(0),
            total: d.get("member_count").and_then(Value::as_u64).unwrap_or(0),
            groups: d
                .get("groups")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|g| Some((g.get("id")?.as_str()?.to_owned(), g.get("count").and_then(Value::as_u64).unwrap_or(0))))
                        .collect()
                })
                .unwrap_or_default(),
            ops,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Presence {
    pub user: Id,
    pub status: String,
    /// Every activity in full, for the cards in profiles.
    pub activities: Vec<Activity>,
    /// The custom status, its emoji first: the bubble beside the avatar.
    pub custom: Option<String>,
}

/// One activity as Discord shows it in a card: Spotify, a game with rich
/// presence, Crunchyroll, a stream, a custom status.
#[derive(Clone, Debug)]
pub struct Activity {
    /// 0 playing, 1 streaming, 2 listening, 3 watching, 4 custom, 5 competing.
    pub kind: u64,
    pub name: String,
    pub application: Option<Id>,
    pub details: Option<String>,
    pub state: Option<String>,
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub url: Option<String>,
    /// (label, url) of the buttons a game shows, when their URLs came along.
    pub buttons: Vec<String>,
    /// Spotify's track id, for "Play on Spotify".
    pub sync_id: Option<String>,
}

impl Activity {
    pub fn parse(a: &Value) -> Option<Self> {
        let text = |key: &str| a.get(key).and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_owned);
        let application = a.get("application_id").and_then(sf);
        let image = |key: &str| {
            a.pointer(&format!("/assets/{key}"))
                .and_then(Value::as_str)
                .and_then(|asset| asset_url(asset, application))
        };
        Some(Self {
            kind: a.get("type").and_then(Value::as_u64).unwrap_or(0),
            name: text("name")?,
            application,
            details: text("details"),
            state: text("state"),
            large_image: image("large_image"),
            large_text: a.pointer("/assets/large_text").and_then(Value::as_str).map(str::to_owned),
            small_image: image("small_image"),
            start: a.pointer("/timestamps/start").and_then(Value::as_u64),
            end: a.pointer("/timestamps/end").and_then(Value::as_u64),
            url: text("url"),
            buttons: a
                .get("buttons")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(|b| b.as_str().or_else(|| b.get("label")?.as_str()).map(str::to_owned)).collect())
                .unwrap_or_default(),
            sync_id: text("sync_id"),
        })
    }

    /// "Listening to Spotify", "Playing", as the card's header says.
    pub fn header(&self) -> String {
        match self.kind {
            1 => t!("Streaming {name}", name = self.name),
            2 => t!("Listening to {name}", name = self.name),
            3 => t!("Watching {name}", name = self.name),
            5 => t!("Competing in {name}", name = self.name),
            _ => t!("Playing").to_owned(),
        }
    }

    /// The line lists show under a name: "Playing X", "Listening to Y".
    pub fn line(&self) -> String {
        match self.kind {
            0 => t!("Playing {name}", name = self.name),
            1 => t!("Streaming {name}", name = self.name),
            2 => t!("Listening to {name}", name = self.name),
            3 => t!("Watching {name}", name = self.name),
            5 => t!("Competing in {name}", name = self.name),
            _ => self.name.clone(),
        }
    }
}

/// An activity asset as a picture URL: Spotify covers, the media proxy's
/// external images, or the application's own uploaded art.
fn asset_url(asset: &str, application: Option<Id>) -> Option<String> {
    if let Some(cover) = asset.strip_prefix("spotify:") {
        return Some(format!("https://i.scdn.co/image/{cover}"));
    }
    if let Some(path) = asset.strip_prefix("mp:") {
        return Some(format!("https://media.discordapp.net/{path}"));
    }
    if asset.starts_with("http") {
        return Some(asset.to_owned());
    }
    Some(format!("https://cdn.discordapp.com/app-assets/{}/{asset}.png", application?))
}

impl Presence {
    pub fn parse(value: &Value) -> Option<Self> {
        let user = value
            .get("user_id")
            .and_then(sf)
            .or_else(|| value.pointer("/user/id").and_then(sf))?;
        let activities = value.get("activities").and_then(Value::as_array);
        let custom = activities.and_then(|list| list.iter().find(|a| a.get("type").and_then(Value::as_u64) == Some(4))).and_then(|a| {
            // A Unicode emoji travels as its name; a server's has an id.
            let emoji = a.pointer("/emoji/name").and_then(Value::as_str).filter(|_| a.pointer("/emoji/id").is_none_or(Value::is_null));
            let state = a.get("state").and_then(Value::as_str).unwrap_or_default();
            let text = [emoji.unwrap_or_default(), state].iter().filter(|t| !t.is_empty()).copied().collect::<Vec<_>>().join(" ");
            (!text.is_empty()).then_some(text)
        });
        Some(Self {
            user,
            status: value.get("status").and_then(Value::as_str).unwrap_or("offline").to_owned(),
            custom,
            activities: activities
                .map(|l| l.iter().filter(|a| a.get("type").and_then(Value::as_u64) != Some(4)).filter_map(Activity::parse).collect())
                .unwrap_or_default(),
        })
    }

    /// "Playing X", "Listening to Y" or the custom status, in the
    /// interface's language when shown.
    pub fn activity(&self) -> Option<String> {
        self.activities.first().map(Activity::line).or_else(|| self.custom.clone())
    }

    /// Every presence in READY or READY_SUPPLEMENTAL, in either shape.
    pub fn all_in(d: &Value) -> Vec<Self> {
        let mut out = Vec::new();
        let mut take = |list: Option<&Value>| {
            for p in list.and_then(Value::as_array).into_iter().flatten() {
                out.extend(Self::parse(p));
            }
        };
        take(d.get("presences"));
        take(d.pointer("/merged_presences/friends"));
        if let Some(guilds) = d.pointer("/merged_presences/guilds").and_then(Value::as_array) {
            for g in guilds {
                take(Some(g));
            }
        }
        out
    }
}

pub enum Control {
    /// Asks for a guild's message events, which large guilds send only to
    /// clients looking at them.
    Subscribe(Id),
    /// Op 31: the soundboards of these servers.
    RequestSoundboard(Vec<Id>),
    /// Asks for a channel's member list, first `rows` rows (opcode 37).
    MemberList { guild: Id, channel: Id, rows: u64 },
    /// Asks for these members' nicknames and roles (opcode 8).
    RequestMembers { guild: Id, users: Vec<Id> },
    /// Joins (`channel` set) or leaves a voice channel (opcode 4).
    VoiceState {
        guild: Option<Id>,
        channel: Option<Id>,
        mute: bool,
        deaf: bool,
        /// Regions fastest first, as the desktop client ranks them.
        regions: Vec<String>,
    },
    /// Our status and activities (opcode 3): rich presence.
    Presence { status: String, activities: Vec<Value> },
    Shutdown,
}

pub type Sink = Arc<dyn Fn(Dispatch) + Send + Sync>;

struct Session {
    id: String,
    resume_url: String,
    seq: Option<u64>,
    me: Id,
}

enum End {
    Reconnect,
    Shutdown,
    AuthFailed,
}

pub async fn run(token: String, out: Sink, mut control: mpsc::UnboundedReceiver<Control>) {
    let mut session: Option<Session> = None;
    let mut backoff = 1u64;
    loop {
        out(Dispatch::Connecting);
        match connect(&token, &mut session, &out, &mut control, &mut backoff).await {
            Ok(End::Shutdown) => return,
            Ok(End::AuthFailed) => {
                out(Dispatch::AuthFailed);
                return;
            }
            Ok(End::Reconnect) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(error) => {
                log::warn!("gateway: {error:#}");
                out(Dispatch::Disconnected);
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        }
    }
}

async fn connect(
    token: &str,
    session: &mut Option<Session>,
    out: &Sink,
    control: &mut mpsc::UnboundedReceiver<Control>,
    backoff: &mut u64,
) -> Result<End> {
    let base = session
        .as_ref()
        .map_or(GATEWAY, |s| s.resume_url.as_str())
        .trim_end_matches('/');
    let mut request = format!("{base}/?v=9&encoding=json").into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", "https://discord.com".parse()?);
    request
        .headers_mut()
        .insert("User-Agent", USER_AGENT.parse()?);
    let config = WebSocketConfig::default()
        .max_message_size(Some(512 << 20))
        .max_frame_size(Some(512 << 20));
    let (socket, _) = tokio_tungstenite::connect_async_with_config(request, Some(config), false)
        .await
        .context(t!("no connection to Discord"))?;
    let (mut tx, mut rx) = socket.split();

    let hello = next_json(&mut rx).await?;
    let interval = hello
        .pointer("/d/heartbeat_interval")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("gateway without HELLO"))?;

    let start = match session {
        Some(s) => json!({"op": 6, "d": {"token": token, "session_id": s.id, "seq": s.seq}}),
        None => json!({"op": 2, "d": {
            "token": token,
            "capabilities": CAPABILITIES,
            "properties": super_properties(),
            "presence": {"status": "unknown", "since": 0, "activities": [], "afk": false},
            "compress": false,
            "client_state": {"guild_versions": {}},
        }}),
    };
    tx.send(WsMessage::text(start.to_string())).await?;

    let jitter = Duration::from_millis(interval * u64::from(rand::random::<u8>()) / 255);
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + jitter,
        Duration::from_millis(interval),
    );
    let mut acked = true;
    let seq = |session: &Option<Session>| session.as_ref().and_then(|s| s.seq);
    let mut pending_seq: Option<u64> = None;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if !acked {
                    return Ok(End::Reconnect);
                }
                acked = false;
                let s = seq(session).or(pending_seq);
                tx.send(WsMessage::text(json!({"op": 1, "d": s}).to_string())).await?;
            }
            command = control.recv() => match command {
                Some(Control::Subscribe(guild)) => {
                    let subscribe = json!({"op": 37, "d": {"subscriptions": {
                        guild.to_string(): {"typing": true, "threads": true, "activities": false}
                    }}});
                    tx.send(WsMessage::text(subscribe.to_string())).await?;
                    // The voice channels' statuses set before we connected,
                    // as the web client asks them (answered by CHANNEL_INFO).
                    let info = json!({"op": 43, "d": {"guild_id": guild.to_string(), "fields": ["status", "voice_start_time"]}});
                    tx.send(WsMessage::text(info.to_string())).await?;
                }
                Some(Control::RequestSoundboard(guilds)) => {
                    let ids: Vec<String> = guilds.iter().map(Id::to_string).collect();
                    let ask = json!({"op": 31, "d": {"guild_ids": ids}});
                    tx.send(WsMessage::text(ask.to_string())).await?;
                }
                Some(Control::MemberList { guild, channel, rows }) => {
                    // Ranges of 100 rows, as the web client asks them.
                    let ranges: Vec<[u64; 2]> = (0..rows.div_ceil(100)).map(|i| [i * 100, i * 100 + 99]).collect();
                    let subscribe = json!({"op": 37, "d": {"subscriptions": {
                        guild.to_string(): {
                            "typing": true,
                            "threads": true,
                            "activities": true,
                            "channels": { channel.to_string(): ranges },
                        }
                    }}});
                    tx.send(WsMessage::text(subscribe.to_string())).await?;
                }
                Some(Control::RequestMembers { guild, users }) => {
                    for chunk in users.chunks(100) {
                        let ids: Vec<String> = chunk.iter().map(ToString::to_string).collect();
                        let request = json!({"op": 8, "d": {
                            "guild_id": [guild.to_string()],
                            "user_ids": ids,
                            "presences": false,
                        }});
                        tx.send(WsMessage::text(request.to_string())).await?;
                    }
                }
                Some(Control::VoiceState { guild, channel, mute, deaf, regions }) => {
                    let mut state = json!({"op": 4, "d": {
                        "guild_id": guild.map(|g| g.to_string()),
                        "channel_id": channel.map(|c| c.to_string()),
                        "self_mute": mute,
                        "self_deaf": deaf,
                        "self_video": false,
                        "flags": 2,
                    }});
                    if channel.is_some() && !regions.is_empty() {
                        state["d"]["preferred_region"] = json!(regions[0]);
                        state["d"]["preferred_regions"] = json!(regions);
                    }
                    tx.send(WsMessage::text(state.to_string())).await?;
                }
                Some(Control::Presence { status, activities }) => {
                    let presence = json!({"op": 3, "d": {
                        "status": status,
                        "since": 0,
                        "activities": activities,
                        "afk": false,
                    }});
                    tx.send(WsMessage::text(presence.to_string())).await?;
                }
                Some(Control::Shutdown) | None => {
                    let _ = tx.send(WsMessage::Close(Some(CloseFrame {
                        code: 1000.into(),
                        reason: "".into(),
                    }))).await;
                    return Ok(End::Shutdown);
                }
            },
            frame = rx.next() => {
                let text = match frame {
                    Some(Ok(WsMessage::Text(text))) => text,
                    Some(Ok(WsMessage::Close(close))) => {
                        let code = close.map_or(1000, |c| u16::from(c.code));
                        log::info!("gateway closed: {code}");
                        return Ok(match code {
                            4004 => End::AuthFailed,
                            4007 | 4009 => {
                                *session = None;
                                End::Reconnect
                            }
                            4010..=4014 => return Err(anyhow!("{}", t!("the gateway refused the connection ({code})", code = code))),
                            _ => End::Reconnect,
                        });
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => return Err(error.into()),
                    None => return Ok(End::Reconnect),
                };
                let payload: Value = serde_json::from_str(text.as_str())?;
                if let Some(s) = payload.get("s").and_then(Value::as_u64) {
                    match session.as_mut() {
                        Some(session) => session.seq = Some(s),
                        None => pending_seq = Some(s),
                    }
                }
                match payload.get("op").and_then(Value::as_u64) {
                    Some(0) => {
                        let name = payload.get("t").and_then(Value::as_str).unwrap_or_default();
                        let d = payload.get("d").unwrap_or(&Value::Null);
                        match name {
                            "READY" => {
                                let ready = Ready::parse(d).ok_or_else(|| anyhow!("invalid READY"))?;
                                *session = Some(Session {
                                    id: d.get("session_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                                    resume_url: d
                                        .get("resume_gateway_url")
                                        .and_then(Value::as_str)
                                        .unwrap_or(GATEWAY)
                                        .to_owned(),
                                    seq: pending_seq.take(),
                                    me: ready.me.id,
                                });
                                *backoff = 1;
                                let presences = Presence::all_in(d);
                                out(Dispatch::Ready(Box::new(ready)));
                                if !presences.is_empty() {
                                    out(Dispatch::Presences(presences));
                                }
                                out(Dispatch::Connected);
                            }
                            "RESUMED" => {
                                *backoff = 1;
                                out(Dispatch::Connected);
                            }
                            _ => {
                                let me = session.as_ref().map_or(0, |s| s.me);
                                for dispatch in translate(name, d, me) {
                                    out(dispatch);
                                }
                            }
                        }
                    }
                    Some(1) => {
                        let s = seq(session);
                        tx.send(WsMessage::text(json!({"op": 1, "d": s}).to_string())).await?;
                    }
                    Some(7) => return Ok(End::Reconnect),
                    Some(9) => {
                        if payload.get("d").and_then(Value::as_bool) != Some(true) {
                            *session = None;
                        }
                        tokio::time::sleep(Duration::from_millis(1000 + u64::from(rand::random::<u16>() % 4000))).await;
                        return Ok(End::Reconnect);
                    }
                    Some(11) => acked = true,
                    _ => {}
                }
            }
        }
    }
}

async fn next_json<S>(rx: &mut S) -> Result<Value>
where
    S: futures_util::Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    loop {
        match rx.next().await {
            Some(Ok(WsMessage::Text(text))) => return Ok(serde_json::from_str(text.as_str())?),
            Some(Ok(WsMessage::Close(_))) | None => return Err(anyhow!("the gateway closed")),
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(error.into()),
        }
    }
}

/// A voice channel status in an event, none when it was cleared.
fn voice_status(value: &Value) -> Option<String> {
    value.get("status").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

fn translate(name: &str, d: &Value, me: Id) -> Vec<Dispatch> {
    let id = |key: &str| d.get(key).and_then(sf);
    let one = |dispatch: Option<Dispatch>| dispatch.into_iter().collect::<Vec<_>>();
    let reaction = || -> Option<(Id, Id, Id, Emoji)> {
        Some((
            id("channel_id")?,
            id("message_id")?,
            id("user_id")?,
            Emoji::parse(d.get("emoji")?)?,
        ))
    };
    match name {
        "GUILD_CREATE" => one(Guild::parse(d, me, None).map(|g| Dispatch::GuildCreate(Box::new(g)))),
        "GUILD_DELETE" => one(id("id").map(Dispatch::GuildDelete)),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            one(Channel::parse(d, None, &Default::default()).map(Dispatch::ChannelUpsert))
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => one(id("id").map(Dispatch::ChannelDelete)),
        "THREAD_LIST_SYNC" => {
            let guild = id("guild_id");
            d.get("threads")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|t| Channel::parse(t, guild, &Default::default()))
                .map(Dispatch::ChannelUpsert)
                .collect()
        }
        "MESSAGE_CREATE" => one(Message::parse(d).map(|m| Dispatch::MessageCreate(Box::new(m)))),
        "MESSAGE_UPDATE" => one(Message::parse(d).map(|m| Dispatch::MessageUpdate(Box::new(m)))),
        "MESSAGE_DELETE" => one((|| {
            Some(Dispatch::MessageDelete {
                channel: id("channel_id")?,
                id: id("id")?,
            })
        })()),
        "MESSAGE_DELETE_BULK" => {
            let channel = id("channel_id");
            d.get("ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| Some(Dispatch::MessageDelete { channel: channel?, id: sf(m)? }))
                .collect()
        }
        "MESSAGE_ACK" => one((|| {
            Some(Dispatch::Ack {
                channel: id("channel_id")?,
                message: id("message_id")?,
            })
        })()),
        "TYPING_START" => one((|| {
            Some(Dispatch::Typing {
                channel: id("channel_id")?,
                user: id("user_id")?,
            })
        })()),
        "GUILD_MEMBERS_CHUNK" => one((|| {
            let members = d
                .get("members")?
                .as_array()?
                .iter()
                .filter_map(|m| Some((User::parse(m.get("user")?)?, Member::parse(m))))
                .collect();
            Some(Dispatch::Members { guild: id("guild_id")?, members })
        })()),
        "GUILD_MEMBER_UPDATE" | "GUILD_MEMBER_ADD" => one((|| {
            let user = User::parse(d.get("user")?)?;
            Some(Dispatch::Members {
                guild: id("guild_id")?,
                members: vec![(user, Member::parse(d))],
            })
        })()),
        "MESSAGE_REACTION_ADD" => one(reaction().map(|(channel, message, user, emoji)| {
            Dispatch::ReactionAdd { channel, message, user, emoji }
        })),
        "MESSAGE_REACTION_REMOVE" => one(reaction().map(|(channel, message, user, emoji)| {
            Dispatch::ReactionRemove { channel, message, user, emoji }
        })),
        "MESSAGE_REACTION_REMOVE_EMOJI" => one((|| {
            Some(Dispatch::ReactionEmojiCleared { channel: id("channel_id")?, message: id("message_id")?, emoji: Emoji::parse(d.get("emoji")?)? })
        })()),
        "USER_UPDATE" => one(User::parse(d).map(Dispatch::UserUpdate)),
        "GUILD_MEMBER_REMOVE" => one((|| {
            Some(Dispatch::MemberRemoved { guild: id("guild_id")?, user: d.pointer("/user/id").and_then(sf)? })
        })()),
        "MESSAGE_REACTION_REMOVE_ALL" => one((|| {
            Some(Dispatch::ReactionsCleared {
                channel: id("channel_id")?,
                message: id("message_id")?,
            })
        })()),
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => one(
            Relationship::parse(d, &Default::default()).map(Dispatch::RelationshipAdd),
        ),
        "RELATIONSHIP_REMOVE" => one(id("id").map(|user| Dispatch::RelationshipRemove {
            user,
            kind: d.get("type").and_then(Value::as_u64).unwrap_or(0),
        })),
        "USER_GUILD_SETTINGS_UPDATE" => vec![Dispatch::GuildSettings(d.clone())],
        "USER_SETTINGS_PROTO_UPDATE" => (|| {
            if d.pointer("/settings/type").and_then(Value::as_u64) != Some(1) {
                return None;
            }
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(d.pointer("/settings/proto")?.as_str()?)
                .ok()?;
            let partial = d.get("partial").and_then(Value::as_bool).unwrap_or(false);
            let folders = Folders::from_proto(&bytes);
            let mut out = vec![Dispatch::Preloaded { bytes, partial }];
            out.extend(folders.map(Dispatch::Folders));
            Some(out)
        })().into_iter().flatten().collect(),
        "VOICE_STATE_UPDATE" => vec![Dispatch::VoiceState(d.clone())],
        "VOICE_CHANNEL_STATUS_UPDATE" => one(id("id").map(|channel| Dispatch::VoiceChannelStatus { channel, status: voice_status(d) })),
        // Every voice channel's status at once, as answers to the client
        // asking for a guild's.
        "CHANNEL_STATUSES" | "CHANNEL_INFO" => d
            .get("channels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| Some(Dispatch::VoiceChannelStatus { channel: c.get("id").and_then(sf)?, status: voice_status(c) }))
            .collect(),
        "VOICE_CHANNEL_EFFECT_SEND" => one((|| {
            Some(Dispatch::VoiceEffect {
                channel: id("channel_id")?,
                user: id("user_id")?,
                sound: d.get("sound_id").and_then(sf),
                volume: d.get("sound_volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                emoji_id: d.pointer("/emoji/id").and_then(sf),
                emoji_name: d.pointer("/emoji/name").and_then(Value::as_str).map(str::to_owned),
            })
        })()),
        "SOUNDBOARD_SOUNDS" => one((|| {
            Some(Dispatch::Soundboard { guild: id("guild_id")?, sounds: d.get("soundboard_sounds")?.as_array()?.iter().filter_map(super::model::SoundboardSound::parse).collect() })
        })()),
        "GUILD_SOUNDBOARD_SOUND_CREATE" | "GUILD_SOUNDBOARD_SOUND_UPDATE" | "GUILD_SOUNDBOARD_SOUND_DELETE" | "GUILD_SOUNDBOARD_SOUNDS_UPDATE" => one(id("guild_id").map(Dispatch::SoundboardChanged)),
        "VOICE_SERVER_UPDATE" => vec![Dispatch::VoiceServer(d.clone())],
        "CALL_CREATE" | "CALL_UPDATE" => vec![Dispatch::Call(d.clone())],
        "CALL_DELETE" => one(id("channel_id").map(Dispatch::CallEnded)),
        "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => one((|| {
            Some(Dispatch::RoleUpsert { guild: id("guild_id")?, role: d.get("role")?.clone() })
        })()),
        "GUILD_ROLE_DELETE" => one((|| {
            Some(Dispatch::RoleDelete { guild: id("guild_id")?, role: id("role_id")? })
        })()),
        "GUILD_UPDATE" => vec![Dispatch::GuildUpdate(d.clone())],
        "PRESENCE_UPDATE" => one(Presence::parse(d).map(|p| Dispatch::Presences(vec![p]))),
        "READY_SUPPLEMENTAL" => vec![Dispatch::Presences(Presence::all_in(d))],
        "SESSIONS_REPLACE" => Vec::new(),
        "MESSAGE_POLL_VOTE_ADD" | "MESSAGE_POLL_VOTE_REMOVE" => one((|| {
            Some(Dispatch::PollVote {
                channel: id("channel_id")?,
                message: id("message_id")?,
                user: id("user_id")?,
                answer: d.get("answer_id")?.as_u64()?,
                add: name == "MESSAGE_POLL_VOTE_ADD",
            })
        })()),
        "GUILD_MEMBER_LIST_UPDATE" => one(MemberListUpdate::parse(d).map(Dispatch::MemberList)),
        "APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE" => one((|| {
            Some(Dispatch::Autocomplete {
                nonce: d.get("nonce")?.as_str()?.to_owned(),
                choices: d
                    .get("choices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| Some((c.get("name")?.as_str()?.to_owned(), c.get("value")?.clone())))
                    .collect(),
            })
        })()),
        "INTERACTION_FAILURE" => one(d.get("nonce").and_then(Value::as_str).map(|nonce| Dispatch::InteractionFailed { nonce: nonce.to_owned() })),
        "GUILD_EMOJIS_UPDATE" => one((|| {
            Some(Dispatch::GuildEmojis {
                guild: id("guild_id")?,
                emojis: d.get("emojis")?.as_array()?.iter().filter_map(Emoji::parse).collect(),
            })
        })()),
        _ => Vec::new(),
    }
}
