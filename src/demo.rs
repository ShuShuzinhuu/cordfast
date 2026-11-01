//! A session without an account, for screenshots and layout checks where
//! there is no Discord login (Linux, CI, a cloud machine):
//! `CORDFAST_DEMO=tests/fixtures/demo.json`. READY, profiles and message
//! histories come from the file through the same parsers the gateway and
//! REST use, so what shows is what a real session of that shape shows.
//! With `CORDFAST_PREVIEW` it opens straight on a screen.

use serde_json::Value;

use crate::backend::{Event, Events};
use crate::discord::gateway::Dispatch;
use crate::discord::model::{Message, Ready};
use crate::discord::rest::Profile;

pub struct Demo(Value);

impl Demo {
    pub fn load() -> Option<Self> {
        let path = std::env::var_os("CORDFAST_DEMO")?;
        match std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|t| Ok(serde_json::from_str(&t)?)) {
            Ok(value) => Some(Self(value)),
            Err(error) => {
                log::warn!("demo {}: {error}", path.to_string_lossy());
                None
            }
        }
    }

    /// Logs in: READY, as the gateway sends it.
    pub fn start(&self, events: &Events) {
        let Some(ready) = self.0.get("ready").and_then(Ready::parse) else {
            log::warn!("demo: invalid READY");
            return;
        };
        events.send(Event::Gateway(Dispatch::Ready(Box::new(ready))));
        // Statuses and what people are doing, as the gateway sends them after READY.
        let presences = self.0.get("ready").map(crate::discord::gateway::Presence::all_in).unwrap_or_default();
        if !presences.is_empty() {
            events.send(Event::Gateway(Dispatch::Presences(presences)));
        }
        events.send(Event::Gateway(Dispatch::Connected));
        // Someone typing, for the line under the box.
        for typing in self.0.get("typing").and_then(Value::as_array).into_iter().flatten() {
            let id = |k: &str| typing.get(k).and_then(crate::discord::model::sf);
            if let (Some(channel), Some(user)) = (id("channel"), id("user")) {
                events.send(Event::Gateway(Dispatch::Typing { channel, user }));
            }
        }
    }

    /// Answers what the file has; everything else goes to the backend.
    pub fn answer(&self, command: &crate::backend::Command, events: &Events) -> bool {
        use crate::backend::Command;
        match command {
            Command::LoadMessages { channel, before: None } => {
                let messages: Vec<Message> = self
                    .0
                    .pointer(&format!("/messages/{channel}"))
                    .and_then(Value::as_array)
                    .map(|list| list.iter().filter_map(Message::parse).collect())
                    .unwrap_or_default();
                events.send(Event::Messages { channel: *channel, before: None, messages });
                true
            }
            Command::LoadMessages { channel, before: Some(_) } => {
                events.send(Event::Messages { channel: *channel, before: Some(0), messages: Vec::new() });
                true
            }
            // The member list, as GUILD_MEMBER_LIST_UPDATE brings it.
            Command::MemberList { .. } => {
                if let Some(update) = self.0.get("member_list").and_then(crate::discord::gateway::MemberListUpdate::parse) {
                    events.send(Event::Gateway(Dispatch::MemberList(update)));
                }
                true
            }
            // The server settings' requests, from the file; changes succeed.
            Command::Api { key, .. } if key.starts_with("do:") || self.0.pointer(&format!("/api/{key}")).is_some() => {
                let value = self.0.pointer(&format!("/api/{key}")).cloned().unwrap_or_else(|| serde_json::json!({}));
                events.send(Event::Api { key: key.clone(), result: Ok(value) });
                true
            }
            // An app's suggestions, as the gateway brings them: the file's
            // list, filtered by what is typed in the focused option.
            Command::SlashCommand { kind: 4, data, nonce, .. } => {
                let typed = data.pointer("/options").and_then(Value::as_array).into_iter().flatten().find(|o| o.get("focused").is_some()).and_then(|o| o.get("value")).and_then(Value::as_str).unwrap_or_default().to_lowercase();
                let choices = self
                    .0
                    .pointer("/autocomplete")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|c| c.to_lowercase().contains(&typed))
                    .map(|c| (c.to_owned(), Value::String(c.to_owned())))
                    .collect();
                events.send(Event::Gateway(Dispatch::Autocomplete { nonce: nonce.clone(), choices }));
                true
            }
            Command::SlashCommand { .. } => true,
            Command::Profile { user, .. } => {
                let profile = self
                    .0
                    .pointer(&format!("/profiles/{user}"))
                    .and_then(Profile::parse)
                    .map(Box::new)
                    .ok_or_else(|| "no profile in the demo".to_owned());
                events.send(Event::Profile { user: *user, profile });
                true
            }
            _ => false,
        }
    }
}
