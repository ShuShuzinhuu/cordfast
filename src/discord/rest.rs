//! Discord's HTTP API, as the web client calls it.

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::{Value, json};

use super::model::{Channel, Id, Member, Message, User};

pub const API: &str = "https://discord.com/api/v9";

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

/// The web client's build, read from discord.com/app's `BUILD_NUMBER`
/// (2026-10-27).
const CLIENT_BUILD_NUMBER: u64 = 652_418;

/// The client description the gateway's IDENTIFY and every request carry,
/// the same a Chrome tab on discord.com sends.
pub fn super_properties() -> Value {
    json!({
        "os": "Windows",
        "browser": "Chrome",
        "device": "",
        "system_locale": crate::i18n::Locale::system().discord_tag(),
        "browser_user_agent": USER_AGENT,
        "browser_version": "141.0.0.0",
        "os_version": "10",
        "referrer": "",
        "referring_domain": "",
        "referrer_current": "",
        "referring_domain_current": "",
        "release_channel": "stable",
        "client_build_number": CLIENT_BUILD_NUMBER,
        "client_event_source": null,
    })
}

/// A client without a token, for the QR login and images.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(true)
        .pool_idle_timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("unable to build the HTTP client")
}

/// Discord asking for a second factor before a sensitive call: 401 with code
/// 60003, a ticket and the ways this account can answer.
#[derive(Debug)]
pub struct MfaRequired {
    pub ticket: String,
    pub methods: Vec<String>,
}

impl std::fmt::Display for MfaRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", t!("Multi-Factor Authentication"))
    }
}

impl std::error::Error for MfaRequired {}

/// Up to here files go inside the message's own request.
const DIRECT_UPLOAD: u64 = 8 * 1024 * 1024;

/// The token each ack's answer gives for the next, as the web client keeps it.
static ACK_TOKEN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    token: String,
    super_properties: String,
}

