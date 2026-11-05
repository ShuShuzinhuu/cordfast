//! The account's own settings, as Discord keeps them for every client:
//! PreloadedUserSettings (`settings-proto/1`), field numbers from the web
//! client's descriptors. READY brings the whole message; each change Discord
//! sends (USER_SETTINGS_PROTO_UPDATE) lies over it until the next whole one.
//! A save carries the changed section whole, as the web client's does: fields
//! left out of a section may go back to their defaults on Discord's side, and
//! what this client has no name for travels along untouched.

use super::model::proto::{self, Field};

/// How a setting is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A google.protobuf wrapper (BoolValue, UInt32Value): `{ 1: value }`.
    Wrapped,
    /// A plain varint (a bool or an enum).
    Plain,
    /// A StringValue: `{ 1: text }`.
    Text,
}

/// A setting: its section of PreloadedUserSettings, its field there, how
/// it is stored and what it is when unset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub section: u32,
    pub field: u32,
    pub kind: Kind,
    pub default: u64,
}

const fn key(section: u32, field: u32, kind: Kind, default: u64) -> Key {
    Key { section, field, kind, default }
}

// Sections: 5 voice_and_video, 6 text_and_images, 7 notifications,
// 8 privacy, 11 status, 13 appearance.
pub const ALWAYS_PREVIEW_VIDEO: Key = key(5, 5, Kind::Wrapped, 0);
pub const GO_LIVE_NOTIFICATIONS: Key = key(5, 7, Kind::Wrapped, 1);
pub const DISABLE_STREAM_PREVIEWS: Key = key(5, 10, Kind::Wrapped, 0);
pub const THREAD_SPLIT_VIEW: Key = key(6, 3, Kind::Wrapped, 1);
pub const IMAGE_DESCRIPTIONS: Key = key(6, 7, Kind::Wrapped, 0);
pub const INLINE_UPLOADS: Key = key(6, 9, Kind::Wrapped, 1);
pub const INLINE_LINKS: Key = key(6, 10, Kind::Wrapped, 1);
pub const GIF_AUTO_PLAY: Key = key(6, 11, Kind::Wrapped, 1);
pub const RENDER_EMBEDS: Key = key(6, 12, Kind::Wrapped, 1);
pub const RENDER_REACTIONS: Key = key(6, 13, Kind::Wrapped, 1);
pub const ANIMATE_EMOJI: Key = key(6, 14, Kind::Wrapped, 1);
/// 0 always, 1 on interaction, 2 never.
pub const ANIMATE_STICKERS: Key = key(6, 15, Kind::Wrapped, 0);
pub const COMPACT: Key = key(6, 17, Kind::Wrapped, 0);
pub const NSFW_GUILDS_IOS: Key = key(6, 20, Kind::Wrapped, 0);
pub const CONVERT_EMOTICONS: Key = key(6, 21, Kind::Wrapped, 1);
pub const NSFW_COMMANDS: Key = key(6, 23, Kind::Wrapped, 0);
/// 1 off, 2 from non-friends, 3 from everyone (0 unset: non-friends).
pub const DM_SPAM_FILTER: Key = key(6, 27, Kind::Plain, 2);
pub const STICKERS_IN_AUTOCOMPLETE: Key = key(6, 28, Kind::Wrapped, 1);
/// "ON_CLICK", "ALWAYS" or "IF_MODERATOR".
pub const RENDER_SPOILERS: Key = key(6, 4, Kind::Text, 0);
/// 0 all messages, 1 only DMs, 2 never.
pub const REACTION_NOTIFICATIONS: Key = key(7, 7, Kind::Plain, 0);
pub const FRIEND_ONLINE_NOTIFICATIONS: Key = key(7, 12, Kind::Wrapped, 0);
pub const FRIEND_ANNIVERSARY_NOTIFICATIONS: Key = key(7, 14, Kind::Wrapped, 1);
pub const PROFILE_UPDATE_NOTIFICATIONS: Key = key(7, 16, Kind::Wrapped, 1);
pub const SERVER_EVENT_NOTIFICATIONS: Key = key(7, 22, Kind::Wrapped, 1);
pub const NOTIFY_FRIENDS_ON_PROFILE_UPDATE: Key = key(7, 24, Kind::Wrapped, 1);
pub const NOTIFY_FRIENDS_ON_COME_ONLINE: Key = key(7, 25, Kind::Wrapped, 0);
pub const NOTIFY_MEMBERS_ON_GO_LIVE: Key = key(7, 26, Kind::Wrapped, 1);
pub const ACTIVITY_JOIN_FRIENDS: Key = key(8, 1, Kind::Wrapped, 1);
pub const ACTIVITY_JOIN_VOICE: Key = key(8, 2, Kind::Wrapped, 1);
/// Direct messages from server members are refused by default.
pub const GUILDS_RESTRICTED: Key = key(8, 4, Kind::Plain, 0);
/// FriendSourceFlags: 2 friends of friends, 4 server members, 8 everyone
/// else; 14 all.
pub const FRIEND_SOURCES: Key = key(8, 11, Kind::Wrapped, 14);
/// 1 off, 2 on in large servers, 3 on.
pub const ACTIVITY_IN_GUILDS: Key = key(8, 14, Kind::Plain, 3);
pub const MESSAGE_REQUESTS_RESTRICTED: Key = key(8, 17, Kind::Wrapped, 0);
/// 1 friends only, 2 friends and small servers, 3 friends and all servers.
pub const PROFILE_VISIBILITY: Key = key(8, 32, Kind::Plain, 3);
pub const SHOW_CURRENT_GAME: Key = key(11, 3, Kind::Wrapped, 1);
pub const DEVELOPER_MODE: Key = key(13, 2, Kind::Plain, 0);
/// 0 automatic, 1 12-hour, 2 24-hour.
pub const HOUR_CYCLE: Key = key(13, 9, Kind::Plain, 0);

