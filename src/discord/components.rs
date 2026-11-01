//! Message components: the action rows of buttons and menus bots attach,
//! and Components V2 (message flag 1 << 15), where the whole message is a tree of
//! containers, sections, text, media galleries, separators and files.

use serde_json::Value;

use super::model::{Emoji, Id, sf};

#[derive(Clone, Debug)]
pub struct Media {
    pub url: String,
    pub proxy_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub spoiler: bool,
    pub description: Option<String>,
}

impl Media {
    fn parse(item: &Value, media_key: &str) -> Option<Self> {
        let media = item.get(media_key)?;
        Some(Self {
            url: media.get("url")?.as_str()?.to_owned(),
            proxy_url: media.get("proxy_url").and_then(Value::as_str).map(str::to_owned),
            width: media.get("width").and_then(Value::as_u64).map(|w| w as u32),
            height: media.get("height").and_then(Value::as_u64).map(|h| h as u32),
            spoiler: item.get("spoiler").and_then(Value::as_bool).unwrap_or(false),
            description: item.get("description").and_then(Value::as_str).map(str::to_owned),
        })
    }

    /// The URL to draw: the media proxy's when there is one.
    pub fn shown(&self) -> &str {
        self.proxy_url.as_deref().unwrap_or(&self.url)
    }
}

#[derive(Clone, Debug)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    pub description: Option<String>,
    pub emoji: Option<Emoji>,
    pub default: bool,
}

#[derive(Clone, Debug)]
pub enum Component {
    Row(Vec<Component>),
    Button {
        /// 1 primary, 2 secondary, 3 success, 4 danger, 5 link, 6 premium.
        style: u64,
        label: Option<String>,
        emoji: Option<Emoji>,
        custom_id: Option<String>,
        url: Option<String>,
        disabled: bool,
    },
    Select {
        kind: u64,
        custom_id: String,
        placeholder: Option<String>,
        options: Vec<SelectOption>,
        max: u64,
        disabled: bool,
    },
    Section { texts: Vec<String>, accessory: Option<Box<Component>> },
    Text(String),
    Thumbnail(Media),
    Gallery(Vec<Media>),
    File { url: String, name: String },
    Separator { divider: bool, large: bool },
    Container { accent: Option<u32>, children: Vec<Component> },
}

impl Component {
    pub fn parse(value: &Value) -> Option<Self> {
        let list = |key: &str| -> Vec<Component> {
            value
                .get(key)
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(Component::parse).collect())
                .unwrap_or_default()
        };
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
        Some(match value.get("type")?.as_u64()? {
            1 => Component::Row(list("components")),
            2 => Component::Button {
                style: value.get("style").and_then(Value::as_u64).unwrap_or(2),
                label: text("label"),
                emoji: value.get("emoji").and_then(Emoji::parse),
                custom_id: text("custom_id"),
                url: text("url"),
                disabled: flag("disabled"),
            },
            kind @ (3 | 5..=8) => Component::Select {
                kind,
                custom_id: text("custom_id").unwrap_or_default(),
                placeholder: text("placeholder"),
                options: value
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|l| {
                        l.iter()
                            .filter_map(|o| {
                                Some(SelectOption {
                                    label: o.get("label")?.as_str()?.to_owned(),
                                    value: o.get("value")?.as_str()?.to_owned(),
                                    description: o.get("description").and_then(Value::as_str).map(str::to_owned),
                                    emoji: o.get("emoji").and_then(Emoji::parse),
                                    default: o.get("default").and_then(Value::as_bool).unwrap_or(false),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                max: value.get("max_values").and_then(Value::as_u64).unwrap_or(1),
                disabled: flag("disabled"),
            },
            9 => Component::Section {
                texts: value
                    .get("components")
                    .and_then(Value::as_array)
                    .map(|l| l.iter().filter_map(|c| c.get("content")?.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
                accessory: value.get("accessory").and_then(Component::parse).map(Box::new),
            },
            10 => Component::Text(text("content").unwrap_or_default()),
            11 => Component::Thumbnail(Media::parse(value, "media")?),
            12 => Component::Gallery(
                value
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|l| l.iter().filter_map(|i| Media::parse(i, "media")).collect())
                    .unwrap_or_default(),
            ),
            13 => {
                let file = value.get("file")?;
                let url = file.get("url")?.as_str()?.to_owned();
                Component::File {
                    name: value
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| url.rsplit('/').next().unwrap_or("file").to_owned()),
                    url,
                }
            }
            14 => Component::Separator {
                divider: value.get("divider").and_then(Value::as_bool).unwrap_or(true),
                large: value.get("spacing").and_then(Value::as_u64) == Some(2),
            },
            17 => Component::Container {
                accent: value.get("accent_color").and_then(Value::as_u64).map(|c| c as u32),
                children: list("components"),
            },
            _ => return None,
        })
    }

    pub fn parse_all(value: &Value) -> Vec<Self> {
        value
            .get("components")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Component::parse).collect())
            .unwrap_or_default()
    }
}

/// What the interactions endpoint needs about the message a component sits on.
#[derive(Clone, Debug)]
pub struct Origin {
    pub channel: Id,
    pub guild: Option<Id>,
    pub message: Id,
    pub application: Id,
    pub flags: u64,
}

impl Origin {
    pub fn parse(value: &Value, channel: Id, guild: Option<Id>, message: Id) -> Option<Self> {
        let application = value
            .get("application_id")
            .and_then(sf)
            .or_else(|| value.pointer("/interaction_metadata/authorizing_integration_owners/0").and_then(sf))
            .or_else(|| value.pointer("/author/id").and_then(sf))?;
        Some(Self {
            channel,
            guild,
            message,
            application,
            flags: value.get("flags").and_then(Value::as_u64).unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_v2_player() {
        let message = json!({ "components": [{
            "type": 17, "accent_color": 16711935, "components": [
                { "type": 10, "content": "NOW PLAYING" },
                { "type": 12, "items": [{ "media": { "url": "https://x/a.png", "width": 300, "height": 300 } }] },
                { "type": 14, "divider": true, "spacing": 1 },
                { "type": 1, "components": [
                    { "type": 2, "style": 2, "custom_id": "pause", "emoji": { "name": "⏸" } },
                    { "type": 2, "style": 5, "url": "https://y", "label": "Abrir" }
                ]}
            ]
        }]});
        let parsed = Component::parse_all(&message);
        let Component::Container { accent, children, .. } = &parsed[0] else { panic!() };
        assert_eq!(*accent, Some(0xff00ff));
        assert_eq!(children.len(), 4);
        assert!(matches!(&children[3], Component::Row(buttons) if buttons.len() == 2));
    }
}