impl Api {
    pub fn new(http: reqwest::Client, token: String) -> Self {
        let super_properties =
            base64::engine::general_purpose::STANDARD.encode(super_properties().to_string());
        Self {
            http,
            token,
            super_properties,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{API}{path}"))
            .header("Authorization", &self.token)
            .header("X-Super-Properties", &self.super_properties)
            .header("X-Discord-Locale", crate::i18n::current().discord_tag())
            .header("Origin", "https://discord.com")
            .header("Referer", "https://discord.com/channels/@me")
    }

    async fn send(&self, builder: reqwest::RequestBuilder) -> Result<Value> {
        if let Some(request) = builder.try_clone().and_then(|b| b.build().ok()) {
            let path = request.url().path().trim_start_matches("/api/v9");
            if NO_TRACK.load(std::sync::atomic::Ordering::Relaxed) && is_tracking(path) {
                return Ok(Value::Null);
            }
        }
        let response = builder.send().await.context(t!("no connection"))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let answer = serde_json::from_str::<Value>(&body).ok();
            if let Some(mfa) = answer.as_ref().filter(|v| v.get("code").and_then(Value::as_u64) == Some(60003)).and_then(|v| v.get("mfa"))
                && let Some(ticket) = mfa.get("ticket").and_then(Value::as_str)
            {
                let methods = mfa.get("methods").and_then(Value::as_array).into_iter().flatten().filter_map(|m| m.get("type")?.as_str().map(str::to_owned)).collect();
                return Err(MfaRequired { ticket: ticket.to_owned(), methods }.into());
            }
            let message = answer
                .and_then(|v| v.get("message")?.as_str().map(str::to_owned))
                .unwrap_or_else(|| status.to_string());
            bail!("{message} ({})", status.as_u16());
        }
        if body.is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Up to 50 messages before `before`, newest first as Discord sends them.
    pub async fn messages(&self, channel: Id, before: Option<Id>) -> Result<Vec<Message>> {
        let mut path = format!("/channels/{channel}/messages?limit=50");
        if let Some(before) = before {
            path.push_str(&format!("&before={before}"));
        }
        let value = self.send(self.request(reqwest::Method::GET, &path)).await?;
        Ok(value
            .as_array()
            .map(|list| list.iter().filter_map(Message::parse).collect())
            .unwrap_or_default())
    }

    /// Sends a message, with files as a multipart upload when there are
    /// any. `ping_reply` false is NoReplyMention.
    pub async fn send_message(
        &self,
        channel: Id,
        content: &str,
        nonce: &str,
        reply_to: Option<Id>,
        ping_reply: bool,
        silent: bool,
        files: Vec<crate::backend::Upload>,
    ) -> Result<()> {
        // SUPPRESS_NOTIFICATIONS, what "@silent" asks for.
        let mut body = json!({ "content": content, "nonce": nonce, "tts": false, "flags": if silent { 4096 } else { 0 } });
        if let Some(reply) = reply_to {
            body["message_reference"] =
                json!({ "channel_id": channel.to_string(), "message_id": reply.to_string() });
            if !ping_reply {
                body["allowed_mentions"] =
                    json!({ "parse": ["users", "roles", "everyone"], "replied_user": false });
            }
        }
        let request = self.request(reqwest::Method::POST, &format!("/channels/{channel}/messages"));
        if files.is_empty() {
            return self.send(request.json(&body)).await.map(drop);
        }
        // Large files go up on their own, read from disk as they are sent.
        let mut total = 0;
        for upload in &files {
            total += tokio::fs::metadata(&upload.path).await.map_or(0, |meta| meta.len());
        }
        if total > DIRECT_UPLOAD {
            body["attachments"] = self.upload_files(channel, &files).await?;
            return self.send(request.json(&body)).await.map(drop);
        }
        let mut attachments = Vec::new();
        let mut form = reqwest::multipart::Form::new();
        for (index, upload) in files.iter().enumerate() {
            let name = upload.sent_name();
            let bytes = tokio::fs::read(&upload.path)
                .await
                .with_context(|| t!("couldn't read {name}", name = upload.name))?;
            let mut attachment = json!({ "id": index, "filename": name });
            if !upload.description.trim().is_empty() {
                attachment["description"] = json!(upload.description.trim());
            }
            attachments.push(attachment);
            form = form.part(
                format!("files[{index}]"),
                reqwest::multipart::Part::bytes(bytes).file_name(name),
            );
        }
        body["attachments"] = Value::Array(attachments);
        form = form.text("payload_json", body.to_string());
        self.send(request.multipart(form)).await.map(drop)
    }

    /// The web client's way: ask where each file goes, send it there, and the
    /// message then names what was sent.
    async fn upload_files(&self, channel: Id, files: &[crate::backend::Upload]) -> Result<Value> {
        let mut asked = Vec::new();
        for (index, upload) in files.iter().enumerate() {
            let size = tokio::fs::metadata(&upload.path).await.with_context(|| t!("couldn't read {name}", name = upload.name))?.len();
            asked.push(json!({ "filename": upload.sent_name(), "file_size": size, "id": index.to_string() }));
        }
        let answer = self.send(self.request(reqwest::Method::POST, &format!("/channels/{channel}/attachments")).json(&json!({ "files": asked }))).await?;
        let places = answer.get("attachments").and_then(Value::as_array).context("no upload address")?;
        let mut attachments = Vec::new();
        for (index, (upload, place)) in files.iter().zip(places).enumerate() {
            let url = place.get("upload_url").and_then(Value::as_str).context("no upload address")?;
            let uploaded = place.get("upload_filename").and_then(Value::as_str).context("no upload name")?;
            let file = tokio::fs::File::open(&upload.path).await.with_context(|| t!("couldn't read {name}", name = upload.name))?;
            let size = file.metadata().await?.len();
            self.http.put(url).header("Content-Type", "application/octet-stream").header("Content-Length", size).body(file).send().await.context(t!("no connection"))?.error_for_status()?;
            let mut attachment = json!({ "id": index.to_string(), "filename": upload.sent_name(), "uploaded_filename": uploaded });
            if !upload.description.trim().is_empty() {
                attachment["description"] = json!(upload.description.trim());
            }
            attachments.push(attachment);
        }
        Ok(Value::Array(attachments))
    }

    pub async fn react(&self, channel: Id, message: Id, emoji: &str, add: bool) -> Result<()> {
        let method = if add { reqwest::Method::PUT } else { reqwest::Method::DELETE };
        let path = format!(
            "/channels/{channel}/messages/{message}/reactions/{}/@me?location=Message&type=0",
            percent_encode(emoji)
        );
        self.send(self.request(method, &path)).await.map(drop)
    }

    /// The profile card: bio, banner, mutual servers, roles here.
    pub async fn profile(&self, user: Id, guild: Option<Id>) -> Result<Profile> {
        let mut path = format!(
            "/users/{user}/profile?with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true"
        );
        if let Some(guild) = guild {
            path.push_str(&format!("&guild_id={guild}"));
        }
        let value = self.send(self.request(reqwest::Method::GET, &path)).await?;
        Profile::parse(&value).context("invalid profile")
    }

    /// Opens (or finds) the DM with `user`.
    pub async fn open_dm(&self, user: Id) -> Result<Channel> {
        let value = self
            .send(
                self.request(reqwest::Method::POST, "/users/@me/channels")
                    .json(&json!({ "recipients": [user.to_string()] })),
            )
            .await?;
        Channel::parse(&value, None, &Default::default()).context("invalid channel")
    }

    /// A forum's posts, most recently active first, with their first
    /// messages.
    pub async fn forum_posts(&self, forum: Id) -> Result<Vec<(Channel, Option<Message>)>> {
        let value = self
            .send(self.request(
                reqwest::Method::GET,
                &format!(
                    "/channels/{forum}/threads/search?archived=false&sort_by=last_message_time&sort_order=desc&limit=25&offset=0"
                ),
            ))
            .await?;
        let first: Vec<Message> = value
            .get("first_messages")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Message::parse).collect())
            .unwrap_or_default();
        Ok(value
            .get("threads")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|t| Channel::parse(t, None, &Default::default()))
                    .map(|t| {
                        let m = first.iter().find(|m| m.channel_id == t.id).cloned();
                        (t, m)
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Marks many channels read at once.
    pub async fn ack_bulk(&self, reads: &[(Id, Id)]) -> Result<()> {
        for chunk in reads.chunks(100) {
            let states: Vec<Value> = chunk
                .iter()
                .map(|(c, m)| {
                    json!({ "channel_id": c.to_string(), "message_id": m.to_string(), "read_state_type": 0 })
                })
                .collect();
            self.send(
                self.request(reqwest::Method::POST, "/read-states/ack-bulk")
                    .json(&json!({ "read_states": states })),
            )
            .await?;
        }
        Ok(())
    }

    /// Rings everyone in a DM or group for a call.
    pub async fn ring(&self, channel: Id) -> Result<()> {
        self.send(
            self.request(reqwest::Method::POST, &format!("/channels/{channel}/call/ring"))
                .json(&json!({ "recipients": null })),
        )
        .await
        .map(drop)
    }

    /// Rich presence: an application's name, for the activity's title.
    pub async fn application_name(&self, application: Id) -> Result<String> {
        let value = self
            .send(self.request(
                reqwest::Method::GET,
                &format!("/oauth2/applications/{application}/rpc"),
            ))
            .await?;
        value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("application without a name")
    }

    /// Games Discord recognises by their running process, as the client
    /// fetches them for its game detection; read typed, to keep only what
    /// the matcher needs.
    pub async fn detectable(&self) -> Result<Vec<super::detect::Detectable>> {
        let response = self
            .request(reqwest::Method::GET, "/applications/detectable")
            .send()
            .await
            .context(t!("no connection"))?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Rich presence: the application's uploaded art, name to id.
    pub async fn application_assets(&self, application: Id) -> Result<Vec<(String, String)>> {
        let value = self
            .send(self.request(
                reqwest::Method::GET,
                &format!("/oauth2/applications/{application}/assets"),
            ))
            .await?;
        Ok(value
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|a| {
                        Some((a.get("name")?.as_str()?.to_owned(), a.get("id")?.as_str()?.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Rich presence: proxies image URLs a game gives as art, as
    /// `mp:external/...` paths.
    pub async fn external_assets(&self, application: Id, urls: &[String]) -> Result<Vec<String>> {
        let value = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    &format!("/applications/{application}/external-assets"),
                )
                .json(&json!({ "urls": urls })),
            )
            .await?;
        Ok(value
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|a| a.get("external_asset_path")?.as_str().map(|p| format!("mp:{p}")))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn edit_message(&self, channel: Id, message: Id, content: &str) -> Result<()> {
        self.send(
            self.request(
                reqwest::Method::PATCH,
                &format!("/channels/{channel}/messages/{message}"),
            )
            .json(&json!({ "content": content })),
        )
        .await
        .map(drop)
    }

    pub async fn delete_message(&self, channel: Id, message: Id) -> Result<()> {
        self.send(self.request(
            reqwest::Method::DELETE,
            &format!("/channels/{channel}/messages/{message}"),
        ))
        .await
        .map(drop)
    }

    /// Marks a channel read up to `message`, on every device.
    pub async fn ack(&self, channel: Id, message: Id) -> Result<()> {
        let token = ACK_TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let answer = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    &format!("/channels/{channel}/messages/{message}/ack"),
                )
                .json(&json!({ "token": token })),
            )
            .await?;
        if let Some(token) = answer.get("token").and_then(Value::as_str) {
            *ACK_TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.to_owned());
        }
        Ok(())
    }

    pub async fn typing(&self, channel: Id) -> Result<()> {
        self.send(self.request(reqwest::Method::POST, &format!("/channels/{channel}/typing")))
            .await
            .map(drop)
    }
}

pub fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub bio: String,
    pub pronouns: String,
    pub banner: Option<String>,
    pub accent: Option<u32>,
    pub theme_colors: Option<(u32, u32)>,
    pub member: Option<Member>,
    pub guild_bio: String,
    pub joined_guild: Option<String>,
    pub mutual_guilds: Vec<Id>,
    pub mutual_friends: Option<u64>,
    pub mutual_friend_list: Vec<User>,
    /// Nicknames in mutual servers, by server.
    pub mutual_nicks: Vec<(Id, Option<String>)>,
    pub connections: Vec<(String, String, bool)>,
    /// (description, icon URL, link)
    pub badges: Vec<(String, String, Option<String>)>,
    /// The profile's user, with banner and the rest.
    pub user: User,
}

impl Profile {
    pub(crate) fn parse(value: &Value) -> Option<Self> {
        let user_value = value.get("user")?;
        let user = User::parse(user_value)?;
        let text = |v: Option<&Value>, key: &str| {
            v.and_then(|v| v.get(key))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let profile = value.get("user_profile");
        let guild_profile = value.get("guild_member_profile");
        let colors = profile
            .and_then(|p| p.get("theme_colors"))
            .and_then(Value::as_array)
            .and_then(|c| Some((c.first()?.as_u64()? as u32, c.get(1)?.as_u64()? as u32)));
        Some(Self {
            bio: text(profile, "bio"),
            pronouns: text(profile, "pronouns"),
            banner: user_value.get("banner").and_then(Value::as_str).map(|hash| {
                format!("https://cdn.discordapp.com/banners/{}/{hash}.png?size=480", user.id)
            }),
            accent: profile
                .and_then(|p| p.get("accent_color"))
                .or_else(|| user_value.get("accent_color"))
                .and_then(Value::as_u64)
                .map(|c| c as u32),
            theme_colors: colors,
            member: value.get("guild_member").filter(|m| !m.is_null()).map(Member::parse),
            guild_bio: text(guild_profile, "bio"),
            joined_guild: value
                .pointer("/guild_member/joined_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
            mutual_guilds: value
                .get("mutual_guilds")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|g| g.get("id").and_then(super::model::sf))
                        .collect()
                })
                .unwrap_or_default(),
            mutual_friends: value.get("mutual_friends_count").and_then(Value::as_u64),
            mutual_friend_list: value
                .get("mutual_friends")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(User::parse).collect())
                .unwrap_or_default(),
            mutual_nicks: value
                .get("mutual_guilds")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|g| Some((g.get("id").and_then(super::model::sf)?, g.get("nick").and_then(Value::as_str).map(str::to_owned))))
                        .collect()
                })
                .unwrap_or_default(),
            connections: value
                .get("connected_accounts")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|c| {
                            Some((
                                c.get("type")?.as_str()?.to_owned(),
                                c.get("name")?.as_str()?.to_owned(),
                                c.get("verified").and_then(Value::as_bool).unwrap_or(false),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            badges: value
                .get("badges")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|b| {
                            Some((
                                b.get("description")?.as_str()?.to_owned(),
                                format!("https://cdn.discordapp.com/badge-icons/{}.png", b.get("icon")?.as_str()?),
                                b.get("link").and_then(Value::as_str).map(str::to_owned),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            user: user.clone(),
        })
    }
}

impl Api {
    /// Moves a DM call to another voice region.
    pub async fn set_call_region(&self, channel: Id, region: &str) -> Result<()> {
        self.send(
            self.request(reqwest::Method::PATCH, &format!("/channels/{channel}/call"))
                .json(&json!({ "region": region })),
        )
        .await
        .map(drop)
    }

    /// Sets a voice channel's region (needs Manage Channels).
    pub async fn set_channel_region(&self, channel: Id, region: Option<&str>) -> Result<()> {
        self.send(
            self.request(reqwest::Method::PATCH, &format!("/channels/{channel}"))
                .json(&json!({ "rtc_region": region })),
        )
        .await
        .map(drop)
    }
}

impl Api {
    /// Declines a ringing call for us alone.
    pub async fn stop_ringing(&self, channel: Id, me: Id) -> Result<()> {
        self.send(
            self.request(reqwest::Method::POST, &format!("/channels/{channel}/call/stop-ringing"))
                .json(&json!({ "recipients": [me.to_string()] })),
        )
        .await
        .map(drop)
    }
}

/// NoTrack: when set, analytics and telemetry endpoints are never called,
/// whatever asks for them.
pub static NO_TRACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn is_tracking(path: &str) -> bool {
    ["/science", "/track", "/metrics", "/experiments", "/sentry"]
        .iter()
        .any(|p| path.starts_with(p))
}

impl Api {
    /// Mutes a guild (`None` for DMs) or one of its channels for so many
    /// seconds (-1 until unmuted), or unmutes it (`None`), as Discord's
    /// "Mute" does.
    pub async fn set_muted(&self, guild: Option<Id>, channel: Option<Id>, seconds: Option<i64>) -> Result<()> {
        let guild = guild.map_or_else(|| "@me".to_owned(), |g| g.to_string());
        let window = seconds.unwrap_or(-1);
        let end = (window > 0).then(|| (jiff::Timestamp::now() + jiff::SignedDuration::from_secs(window)).to_string());
        let config = json!({ "selected_time_window": window, "end_time": end });
        let muted = seconds.is_some();
        let body = match channel {
            Some(channel) => json!({ "channel_overrides": { channel.to_string(): { "muted": muted, "mute_config": config } } }),
            None => json!({ "muted": muted, "mute_config": config }),
        };
        self.send(
            self.request(reqwest::Method::PATCH, &format!("/users/@me/guilds/{guild}/settings"))
                .json(&body),
        )
        .await
        .map(drop)
    }

    /// Discord's notification setting for a guild or one of its channels:
    /// 0 every message, 1 only @mentions, 2 nothing, 3 (a channel) as its
    /// category or server.
    pub async fn set_notify_level(&self, guild: Option<Id>, channel: Option<Id>, level: u8) -> Result<()> {
        let guild = guild.map_or_else(|| "@me".to_owned(), |g| g.to_string());
        let body = match channel {
            Some(channel) => json!({ "channel_overrides": { channel.to_string(): { "message_notifications": level } } }),
            None => json!({ "message_notifications": level.min(2) }),
        };
        self.send(
            self.request(reqwest::Method::PATCH, &format!("/users/@me/guilds/{guild}/settings"))
                .json(&body),
        )
        .await
        .map(drop)
    }

    /// Closes a DM (it comes back with the next message).
    pub async fn close_dm(&self, channel: Id) -> Result<()> {
        self.send(self.request(reqwest::Method::DELETE, &format!("/channels/{channel}?silent=false")))
            .await
            .map(drop)
    }

    pub async fn leave_guild(&self, guild: Id) -> Result<()> {
        self.send(
            self.request(reqwest::Method::DELETE, &format!("/users/@me/guilds/{guild}"))
                .json(&json!({ "lurking": false })),
        )
        .await
        .map(drop)
    }
}

impl Api {
    /// Any API call, for the server settings pages: `method` is GET, POST,
    /// PATCH, PUT or DELETE, `path` starts after `/api/v9`.
    /// Answers an [`MfaRequired`]: the token that lets the call through,
    /// good for five minutes.
    pub async fn mfa_finish(&self, ticket: &str, mfa_type: &str, data: &str) -> Result<String> {
        let answer = self
            .send(self.request(reqwest::Method::POST, "/mfa/finish").json(&json!({ "ticket": ticket, "mfa_type": mfa_type, "data": data })))
            .await?;
        answer.get("token").and_then(Value::as_str).map(str::to_owned).context("no token")
    }

    /// A call again, with the token from [`Api::mfa_finish`].
    pub async fn raw_with_mfa(&self, method: &str, path: &str, body: Option<Value>, token: &str) -> Result<Value> {
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        let mut request = self.request(method, path).header("X-Discord-MFA-Authorization", token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        self.send(request).await
    }

    pub async fn raw(&self, method: &str, path: &str, body: Option<Value>, reason: Option<&str>) -> Result<Value> {
        let method = reqwest::Method::from_bytes(method.as_bytes())?;
        let mut request = self.request(method, path);
        if let Some(reason) = reason {
            request = request.header("X-Audit-Log-Reason", percent_encode(reason));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        self.send(request).await
    }
}

impl Api {
    pub async fn save_settings(&self, settings: &[u8]) -> Result<()> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(settings);
        self.send(
            self.request(reqwest::Method::PATCH, "/users/@me/settings-proto/1")
                .json(&json!({ "settings": encoded })),
        )
        .await
        .map(drop)
    }

    /// Friends: send a request by username.
    pub async fn add_friend(&self, username: &str) -> Result<()> {
        self.send(
            self.request(reqwest::Method::POST, "/users/@me/relationships")
                .json(&json!({ "username": username.trim().trim_start_matches('@'), "discriminator": null })),
        )
        .await
        .map(drop)
    }

    /// Accepts a request (`PUT`) or removes a friend, request or block.
    pub async fn relationship(&self, user: Id, accept_or_block: Option<u64>) -> Result<()> {
        let request = match accept_or_block {
            Some(kind) => self
                .request(reqwest::Method::PUT, &format!("/users/@me/relationships/{user}"))
                .json(&if kind == 2 { json!({ "type": 2 }) } else { json!({}) }),
            None => self.request(reqwest::Method::DELETE, &format!("/users/@me/relationships/{user}")),
        };
        self.send(request).await.map(drop)
    }
}

impl Api {
    /// A component interaction (type 3), as the client sends one when a
    /// button is pressed or a menu value chosen.
    pub async fn interact(&self, origin: &super::components::Origin, session: &str, data: Value) -> Result<()> {
        let body = json!({
            "type": 3,
            "nonce": super::model::now_snowflake().to_string(),
            "guild_id": origin.guild.map(|g| g.to_string()),
            "channel_id": origin.channel.to_string(),
            "message_flags": origin.flags,
            "message_id": origin.message.to_string(),
            "application_id": origin.application.to_string(),
            "session_id": session,
            "data": data,
        });
        self.send(self.request(reqwest::Method::POST, "/interactions").json(&body)).await.map(drop)
    }

    /// A slash command (type 2) or a request for an option's suggestions
    /// (type 4), as the client sends them from the composer.
    pub async fn command(&self, guild: Option<Id>, channel: Id, application: Id, session: &str, kind: u64, data: Value, nonce: &str) -> Result<()> {
        let body = json!({
            "type": kind,
            "nonce": nonce,
            "guild_id": guild.map(|g| g.to_string()),
            "channel_id": channel.to_string(),
            "application_id": application.to_string(),
            "session_id": session,
            "data": data,
            "analytics_location": "slash_ui",
        });
        self.send(self.request(reqwest::Method::POST, "/interactions").json(&body)).await.map(drop)
    }
}