#[derive(Clone, Debug, Default)]
pub struct Preloaded {
    whole: Vec<u8>,
    /// Changes since, newest last.
    patches: Vec<Vec<u8>>,
}

impl Preloaded {
    pub fn new(whole: Vec<u8>) -> Self {
        Self { whole, patches: Vec::new() }
    }

    /// A change Discord sent: a whole message replaces everything.
    pub fn update(&mut self, bytes: Vec<u8>, partial: bool) {
        if partial {
            self.patches.push(bytes);
        } else {
            *self = Self::new(bytes);
        }
    }

    fn find(message: &[u8], key: Key) -> Option<Value> {
        let section = proto::fields(message).into_iter().filter(|(n, _)| *n == key.section).filter_map(|(_, v)| v.bytes()).last()?;
        let field = proto::fields(section).into_iter().filter(|(n, _)| *n == key.field).map(|(_, v)| v).last()?;
        match (key.kind, field) {
            (Kind::Plain, Field::Varint(v)) => Some(Value::Number(v)),
            (Kind::Wrapped, Field::Bytes(b)) => Some(Value::Number(proto::fields(b).into_iter().find_map(|(n, v)| match (n, v) {
                (1, Field::Varint(v)) => Some(v),
                _ => None,
            })
            .unwrap_or(0))),
            (Kind::Text, Field::Bytes(b)) => Some(Value::Text(proto::fields(b).into_iter().find_map(|(n, v)| (n == 1).then(|| v.bytes()).flatten()).map(|t| String::from_utf8_lossy(t).into_owned()).unwrap_or_default())),
            _ => None,
        }
    }

    fn value(&self, key: Key) -> Option<Value> {
        self.patches.iter().rev().find_map(|p| Self::find(p, key)).or_else(|| Self::find(&self.whole, key))
    }

    pub fn number(&self, key: Key) -> u64 {
        match self.value(key) {
            Some(Value::Number(v)) => v,
            _ => key.default,
        }
    }

    pub fn flag(&self, key: Key) -> bool {
        self.number(key) != 0
    }

