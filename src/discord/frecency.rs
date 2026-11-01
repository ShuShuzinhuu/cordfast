//! What the expression picker shows first: Discord's FrecencyUserSettings
//! (`GET /users/@me/settings-proto/2`), read with its own sums. The
//! protobuf, from the web client's descriptors: 2 favorite_gifs, 3
//! favorite_stickers, 4 sticker_frecency, 5 favorite_emojis, 6
//! emoji_frecency; a frecency is a map from the emoji's id or name (the
//! sticker's id, a fixed64) to FrecencyItem { 1 total_uses, 2 recent_uses
//! (packed uint64 milliseconds), 3 frecency, 4 score }.

use super::model::Id;
use super::model::proto::{Field, fields, varints};

/// The picker's lists, most used first.
#[derive(Clone, Debug, Default)]
pub struct Frecency {
    /// Custom emoji ids and Unicode emoji names, as Discord keys them.
    pub emojis: Vec<String>,
    pub favorite_emojis: Vec<String>,
    pub stickers: Vec<Id>,
    pub favorite_stickers: Vec<Id>,
}

/// How many the picker keeps (the bundle's `numFrequentlyItems`).
const KEEP: usize = 32;

impl Frecency {
    pub fn decode(bytes: &[u8], now_ms: u64) -> Self {
        let mut out = Self::default();
        for (field, value) in fields(bytes) {
            match (field, value) {
                (3, Field::Bytes(b)) => out.favorite_stickers = fields(b).into_iter().filter(|(f, _)| *f == 1).flat_map(|(_, v)| v.fixed64s()).collect(),
                (4, Field::Bytes(b)) => out.stickers = ranked(b, now_ms, |key| match key {
                    Field::Fixed64(id) => Some(id.to_string()),
                    _ => None,
                })
                .into_iter()
                .filter_map(|k| k.parse().ok())
                .collect(),
                (5, Field::Bytes(b)) => out.favorite_emojis = fields(b).into_iter().filter_map(|(f, v)| if let (1, Field::Bytes(s)) = (f, v) { std::str::from_utf8(s).ok().map(str::to_owned) } else { None }).collect(),
                (6, Field::Bytes(b)) => out.emojis = ranked(b, now_ms, |key| match key {
                    Field::Bytes(s) => std::str::from_utf8(s).ok().map(str::to_owned),
                    _ => None,
                }),
                _ => {}
            }
        }
        out
    }

    /// Counts a use now, as the client does before its next save: the
    /// emoji moves to the front of this session's list.
    pub fn used_emoji(&mut self, key: String) {
        self.emojis.retain(|k| *k != key);
        self.emojis.insert(0, key);
        self.emojis.truncate(KEEP);
    }

    pub fn used_sticker(&mut self, id: Id) {
        self.stickers.retain(|k| *k != id);
        self.stickers.insert(0, id);
        self.stickers.truncate(KEEP);
    }
}

/// A frecency map's keys, best first, scored as the web client does: each
/// of the last ten uses weighs by its age in days (100 up to 3 days, 70 up
/// to 15, 50 up to 30, 30 up to 45, 10 up to 80, then 1), and the frecency
/// is the total uses times the mean weight, rounded up.
fn ranked(map: &[u8], now_ms: u64, key_of: impl Fn(Field<'_>) -> Option<String>) -> Vec<String> {
    let mut scored: Vec<(String, u64)> = Vec::new();
    for (field, entry) in fields(map) {
        let (1, Field::Bytes(entry)) = (field, entry) else { continue };
        let mut key = None;
        let mut item: &[u8] = &[];
        for (f, v) in fields(entry) {
            match (f, v) {
                (1, v) => key = key_of(v),
                (2, Field::Bytes(b)) => item = b,
                _ => {}
            }
        }
        let Some(key) = key else { continue };
        let mut total = 0;
        let mut recent: Vec<u64> = Vec::new();
        for (f, v) in fields(item) {
            match (f, v) {
                (1, Field::Varint(n)) => total = n,
                (2, Field::Bytes(packed)) => recent.extend(varints(packed)),
                (2, Field::Varint(n)) => recent.push(n),
                _ => {}
            }
        }
        recent.sort_unstable();
        let samples = &recent[recent.len().saturating_sub(10)..];
        let score: u64 = samples.iter().map(|t| weight(now_ms.saturating_sub(*t) / 86_400_000)).sum();
        if score == 0 || samples.is_empty() {
            continue;
        }
        let frecency = (total * score).div_ceil(samples.len() as u64);
        scored.push((key, frecency));
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().take(KEEP).map(|(k, _)| k).collect()
}

fn weight(days: u64) -> u64 {
    match days {
        0..=3 => 100,
        4..=15 => 70,
        16..=30 => 50,
        31..=45 => 30,
        46..=80 => 10,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(field: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut put = |mut v: u64| loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        };
        put(u64::from(field) << 3 | 2);
        put(body.len() as u64);
        out.extend_from_slice(body);
        out
    }

    fn uses(total: u8, times: &[u64]) -> Vec<u8> {
        let mut packed = Vec::new();
        for t in times {
            let mut v = *t;
            loop {
                let b = (v & 0x7f) as u8;
                v >>= 7;
                if v == 0 {
                    packed.push(b);
                    break;
                }
                packed.push(b | 0x80);
            }
        }
        let mut item = vec![0x08, total];
        item.extend(message(2, &packed));
        item
    }

    /// A recent emoji beats one used more but long ago; favourites keep
    /// their order.
    #[test]
    fn ranks_emoji_as_the_client_does() {
        let day = 86_400_000u64;
        let now = 1_000 * day;
        let mut map = Vec::new();
        let mut entry = message(1, b"thumbsup");
        entry.extend(message(2, &uses(2, &[now - day, now])));
        map.extend(message(1, &entry));
        let mut entry = message(1, b"123456");
        entry.extend(message(2, &uses(9, &[now - 200 * day])));
        map.extend(message(1, &entry));
        let mut settings = message(6, &map);
        let mut favorites = message(1, b"fire");
        favorites.extend(message(1, b"heart"));
        settings.extend(message(5, &favorites));
        let frecency = Frecency::decode(&settings, now);
        assert_eq!(frecency.emojis, ["thumbsup", "123456"]);
        assert_eq!(frecency.favorite_emojis, ["fire", "heart"]);
    }
}
