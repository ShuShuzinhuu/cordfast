//! Plugins: optional behaviours, each switched on or off in Settings →
//! Plugins. Vencord's plugins are JavaScript that patches Discord's web
//! client and cannot run in a native app; these are the popular ones
//! rewritten natively, under the same names so they are easy to find.
//! NoTrack has less to do here than in Vencord: CordFast sends no analytics
//! of its own, and the flag also refuses any tracking endpoint in `rest`.
//!
//! Vencord's required plugins come first and stay on, as there, so the page
//! reads as Vencord's. Natively, Settings is this page, WebContextMenus the
//! link and image menus, DisableDeepLinks the discord.com links that open
//! here; ConcatenatedComponentExtractor, FixDiscordCss and SupportHelper
//! only patch the web bundle, so they have nothing to do in a native
//! client. Every other plugin starts off.

use crate::discord::model::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plugin {
    ConcatenatedComponentExtractor,
    DisableDeepLinks,
    FixDiscordCss,
    NoTrack,
    Settings,
    SupportHelper,
    WebContextMenus,
    MessageLogger,
    SilentTyping,
    NoReplyMention,
    ClearUrls,
    ShowHiddenChannels,
    ShowMeYourName,
    MessageClickActions,
    ViewRaw,
    CopyMessageLink,
    ReverseImageSearch,
    RelationshipNotifier,
    NoBlockedMessages,
    TypingIndicator,
    FriendsSince,
    BetterFolders,
    CallTimer,
    VolumeBooster,
}

pub struct Info {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub default: bool,
    /// Always on, its switch locked (Vencord's "Required Plugins").
    pub required: bool,
}

pub const ALL: &[Plugin] = &[
    Plugin::ConcatenatedComponentExtractor,
    Plugin::DisableDeepLinks,
    Plugin::FixDiscordCss,
    Plugin::NoTrack,
    Plugin::Settings,
    Plugin::SupportHelper,
    Plugin::WebContextMenus,
    Plugin::MessageLogger,
    Plugin::SilentTyping,
    Plugin::NoReplyMention,
    Plugin::ClearUrls,
    Plugin::ShowHiddenChannels,
    Plugin::ShowMeYourName,
    Plugin::MessageClickActions,
    Plugin::ViewRaw,
    Plugin::CopyMessageLink,
    Plugin::ReverseImageSearch,
    Plugin::RelationshipNotifier,
    Plugin::NoBlockedMessages,
    Plugin::TypingIndicator,
    Plugin::FriendsSince,
    Plugin::BetterFolders,
    Plugin::CallTimer,
    Plugin::VolumeBooster,
];