    pub fn text(&self, key: Key) -> Option<String> {
        match self.value(key) {
            Some(Value::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// text_and_images.explicit_content_settings: 1 guilds, 2 friends' DMs,
    /// 3 others' DMs, each 1 show, 2 blur or 3 block.
    pub fn content_filter(&self, which: u32) -> u64 {
        let read = |message: &[u8]| {
            let section = proto::fields(message).into_iter().filter(|(n, _)| *n == 6).filter_map(|(_, v)| v.bytes()).last()?;
            let settings = proto::fields(section).into_iter().filter(|(n, _)| *n == 29).filter_map(|(_, v)| v.bytes()).last()?;
            proto::fields(settings).into_iter().filter(|(n, _)| *n == which).find_map(|(_, v)| match v {
                Field::Varint(value) => Some(value),
                _ => None,
            })
        };
        self.patches.iter().rev().find_map(|patch| read(patch)).or_else(|| read(&self.whole)).unwrap_or(2)
    }

    /// Sets one of them and returns the message that saves it.
    pub fn set_content_filter(&mut self, which: u32, value: u64) -> Vec<u8> {
        self.save(6, &length_field(29, &varint_field(which, value)))
    }

    /// voice_and_video.soundboard_settings.volume, a float from 0 to 100.
    pub fn soundboard_volume(&self) -> f32 {
        let read = |message: &[u8]| {
            let section = proto::fields(message).into_iter().filter(|(n, _)| *n == 5).filter_map(|(_, v)| v.bytes()).last()?;
            let settings = proto::fields(section).into_iter().filter(|(n, _)| *n == 9).filter_map(|(_, v)| v.bytes()).last()?;
            proto::fields(settings).into_iter().find_map(|(n, v)| match (n, v) {
                (1, Field::Fixed32(bits)) => Some(f32::from_bits(bits)),
                _ => None,
            })
        };
        self.patches.iter().rev().find_map(|patch| read(patch)).or_else(|| read(&self.whole)).unwrap_or(100.0)
    }

    /// Sets it and returns the message that saves it.
    pub fn set_soundboard_volume(&mut self, volume: f32) -> Vec<u8> {
        let mut settings = vec![(1 << 3) | 5];
        settings.extend_from_slice(&volume.to_le_bytes());
        self.save(5, &length_field(9, &settings))
    }

    /// Sets a number here and returns the message that saves it.
    pub fn set_number(&mut self, key: Key, value: u64) -> Vec<u8> {
        let inner = match key.kind {
            Kind::Plain => varint_field(key.field, value),
            _ => length_field(key.field, &varint_field(1, value)),
        };
        self.save(key.section, &inner)
    }

    pub fn set_text(&mut self, key: Key, value: &str) -> Vec<u8> {
        self.save(key.section, &length_field(key.field, &length_field(1, value.as_bytes())))
    }

    /// Saves our status (online, idle, dnd, invisible), until `expires_ms`
    /// or for good: StatusSettings { 1: status (a StringValue), 4:
    /// status_expires_at_ms (fixed64) }.
    pub fn set_status(&mut self, status: &str, expires_ms: Option<u64>) -> Vec<u8> {
        let mut inner = length_field(1, &length_field(1, status.as_bytes()));
        inner.extend(fixed64_field(4, expires_ms.unwrap_or(0)));
        self.save(STATUS_SECTION, &inner)
    }

    /// Sets (or clears, with `None`) the custom status:
    /// StatusSettings.custom_status (2) → CustomStatus { 1: text, 4:
    /// expires_at_ms }. It replaces the old one, whose emoji would otherwise
    /// merge into the new text.
    pub fn set_custom_status(&mut self, text: Option<&str>, expires_ms: Option<u64>) -> Vec<u8> {
        let mut custom = length_field(1, text.unwrap_or_default().as_bytes());
        // An empty status that ended a moment ago clears it everywhere.
        custom.extend(fixed64_field(4, if text.is_some() { expires_ms.unwrap_or(0) } else { 1 }));
        let mut section = without(&self.section(STATUS_SECTION), 2);
        section.extend(length_field(2, &custom));
        self.replace(STATUS_SECTION, section)
    }

    /// A section as it stands: every copy of it, oldest first, which
    /// protobuf reads as one message with the newest values winning.
    fn section(&self, section: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for message in std::iter::once(&self.whole).chain(&self.patches) {
            for (_, value) in proto::fields(message).into_iter().filter(|(n, _)| *n == section) {
                out.extend_from_slice(value.bytes().unwrap_or_default());
            }
        }
        out
    }

    fn save(&mut self, section: u32, change: &[u8]) -> Vec<u8> {
        let mut whole = self.section(section);
        whole.extend_from_slice(change);
        self.replace(section, whole)
    }

    /// Makes `body` the section here and returns the message that saves it.
    fn replace(&mut self, section: u32, body: Vec<u8>) -> Vec<u8> {
        let message = length_field(section, &body);
        self.whole = without(&self.whole, section);
        self.whole.extend_from_slice(&message);
        for patch in &mut self.patches {
            *patch = without(patch, section);
        }
        self.patches.retain(|patch| !patch.is_empty());
        message
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Number(u64),
    Text(String),
}

/// PreloadedUserSettings.status.
const STATUS_SECTION: u32 = 11;

/// A message without a field, every other byte as it came (fields this
/// client has no name for included). Stops copying at anything malformed.
fn without(message: &[u8], field: u32) -> Vec<u8> {
    fn tag(buf: &[u8], pos: &mut usize) -> Option<u64> {
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
    let mut out = Vec::with_capacity(message.len());
    let mut pos = 0;
    while pos < message.len() {
        let start = pos;
        let Some(key) = tag(message, &mut pos) else { break };
        let size = match key & 7 {
            0 => tag(message, &mut pos).map(|_| 0),
            1 => Some(8),
            2 => tag(message, &mut pos),
            5 => Some(4),
            _ => None,
        };
        let Some(end) = size.and_then(|size| pos.checked_add(size as usize)).filter(|end| *end <= message.len()) else { break };
        pos = end;
        if key >> 3 != u64::from(field) {
            out.extend_from_slice(&message[start..end]);
        }
    }
    out
}

fn varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn varint_field(field: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    varint(u64::from(field) << 3, &mut out);
    varint(value, &mut out);
    out
}

fn fixed64_field(field: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    varint(u64::from(field) << 3 | 1, &mut out);
    out.extend_from_slice(&value.to_le_bytes());
    out
}

fn length_field(field: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    varint(u64::from(field) << 3 | 2, &mut out);
    varint(body.len() as u64, &mut out);
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved change reads back over READY's value, and a whole message
    /// from Discord replaces both.
    #[test]
    fn changes_lie_over_the_whole() {
        // text_and_images { render_embeds: BoolValue(false) }
        let whole = length_field(6, &length_field(12, &varint_field(1, 0)));
        let mut settings = Preloaded::new(whole);
        assert!(!settings.flag(RENDER_EMBEDS));
        assert!(settings.flag(RENDER_REACTIONS), "unset takes the default");
        let patch = settings.set_number(RENDER_EMBEDS, 1);
        // The section whole: the old value, then the new, which wins.
        assert_eq!(patch, [0x32, 8, 0x62, 2, 0x08, 0, 0x62, 2, 0x08, 1]);
        assert!(settings.flag(RENDER_EMBEDS));
        settings.set_text(RENDER_SPOILERS, "ALWAYS");
        assert_eq!(settings.text(RENDER_SPOILERS).as_deref(), Some("ALWAYS"));
        settings.update(Vec::new(), false);
        assert!(settings.flag(RENDER_EMBEDS));
        assert_eq!(settings.text(RENDER_SPOILERS), None);
    }

    /// A save keeps the section's other fields, ones this client does not
    /// know included, and leaves other sections out.
    #[test]
    fn a_save_carries_the_whole_section() {
        // text_and_images { 99: 7 (unknown) }, privacy { 1: 1 }
        let mut whole = length_field(6, &varint_field(99, 7));
        whole.extend(length_field(2, &varint_field(1, 1)));
        let mut settings = Preloaded::new(whole);
        let saved = settings.set_number(RENDER_EMBEDS, 0);
        let fields = proto::fields(&saved);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, 6);
        let section = proto::fields(fields[0].1.bytes().unwrap());
        assert!(section.iter().any(|(n, _)| *n == 99), "unknown field kept");
        assert!(section.iter().any(|(n, _)| *n == RENDER_EMBEDS.field));
        assert!(!settings.flag(RENDER_EMBEDS));
        // A second save still has both, without growing other sections.
        let saved = settings.set_number(RENDER_REACTIONS, 0);
        let section = proto::fields(proto::fields(&saved)[0].1.bytes().unwrap()).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert!(section.contains(&99) && section.contains(&RENDER_EMBEDS.field) && section.contains(&RENDER_REACTIONS.field));
    }

    /// A new custom status replaces the old, emoji and all, and leaves the
    /// status beside it alone.
    #[test]
    fn a_custom_status_replaces_the_old_one() {
        let mut settings = Preloaded::new(Vec::new());
        settings.set_status("dnd", None);
        // custom_status { 3: emoji name }
        let mut old = settings.section(STATUS_SECTION);
        old.extend(length_field(2, &length_field(3, b"wave")));
        settings.replace(STATUS_SECTION, old);
        let saved = settings.set_custom_status(Some("hi"), None);
        let fields = proto::fields(&saved);
        let section = proto::fields(fields[0].1.bytes().unwrap());
        assert!(section.iter().any(|(n, _)| *n == 1), "the status stays");
        let customs: Vec<_> = section.iter().filter(|(n, _)| *n == 2).collect();
        assert_eq!(customs.len(), 1);
        assert!(!proto::fields(customs[0].1.bytes().unwrap()).iter().any(|(n, _)| *n == 3), "the old emoji goes");
    }
}