impl Plugin {
    pub fn info(self) -> Info {
        let (id, name, description, default) = match self {
            // Vencord's own descriptions.
            Self::ConcatenatedComponentExtractor => ("concatenated-component-extractor", "ConcatenatedComponentExtractor", n!("Extract components that have been concatenated by the bundler"), true),
            Self::DisableDeepLinks => ("disable-deep-links", "DisableDeepLinks", n!("Disables Discord's stupid deep linking feature which tries to force you to use their Desktop App"), true),
            Self::FixDiscordCss => ("fix-discord-css", "FixDiscordCss", n!("Fixes Discord's css lag"), true),
            Self::Settings => ("settings", "Settings", n!("Adds Settings UI and debug info"), true),
            Self::SupportHelper => ("support-helper", "SupportHelper", n!("Helps us provide support to you"), true),
            Self::WebContextMenus => ("web-context-menus", "WebContextMenus", n!("Re-adds context menus missing in the web version of Discord: Links & Images (Copy/Open Link/Image), Text Area (Copy, Cut, Paste, SpellCheck)"), true),
            Self::NoTrack => (
                "no-track",
                "NoTrack",
                n!("Disable Discord's tracking (analytics/'science'), metrics and Sentry crash reporting"),
                false,
            ),
            Self::MessageLogger => (
                "message-logger",
                "MessageLogger",
                n!("Keeps deleted messages (in red) and the earlier text of edited ones."),
                false,
            ),
            Self::SilentTyping => (
                "silent-typing",
                "SilentTyping",
                n!("Doesn't tell others you're typing."),
                false,
            ),
            Self::NoReplyMention => (
                "no-reply-mention",
                "NoReplyMention",
                n!("Replies don't mention (ping) the message's author."),
                false,
            ),
            Self::ClearUrls => (
                "clear-urls",
                "ClearURLs",
                n!("Removes tracking parameters (utm_, fbclid, si=...) from the links you send."),
                false,
            ),
            Self::ShowHiddenChannels => (
                "show-hidden-channels",
                "ShowHiddenChannels",
                n!("Shows the channels you can't see, with a lock."),
                false,
            ),
            Self::ShowMeYourName => (
                "show-me-your-name",
                "ShowMeYourName",
                n!("Shows the username beside the nickname in messages."),
                false,
            ),
            Self::MessageClickActions => (
                "message-click-actions",
                "MessageClickActions",
                n!("Double click: edits your message or replies to someone else's."),
                false,
            ),
            Self::ViewRaw => (
                "view-raw",
                "ViewRaw",
                n!("\"View Raw\" in the message menu, with the original markup."),
                false,
            ),
            Self::CopyMessageLink => (
                "copy-message-link",
                "CopyMessageLink",
                n!("\"Copy Message Link\" in the message menu."),
                false,
            ),
            Self::ReverseImageSearch => (
                "reverse-image-search",
                "ReverseImageSearch",
                n!("Searches an image on Google Lens from the right-click menu."),
                false,
            ),
            Self::RelationshipNotifier => (
                "relationship-notifier",
                "RelationshipNotifier",
                n!("Tells you when someone unfriends you or you leave a server."),
                false,
            ),
            Self::NoBlockedMessages => (
                "no-blocked-messages",
                "NoBlockedMessages",
                n!("Hides the messages of people you blocked completely."),
                false,
            ),
            Self::TypingIndicator => (
                "typing-indicator",
                "TypingIndicator",
                n!("Shows who is typing beside the channel in the list."),
                false,
            ),
            Self::FriendsSince => (
                "friends-since",
                "FriendsSince",
                n!("Shows on the profile since when you've been friends."),
                false,
            ),
            Self::BetterFolders => (
                "better-folders",
                "BetterFolders",
                n!("Opening a folder closes the others."),
                false,
            ),
            Self::CallTimer => (
                "call-timer",
                "CallTimer",
                n!("Shows how long you've been in the call."),
                false,
            ),
            Self::VolumeBooster => (
                "volume-booster",
                "VolumeBooster",
                n!("Allows user volumes up to 400% (instead of 200%)."),
                false,
            ),
        };
        let required = matches!(self, Self::ConcatenatedComponentExtractor | Self::DisableDeepLinks | Self::FixDiscordCss | Self::NoTrack | Self::Settings | Self::SupportHelper | Self::WebContextMenus);
        Info {
            id,
            name,
            description,
            default: default || required,
            required,
        }
    }
}

/// Query parameters ClearURLs removes.
const TRACKERS: &[&str] = &[
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content", "utm_id",
    "fbclid", "gclid", "dclid", "msclkid", "igshid", "igsh", "mc_eid", "mc_cid",
    "si", "feature", "ref_src", "ref_url", "_hsenc", "_hsmi", "yclid", "twclid",
    "s_cid", "spm", "pd_rd_r", "pd_rd_w", "pd_rd_wg", "pf_rd_p", "pf_rd_r",
];

/// ClearURLs: strips tracking parameters from every link in `text`.
pub fn clear_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (link, piece) in crate::text::links(text) {
        if !link {
            out.push_str(piece);
            continue;
        }
        let (base, fragment) = piece.split_once('#').map_or((piece, None), |(b, f)| (b, Some(f)));
        match base.split_once('?') {
            Some((path, query)) => {
                let kept: Vec<&str> = query
                    .split('&')
                    .filter(|pair| {
                        let key = pair.split('=').next().unwrap_or_default();
                        !TRACKERS.contains(&key) && !key.starts_with("utm_")
                    })
                    .collect();
                out.push_str(path);
                if !kept.is_empty() {
                    out.push('?');
                    out.push_str(&kept.join("&"));
                }
            }
            None => out.push_str(base),
        }
        if let Some(fragment) = fragment {
            out.push('#');
            out.push_str(fragment);
        }
    }
    out
}

/// Whether MessageLogger should keep `message` though it was deleted.
pub fn keeps(message: &Message) -> bool {
    !message.pending
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_trackers() {
        assert_eq!(
            clear_urls("look https://youtu.be/abc?si=XYZ&t=10 and https://a.com/?utm_source=x"),
            "look https://youtu.be/abc?t=10 and https://a.com/"
        );
    }
}
