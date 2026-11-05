//! User settings in Discord's redesigned layout (the bundle's settings
//! tree): the sidebar with its search, the profile card on top, then
//! General, Billing, Experience, Games & Apps and the rest, each page
//! made of categories with Discord's own settings in their order. The
//! account's settings (privacy, messages, notifications, activity) are the
//! PreloadedUserSettings every client shares; CordFast's own (themes,
//! voice processing, the window) are kept here. Billing pages only show
//! what the account has: there is no payment flow.

use egui::{Color32, Frame, Key, Margin, Rect, RichText, Sense, Stroke, Vec2, vec2};
use serde_json::Value;

use super::form::{self, Kind};
use crate::app::{Action, App, SettingsTab};
use crate::discord::preloaded::{self as pre, Key as PreKey};
use crate::icons::Icon;
use crate::plugins;
use crate::theme;

/// A sidebar section: its name (none for those Discord leaves unnamed)
/// and its pages with their icons.
type Section = (Option<&'static str>, &'static [(SettingsTab, Icon, &'static str)]);

const SECTIONS: &[Section] = &[
    (None, &[
        (SettingsTab::Account, Icon::User, n!("Account")),
        (SettingsTab::DataPrivacy, Icon::Privacy, n!("Data & Privacy")),
        (SettingsTab::MessagingPermissions, Icon::Messaging, n!("Messaging Permissions")),
        (SettingsTab::Notifications, Icon::Bell, n!("Notifications")),
        (SettingsTab::Clips, Icon::Clips, n!("Clips")),
    ]),
    (Some(n!("Billing")), &[
        (SettingsTab::Nitro, Icon::Nitro, n!("Nitro")),
        (SettingsTab::ServerBoost, Icon::Boost, n!("Server Boost")),
        (SettingsTab::Subscriptions, Icon::Subscriptions, n!("Subscriptions")),
        (SettingsTab::GiftInventory, Icon::Gift, n!("Gift Inventory")),
        (SettingsTab::Billing, Icon::CreditCard, n!("Billing")),
    ]),
    (Some(n!("Experience")), &[
        (SettingsTab::Voice, Icon::Microphone, n!("Voice & Video")),
        (SettingsTab::Appearance, Icon::Palette, n!("Appearance")),
        (SettingsTab::Accessibility, Icon::Accessibility, n!("Accessibility")),
        (SettingsTab::Streamer, Icon::Video, n!("Streamer Mode")),
        (SettingsTab::System, Icon::System, n!("System")),
        (SettingsTab::Language, Icon::Language, n!("Language & Time")),
    ]),
    (Some(n!("Games & Apps")), &[
        (SettingsTab::RegisteredGames, Icon::Controller, n!("Registered Games")),
        (SettingsTab::ActivityPrivacy, Icon::ActivityPrivacy, n!("Activity Privacy")),
        (SettingsTab::Overlay, Icon::Overlay, n!("Game Overlay")),
        (SettingsTab::ConnectedApps, Icon::Link, n!("Connected Apps")),
    ]),
    (Some("CordFast"), &[
        (SettingsTab::Plugins, Icon::Puzzle, n!("Plugins")),
        (SettingsTab::About, Icon::Info, n!("About CordFast")),
    ]),
    (None, &[(SettingsTab::Developer, Icon::Developer, n!("Developer"))]),
];

pub fn show(app: &mut App, ui: &mut egui::Ui, tab: SettingsTab, below: Rect) {
    if ui.input(|i| i.key_pressed(Key::Escape)) && app.recording.is_none() && !ui.ctx().egui_wants_keyboard_input() {
        app.actions.push(Action::CloseSettings);
    }
    let mut clicked = None;
    let mut logout = false;
    let heading = page_title(tab);
    let footer = format!("CordFast {}", env!("CARGO_PKG_VERSION"));
    let close = form::modal(
        ui,
        "settings",
        below,
        app,
        heading,
        &footer,
        |ui, app| {
            // Discord's profile card over the search: the avatar, the name,
            // Edit Profiles.
            if profile_card(app, ui, tab == SettingsTab::Profile).clicked() {
                clicked = Some(SettingsTab::Profile);
            }
            ui.add_space(8.0);
            form::nav_search(ui, &mut app.settings_search);
            ui.add_space(6.0);
            let needle = app.settings_search.trim().to_lowercase();
            let mut first = true;
            for (heading, pages) in SECTIONS {
                let shown: Vec<_> = pages.iter().filter(|(_, _, label)| needle.is_empty() || crate::i18n::tr(label).to_lowercase().contains(&needle)).collect();
                if shown.is_empty() {
                    continue;
                }
                match heading {
                    Some(heading) => form::nav_section(ui, crate::i18n::tr(heading)),
                    None if !first => form::nav_section(ui, ""),
                    None => {}
                }
                first = false;
                for (id, icon, label) in shown {
                    let item = form::nav_icon_item(ui, *icon, crate::i18n::tr(label), *id == tab, false);
                    if item.clicked() {
                        clicked = Some(*id);
                    }
                    if *id == tab {
                        // In view when it opens from elsewhere (a shortcut, a menu).
                        let seen = egui::Id::new("settings-nav-seen");
                        if ui.data(|d| d.get_temp::<SettingsTab>(seen)) != Some(tab) {
                            ui.data_mut(|d| d.insert_temp(seen, tab));
                            item.scroll_to_me(None);
                        }
                        form::nav_outline(ui);
                    }
                }
            }
            if needle.is_empty() || t!("Log Out").to_lowercase().contains(&needle) {
                logout = form::nav_icon_item(ui, Icon::DoorExit, t!("Log Out"), false, true).clicked();
            }
        },
        |ui, app| {
            let before = app.settings.clone();
            match tab {
                SettingsTab::Profile => profile(app, ui),
                SettingsTab::Account => account(app, ui),
                SettingsTab::DataPrivacy => data_privacy(app, ui),
                SettingsTab::MessagingPermissions => messaging(app, ui),
                SettingsTab::Notifications => notifications(app, ui),
                SettingsTab::Clips => clips(ui),
                SettingsTab::Streamer => streamer(app, ui),
                SettingsTab::Nitro => nitro(app, ui),
                SettingsTab::ServerBoost => boosts(app, ui),
                SettingsTab::Subscriptions => subscriptions(app, ui),
                SettingsTab::GiftInventory => gifts(app, ui),
                SettingsTab::Billing => billing(app, ui),
                SettingsTab::Voice => voice(app, ui),
                SettingsTab::Appearance => appearance(app, ui),
                SettingsTab::Accessibility => accessibility(app, ui),
                SettingsTab::System => system(app, ui),
                SettingsTab::Language => language(app, ui),
                SettingsTab::RegisteredGames => games(app, ui),
                SettingsTab::ActivityPrivacy => activity_privacy(app, ui),
                SettingsTab::Overlay => super::overlay::settings_page(app, ui),
                SettingsTab::ConnectedApps => connected_apps(app, ui),
                SettingsTab::Developer => developer(app, ui),
                SettingsTab::Plugins => plugins_page(app, ui),
                SettingsTab::About => about(app, ui),
            }
            if app.settings != before {
                app.actions.push(Action::SettingsChanged);
            }
        },
    );
    if let Some(id) = clicked {
        app.actions.push(Action::OpenSettings(id));
    }
    if logout {
        app.modal = Some(crate::app::Modal::Confirm { title: t!("Log Out").into(), body: t!("Are you sure you want to logout?").into(), button: t!("Log Out").into(), then: crate::app::Confirmed::Logout });
    }
    if close {
        app.actions.push(Action::CloseSettings);
    }
}

/// The page's name in the window's header: its sidebar label.
fn page_title(tab: SettingsTab) -> &'static str {
    if tab == SettingsTab::Profile {
        return t!("Profiles");
    }
    SECTIONS.iter().flat_map(|(_, pages)| pages.iter()).find(|(id, _, _)| *id == tab).map_or("", |(_, _, label)| crate::i18n::tr(label))
}

/// The card over the sidebar: the avatar, the name and "Edit Profiles".
fn profile_card(app: &mut App, ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let p = theme::p();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 8.0, if selected { p.selected } else { p.hover });
    }
    let Some(me) = app.me.clone() else { return response };
    let avatar = Rect::from_min_size(rect.left_center() + vec2(4.0, -26.0), Vec2::splat(52.0));
    app.images.circle(ui, avatar, Some(&me.avatar_url()), &super::initials(me.display_name()), 26.0);
    let x = avatar.right() + 10.0;
    super::paint_truncated(ui, Rect::from_min_max(egui::pos2(x, rect.top() + 10.0), egui::pos2(rect.right() - 8.0, rect.center().y)), me.display_name(), theme::strong(16.0), p.strong);
    let edit = t!("Edit Profiles");
    let galley = ui.painter().layout_no_wrap(edit.to_owned(), egui::FontId::proportional(14.0), p.muted);
    let at = egui::pos2(x, rect.center().y + 2.0);
    let width = galley.size().x;
    ui.painter().galley(at, galley, p.muted);
    crate::icons::paint(ui, Rect::from_min_size(egui::pos2(at.x + width + 4.0, at.y + 2.0), Vec2::splat(13.0)), Icon::Pencil, p.muted);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---- shared pieces ---------------------------------------------------------------

/// One of the account's switches (PreloadedUserSettings), saved as soon as
/// it is flipped. `inverted` shows the opposite of what is stored (Discord
/// keeps "restricted" where the page asks "allow").
fn account_switch(app: &mut App, ui: &mut egui::Ui, key: PreKey, inverted: bool, title: &str, note: &str) {
    let stored = app.preloaded.flag(key);
    let mut on = stored != inverted;
    if form::switch_row(ui, &mut on, title, note) {
        app.set_preloaded(key, u64::from(on != inverted));
    }
}

/// One of the account's choices, as Discord's radio cards.
fn account_radio(app: &mut App, ui: &mut egui::Ui, key: PreKey, options: &[(u64, &str, &str)]) {
    let current = app.preloaded.number(key);
    if let Some(value) = form::radios(ui, current, options) {
        app.set_preloaded(key, value);
    }
}

/// A label over a control, for radio groups.
fn field_title(ui: &mut egui::Ui, text: &str, note: &str) {
    ui.label(RichText::new(text).font(theme::strong(16.0)).color(theme::p().strong));
    if !note.is_empty() {
        ui.add_space(2.0);
        form::note(ui, note);
    }
    ui.add_space(10.0);
}

/// An API answer for a page: drawn when it came, a spinner while it
/// loads, the error if it failed.
fn loaded<'a>(app: &'a App, ui: &mut egui::Ui, key: &str) -> Option<&'a Value> {
    match app.settings_data.get(key) {
        Some(Some(Ok(value))) => Some(value),
        Some(Some(Err(error))) => {
            ui.label(RichText::new(error).color(theme::p().red));
            None
        }
        _ => {
            ui.spinner();
            None
        }
    }
}

/// A row with the item's name and a detail under it, and a button on the
/// right: true when the button was clicked.
fn item_row(ui: &mut egui::Ui, name: &str, detail: &str, button: Option<(&str, Kind)>) -> bool {
    let p = theme::p();
    let mut clicked = false;
    Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(name).size(15.0).color(p.strong));
                if !detail.is_empty() {
                    ui.label(RichText::new(detail).size(13.0).color(p.muted));
                }
            });
            if let Some((label, kind)) = button {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = form::button(ui, label, kind).clicked();
                });
            }
        });
    });
    ui.add_space(8.0);
    clicked
}

fn open_web(path: &str) {
    let _ = open::that_detached(format!("https://discord.com{path}"));
}

fn date(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .and_then(|d| d.parse::<jiff::Timestamp>().ok())
        .map(|t| crate::text::time(t.as_millisecond().max(0) as u64))
        .unwrap_or_default()
}

// ---- General ------------------------------------------------------------------------

fn profile(app: &mut App, ui: &mut egui::Ui) {
    let Some(me) = app.me.clone() else { return };
    let Some(draft) = app.profile_draft.as_mut() else {
        ui.spinner();
        return;
    };
    form::label(ui, t!("Display Name"));
    form::input(ui, &mut draft.display_name, &me.username, 360.0);
    ui.add_space(20.0);
    form::label(ui, t!("Pronouns"));
    form::input(ui, &mut draft.pronouns, t!("Add your pronouns"), 360.0);
    ui.add_space(20.0);
    form::label(ui, t!("About Me"));
    form::textarea(ui, &mut draft.bio, "", 5, 190);
    let draft = draft.clone();
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if form::button(ui, t!("Save Changes"), Kind::Success).clicked() {
            app.commands.send(crate::backend::Command::Api { key: "profile-saved".into(), method: "PATCH", path: "/users/@me".into(), body: Some(serde_json::json!({ "global_name": draft.display_name.trim() })) });
            app.commands.send(crate::backend::Command::Api { key: "profile-saved".into(), method: "PATCH", path: "/users/@me/profile".into(), body: Some(serde_json::json!({ "bio": draft.bio, "pronouns": draft.pronouns })) });
        }
        if form::button(ui, t!("Change Avatar"), Kind::Primary).clicked()
            && let Some(file) = app.settings.file_dialog().add_filter(t!("Image"), &["png", "jpg", "jpeg", "gif", "webp"]).pick_file()
        {
            app.settings.picked(&file);
            match crate::admin::image_data(&file) {
                Some(data) => app.commands.send(crate::backend::Command::Api { key: "profile-saved".into(), method: "PATCH", path: "/users/@me".into(), body: Some(serde_json::json!({ "avatar": data })) }),
                None => app.notify(t!("Invalid image, or larger than 10 MB.")),
            }
        }
        if form::button(ui, t!("View My Profile"), Kind::Secondary).clicked() {
            let at = ui.ctx().pointer_latest_pos().unwrap_or_default();
            app.actions.push(Action::CloseSettings);
            app.actions.push(Action::OpenProfile { user: me.clone(), guild: None, at });
        }
    });
}

/// "s••••@gmail.com", as Discord hides an address until Reveal.
fn masked(text: &str) -> String {
    match text.split_once('@') {
        Some((name, domain)) => format!("{}@{domain}", "•".repeat(name.chars().count())),
        None => format!("{}{}", "•".repeat(text.chars().count().saturating_sub(4)), text.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<String>()),
    }
}

fn account(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let devices_id = ui.id().with("account-devices");
    if ui.data(|d| d.get_temp::<bool>(devices_id)).unwrap_or(false) {
        if form::link(ui, &format!("‹ {}", t!("Back"))) {
            ui.data_mut(|d| d.insert_temp(devices_id, false));
        }
        ui.add_space(16.0);
        devices(app, ui);
        return;
    }
    let Some(me) = loaded(app, ui, "settings:me").cloned() else { return };
    let reveal_id = ui.id().with("reveal");
    let mut revealed: (bool, bool) = ui.data(|d| d.get_temp(reveal_id)).unwrap_or_default();
    let text = |k: &str| me.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    let hide_personal = app.streamer_on() && app.settings.streamer_hide_personal;
    // Discord asks for the password in its own dialogs: Edit and Add open it.
    form::category(ui, t!("Account Info"), "", |ui| {
        if form::row(ui, t!("Username"), "", |ui| {
            let edit = form::button(ui, t!("Edit"), Kind::Secondary).clicked();
            ui.add_space(4.0);
            form::value(ui, &text("username"));
            edit
        }) {
            super::gates::account_dialog(app, super::gates::AccountEdit::Username);
        }
        let email = text("email");
        let (edit, reveal) = form::row(ui, t!("Email"), "", |ui| {
            let edit = form::button(ui, t!("Edit"), Kind::Secondary).clicked();
            ui.add_space(4.0);
            let reveal = !hide_personal && form::link(ui, if revealed.0 { t!("Hide") } else { t!("Reveal") });
            form::value(ui, &if revealed.0 && !hide_personal { email.clone() } else { masked(&email) });
            (edit, reveal)
        });
        revealed.0 ^= reveal;
        if edit {
            super::gates::account_dialog(app, super::gates::AccountEdit::Email);
        }
        let phone = text("phone");
        let (edit, reveal) = form::row(ui, t!("Phone Number"), "", |ui| {
            let edit = form::button(ui, if phone.is_empty() { t!("Add") } else { t!("Edit") }, Kind::Secondary).clicked();
            ui.add_space(4.0);
            if phone.is_empty() {
                form::value(ui, t!("You haven't added a phone number yet."));
                return (edit, false);
            }
            let reveal = !hide_personal && form::link(ui, if revealed.1 { t!("Hide") } else { t!("Reveal") });
            form::value(ui, &if revealed.1 && !hide_personal { phone.clone() } else { masked(&phone) });
            (edit, reveal)
        });
        revealed.1 ^= reveal;
        if edit {
            open_web("/channels/@me");
        }
    });
    ui.data_mut(|d| d.insert_temp(reveal_id, revealed));
    form::category(ui, t!("Password & Security"), "", |ui| {
        if form::row(ui, t!("Password"), "", |ui| form::button(ui, t!("Edit"), Kind::Secondary).clicked()) {
            super::gates::account_dialog(app, super::gates::AccountEdit::Password);
        }
        let mfa = me.get("mfa_enabled").and_then(Value::as_bool).unwrap_or(false);
        if form::link_row(ui, t!("Multi-Factor Authentication"), if mfa { t!("Enabled") } else { t!("Disabled") }) {
            super::gates::account_dialog(app, if mfa { super::gates::AccountEdit::MfaOff } else { super::gates::AccountEdit::MfaOn });
        }
        let count = app.settings_data.get("settings:sessions").and_then(|v| v.as_ref()).and_then(|r| r.as_ref().ok()).and_then(|v| v.get("user_sessions")).and_then(Value::as_array).map_or(0, Vec::len);
        if form::link_row(ui, t!("Logged-in Devices"), &tn!(count, "{count} device", "{count} devices")) {
            ui.data_mut(|d| d.insert_temp(devices_id, true));
        }
    });
    form::category(ui, t!("Account Status"), "", |ui| {
        if let Some(adult) = me.get("nsfw_allowed").and_then(Value::as_bool) {
            let (state, detail) = if adult {
                (t!("Adult"), t!("You’re in the adult age group (18+) and have access to age-restricted content, spaces, and settings."))
            } else {
                (t!("Teen"), t!("We’ve determined you’re in the teen age group (13-17)"))
            };
            form::status_row(ui, Icon::User, p.strong, t!("Age Group"), detail, state, false);
        }
        // Discord's safety hub: the account's standing and what it means.
        let standing = app.settings_data.get("settings:standing").and_then(|v| v.as_ref()).and_then(|r| r.as_ref().ok()).and_then(|v| v.pointer("/account_standing/state")).and_then(Value::as_u64);
        if let Some(state) = standing {
            let risk = t!("You may lose access to some parts of Discord if you break the rules again.");
            let (label, detail, tint) = match state {
                100 => (t!("All good"), t!("Thank you for upholding Discord's Terms of Service and Community Guidelines. If you have a violation, it will appear here."), p.green),
                200 => (t!("Limited"), risk, p.warning),
                300 => (t!("Very limited"), risk, p.warning),
                400 => (t!("At risk"), risk, p.red),
                _ => (t!("Suspended"), "", p.red),
            };
            if form::status_row(ui, if state == 100 { Icon::Check } else { Icon::Info }, tint, t!("Account Standing"), detail, label, true) {
                open_web("/channels/@me");
            }
        }
    });
    form::category(ui, t!("Family Center"), "", |ui| {
        if form::row(ui, t!("Family Center"), "", |ui| form::button(ui, t!("Open"), Kind::Secondary).clicked()) {
            open_web("/family-center");
        }
    });
    form::category(ui, "", "", |ui| {
        if form::row(ui, t!("Disable your account"), t!("Temporarily disable your account."), |ui| form::button(ui, t!("Disable Account"), Kind::Secondary).clicked()) {
            super::gates::account_dialog(app, super::gates::AccountEdit::Disable);
        }
        if form::row(ui, t!("Close your account"), t!("Permanently close your account."), |ui| form::button(ui, t!("Delete Account"), Kind::Danger).clicked()) {
            super::gates::account_dialog(app, super::gates::AccountEdit::Delete);
        }
    });
}

/// Streamer Mode: what it hides while a stream runs.
fn streamer(app: &mut App, ui: &mut egui::Ui) {
    let mut changed = false;
    form::category(ui, t!("Streamer Mode"), "", |ui| {
        changed |= form::switch_row(ui, &mut app.settings.streamer_mode, t!("Enable Streamer Mode"), "");
        changed |= form::switch_row(ui, &mut app.settings.streamer_auto, t!("Automatically Enable/Disable"), "OBS, XSplit");
    });
    form::category(ui, t!("Options"), "", |ui| {
        changed |= form::switch_row(ui, &mut app.settings.streamer_hide_personal, t!("Hide Personal Information"), "");
        changed |= form::switch_row(ui, &mut app.settings.streamer_hide_invites, t!("Hide Invite Links"), "");
        changed |= form::switch_row(ui, &mut app.settings.streamer_no_sounds, t!("Disable Sounds"), "");
        changed |= form::switch_row(ui, &mut app.settings.streamer_no_notifications, t!("Disable Notifications"), "");
    });
    if changed {
        app.actions.push(Action::SettingsChanged);
    }
}

/// Logged-in Devices, opened from its row on the account page.
fn devices(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("Logged-in Devices"), t!("These are all the devices that are currently logged-in with your Discord account. Log out of devices that you don't recognize."), |ui| {
        let Some(sessions) = loaded(app, ui, "settings:sessions").and_then(|v| v.get("user_sessions")).and_then(Value::as_array).cloned() else { return };
        let mut out = Vec::new();
        for session in &sessions {
            let info = session.get("client_info").cloned().unwrap_or_default();
            let field = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            let name = [field("os"), field("platform")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            let detail = [field("location"), date(session.get("approx_last_used_time"))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            if item_row(ui, if name.is_empty() { t!("Unknown") } else { &name }, &detail, Some((t!("Log Out"), Kind::Secondary))) {
                out.extend(session.get("id_hash").and_then(Value::as_str).map(str::to_owned));
            }
        }
        if sessions.len() > 1 && form::button(ui, t!("Log out of all known devices"), Kind::Danger).clicked() {
            out.extend(sessions.iter().filter_map(|s| s.get("id_hash").and_then(Value::as_str).map(str::to_owned)));
        }
        if !out.is_empty() {
            app.commands.send(crate::backend::Command::Api { key: "sessions-logout".into(), method: "POST", path: "/auth/sessions/logout".into(), body: Some(serde_json::json!({ "session_id_hashes": out })) });
            app.commands.send(crate::admin::get("settings:sessions", "/auth/sessions".into()));
        }
    });
}

fn data_privacy(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("How Discord Uses My Data"), "", |ui| {
        let mut always = true;
        ui.add_enabled_ui(false, |ui| form::switch_row(ui, &mut always, t!("Use data to make Discord work"), t!("Discord needs to store and process some data in order to provide you the basic Discord service, such as your messages, what servers you’re in and your Direct Messages. By using Discord, you allow Discord to provide this basic service. You can stop this by Disabling or Deleting your account.")));
        let consent = app.settings_data.get("settings:consent").and_then(|v| v.as_ref()).and_then(|r| r.as_ref().ok()).cloned();
        for (key, title, note) in [
            ("usage_statistics", t!("Use data to improve Discord"), t!("Allow Discord to use and process my information to understand and improve Discord’s services.")),
            ("personalization", t!("Use data to customize my Discord experience"), t!("Allow Discord to use and process my information to customize my Discord experience.")),
        ] {
            let Some(consent) = &consent else { break };
            let stored = consent.pointer(&format!("/{key}/consented")).and_then(Value::as_bool).unwrap_or(false);
            let mut on = stored;
            if form::switch_row(ui, &mut on, title, note) {
                let body = if on { serde_json::json!({ "grant": [key], "revoke": [] }) } else { serde_json::json!({ "grant": [], "revoke": [key] }) };
                app.commands.send(crate::backend::Command::Api { key: "consent".into(), method: "POST", path: "/users/@me/consent".into(), body: Some(body) });
                if let Some(Some(Ok(value))) = app.settings_data.get_mut("settings:consent") {
                    value[key]["consented"] = Value::Bool(on);
                }
            }
        }
        if form::row(ui, t!("Request my data"), t!("Get a copy of the data Discord has about you. It can take up to 30 days to arrive by email."), |ui| form::button(ui, t!("Request Data"), Kind::Primary).clicked()) {
            app.commands.send(crate::backend::Command::Api { key: "harvest".into(), method: "POST", path: "/users/@me/harvest".into(), body: Some(serde_json::json!({})) });
            app.notify(t!("Done."));
        }
    });
    form::category(ui, t!("Profile Privacy"), t!("Control who can see your profile info — like your bio and connected accounts."), |ui| {
        field_title(ui, t!("Share my full profile with"), "");
        account_radio(app, ui, pre::PROFILE_VISIBILITY, &[
            (3, t!("Friends & All Servers"), t!("Your full profile is visible to friends and any server you join.")),
            (2, t!("Friends & Small Servers Only"), t!("Your full profile is visible to friends and any server you join with 200 or fewer members. Everyone else sees a limited version.")),
            (1, t!("Friends Only"), t!("Your full profile is visible to friends. Everyone else sees a limited version.")),
        ]);
        ui.add_space(12.0);
        account_switch(app, ui, pre::NOTIFY_FRIENDS_ON_PROFILE_UPDATE, false, t!("Share my profile updates with friends"), "");
    });
}

fn messaging(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("Content Filters"), "", |ui| {
        // Discord's three places, each to show, blur or block.
        let names = [t!("Show").to_owned(), t!("Blur").to_owned(), t!("Block").to_owned()];
        for (which, label) in [(1, t!("Direct messages from friends")), (2, t!("Direct messages from others")), (3, t!("Server channels"))] {
            // explicit_content_friend_dm, _non_friend_dm and _guilds.
            let field = match which {
                1 => 2,
                2 => 3,
                _ => 1,
            };
            let current = app.preloaded.content_filter(field).clamp(1, 3) as usize - 1;
            form::label(ui, label);
            if let Some(picked) = form::select(ui, &format!("content-filter-{which}"), &names[current], &names, 240.0) {
                let patch = app.preloaded.set_content_filter(field, picked as u64 + 1);
                app.commands.send(crate::backend::Command::SaveSettings(patch));
            }
            ui.add_space(10.0);
        }
        account_switch(app, ui, pre::NSFW_COMMANDS, false, t!("Allow access to age-restricted commands from apps in DMs"), t!("Allows people 18+ to access commands marked as age-restricted in DMs. Applies to all apps."));
        account_switch(app, ui, pre::NSFW_GUILDS_IOS, false, t!("Allow access to age-restricted servers on iOS"), t!("Access age-restricted servers (18+) on iOS devices, after joining them on desktop."));
    });
    form::category(ui, t!("Spam Filters"), "", |ui| {
        field_title(ui, t!("Automatically filter suspected spam messages"), t!("Discord can filter out some messages that contain spam. These messages go to your Spam inbox."));
        let current = match app.preloaded.number(pre::DM_SPAM_FILTER) {
            0 => 2,
            n => n,
        };
        if let Some(value) = form::radios(ui, current, &[(3, t!("Filter all spam"), ""), (2, t!("Filter messages from non-friends (Recommended)"), ""), (1, t!("Don't filter spam"), "")]) {
            app.set_preloaded(pre::DM_SPAM_FILTER, value);
        }
    });
    form::category(ui, t!("Direct Message (DM) Permissions"), "", |ui| {
        account_switch(app, ui, pre::GUILDS_RESTRICTED, true, t!("Direct Messages"), t!("Allow DMs from other server members"));
        account_switch(app, ui, pre::MESSAGE_REQUESTS_RESTRICTED, true, t!("Message requests"), t!("Filter messages from server members you may not know."));
    });
    form::category(ui, t!("Friend Request Permissions"), t!("Control who can send you friend requests and how they appear."), |ui| {
        field_title(ui, t!("Allow Friend Requests from..."), "");
        let flags = app.preloaded.number(pre::FRIEND_SOURCES);
        // Discord's bits: 2 friends of friends, 4 server members, 8 the rest.
        let mut everyone = flags & 8 != 0;
        let mut friends = flags & 2 != 0 || everyone;
        let mut members = flags & 4 != 0 || everyone;
        let before = (everyone, friends, members);
        form::switch_row(ui, &mut everyone, t!("Everyone"), "");
        form::switch_row(ui, &mut friends, t!("Friend of friends"), "");
        form::switch_row(ui, &mut members, t!("Server members"), t!("Server members can only send you friend requests from servers where you also allow Direct Messages."));
        if (everyone, friends, members) != before {
            let value = if everyone && everyone != before.0 { 14 } else { u64::from(friends) * 2 + u64::from(members) * 4 + if everyone && friends && members { 8 } else { 0 } };
            app.set_preloaded(pre::FRIEND_SOURCES, value);
        }
    });
    form::category(ui, t!("Ignore & Block"), t!("You’re in control. To compare your options for reducing unwanted interactions, explore our feature guide"), |ui| {
        let blocked: Vec<crate::discord::model::User> = app.relationships.values().filter(|r| r.kind == 2).map(|r| r.user.clone()).collect();
        if blocked.is_empty() {
            form::note(ui, t!("You haven't blocked anyone."));
        }
        for user in blocked {
            if item_row(ui, user.display_name(), &user.username, Some((t!("Unblock"), Kind::Secondary))) {
                app.actions.push(Action::Relationship { user: user.id, kind: None });
            }
        }
    });
}

fn sounds_list(app: &mut App, ui: &mut egui::Ui, which: &[crate::sounds::Sound]) {
    for (sound, label, name) in crate::sounds::ALL {
        if !which.contains(&sound) {
            continue;
        }
        let mut on = !app.settings.sounds_off.iter().any(|s| s == name);
        if form::switch_row(ui, &mut on, crate::i18n::tr(label), "") {
            app.settings.sounds_off.retain(|s| s != name);
            if !on {
                app.settings.sounds_off.push(name.to_owned());
            }
        }
    }
}

fn notifications(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("Overview"), "", |ui| {
        let s = &mut app.settings;
        form::switch_row(ui, &mut s.notifications, t!("Enable Desktop Notifications"), t!("If you're looking for per-channel or per-server notifications, right-click the desired server icon and select Notification Settings."));
        form::switch_row(ui, &mut s.notify_dms, t!("Direct Messages"), "");
        form::switch_row(ui, &mut s.notify_mentions, t!("Mentions and Replies"), t!("Follows the channels and servers you muted in Discord."));
        form::switch_row(ui, &mut s.notify_preview, t!("Show Message Text"), "");
        field_title(ui, t!("Notify me when..."), "");
        account_switch(app, ui, pre::GO_LIVE_NOTIFICATIONS, false, t!("People I know start streaming in small servers"), "");
        account_switch(app, ui, pre::FRIEND_ANNIVERSARY_NOTIFICATIONS, false, t!("A friend and I reach a friendship anniversary"), "");
        account_switch(app, ui, pre::FRIEND_ONLINE_NOTIFICATIONS, false, t!("Friends come online"), "");
        account_switch(app, ui, pre::SERVER_EVENT_NOTIFICATIONS, false, t!("A server has an upcoming event"), "");
        account_switch(app, ui, pre::PROFILE_UPDATE_NOTIFICATIONS, false, t!("Friends update their profile"), "");
        let options = [t!("All Messages").to_owned(), t!("Only Direct Messages").to_owned(), t!("Never").to_owned()];
        let current = app.preloaded.number(pre::REACTION_NOTIFICATIONS).min(2) as usize;
        if let Some(i) = form::row(ui, t!("Someone reacts to my messages"), "", |ui| form::select(ui, "reaction-notifications", &options[current], &options, 220.0)) {
            app.set_preloaded(pre::REACTION_NOTIFICATIONS, i as u64);
        }
    });
    form::category(ui, t!("Sounds"), "", |ui| {
        sounds_list(app, ui, &[crate::sounds::Sound::Message]);
        let mut all_off = !app.settings.notify_sound;
        if form::switch_row(ui, &mut all_off, t!("Disable All Notification Sounds"), t!("Disables all notification sounds, including voice & video sound effects. Your individual sound preferences will be saved and restored if you turn this on.")) {
            app.settings.notify_sound = !all_off;
        }
    });
}

fn clips(ui: &mut egui::Ui) {
    form::unavailable(ui, t!("Clipping needs screen sharing, which isn't in CordFast yet."));
    form::category(ui, t!("Clipping"), t!("Clip any fun or amazing gaming moments by saving them directly to your computer."), |ui| {
        let mut off = false;
        ui.add_enabled_ui(false, |ui| form::switch_row(ui, &mut off, t!("Enable Clipping"), ""));
    });
}

// ---- Billing (read-only) ----------------------------------------------------------------

fn nitro(app: &mut App, ui: &mut egui::Ui) {
    let Some(me) = loaded(app, ui, "settings:me").cloned() else { return };
    let tier = match me.get("premium_type").and_then(Value::as_u64).unwrap_or(0) {
        1 => Some(t!("Nitro Classic")),
        2 => Some(t!("Nitro")),
        3 => Some(t!("Nitro Basic")),
        _ => None,
    };
    let p = theme::p();
    Frame::new().fill(p.accent.gamma_multiply(0.25)).corner_radius(12).inner_margin(Margin::same(20)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        crate::icons::paint(ui, Rect::from_min_size(ui.cursor().min, Vec2::splat(32.0)), Icon::Nitro, p.strong);
        ui.add_space(40.0);
        match tier {
            Some(tier) => {
                ui.label(RichText::new(tier).font(theme::strong(22.0)).color(p.strong));
                form::note(ui, t!("Your subscription is managed on Discord."));
            }
            None => {
                ui.label(RichText::new(t!("You don't have Nitro")).font(theme::strong(22.0)).color(p.strong));
                form::note(ui, t!("CordFast shows your plan but sells nothing: subscriptions are made on Discord."));
            }
        }
    });
    ui.add_space(20.0);
    if form::button(ui, t!("Open on Discord"), Kind::Secondary).clicked() {
        open_web("/nitro");
    }
}

/// Server Boost: what Discord's page manages. The boosts can be moved,
/// applied and moved here; buying more and canceling stay on Discord.
fn boosts(app: &mut App, ui: &mut egui::Ui) {
    use crate::app::{Confirmed, Modal};
    use crate::discord::model::sf;
    let p = theme::p();
    let Some(slots) = loaded(app, ui, "settings:boosts").and_then(Value::as_array).cloned() else { return };
    let answer = |app: &App, key: &str| app.settings_data.get(key).and_then(|v| v.as_ref()).and_then(|r| r.as_ref().ok()).cloned();
    // Nitro includes two (the bundle's getNumIncludedPremiumGuildSubscriptionSlots):
    // those can only go by canceling Nitro itself.
    let included = if answer(app, "settings:me").and_then(|me| me.get("premium_type").and_then(Value::as_u64)) == Some(2) { 2 } else { 0 };
    let subscriptions = answer(app, "settings:subscriptions").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let guilds: Vec<(crate::discord::model::Id, String)> = app.guilds.iter().map(|g| (g.id, g.name.clone())).collect();
    let id_of = |slot: &Value| slot.get("id").and_then(sf);
    let applied = |slot: &Value| slot.pointer("/premium_guild_subscription/guild_id").and_then(sf);
    let canceled = |slot: &Value| slot.get("canceled").and_then(Value::as_bool).unwrap_or(false);
    let active = slots.iter().filter(|s| !canceled(s)).count();
    let free: Vec<crate::discord::model::Id> = slots.iter().filter(|s| applied(s).is_none() && !on_cooldown(s) && !canceled(s)).filter_map(id_of).collect();
    let unused = slots.iter().filter(|s| applied(s).is_none()).count();
    let apply_modal = |guild, slot| Modal::Confirm {
        title: t!("Boost This Server").into(),
        body: t!("Boosting this server will prevent you from transferring this Boost to another server for 7 days.").into(),
        button: t!("Boost this server").into(),
        then: Confirmed::ApplyBoost { guild, slot },
    };
    if slots.is_empty() {
        form::note(ui, t!("You don't have any Boosts."));
        ui.add_space(24.0);
    }
    if unused > 0 {
        Frame::new().fill(p.hover).corner_radius(8).inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                crate::icons::paint(ui, icon, Icon::Boost, BOOST);
                ui.vertical(|ui| {
                    ui.label(RichText::new(t!("Your Boosts are waiting!")).font(theme::strong(16.0)).color(p.strong));
                    ui.label(RichText::new(tn!(unused, "You have {count} unused Boost", "You have {count} unused Boosts")).size(13.0).color(p.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button = form::button_enabled(ui, t!("Boost any server"), Kind::Primary, !free.is_empty());
                    if let (Some(guild), Some(slot)) = (guild_menu(&button, &guilds), free.first()) {
                        app.modal = Some(apply_modal(guild, *slot));
                    }
                });
            });
        });
        ui.add_space(32.0);
    }
    if !slots.is_empty() {
        form::category(ui, t!("Active Boosts"), t!("These are your purchased Boosts, powering your favorite servers."), |ui| {
            let mut groups: Vec<(Option<crate::discord::model::Id>, Vec<&Value>)> = Vec::new();
            for slot in &slots {
                let guild = applied(slot);
                match groups.iter_mut().find(|(g, _)| *g == guild) {
                    Some((_, group)) => group.push(slot),
                    None => groups.push((guild, vec![slot])),
                }
            }
            for (guild, group) in groups {
                Frame::new().stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(16)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(guild) = guild {
                        let known = app.guild(guild).map(|g| (g.name.clone(), g.icon_url()));
                        ui.horizontal(|ui| {
                            let (icon, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                            let name = known.as_ref().map_or_else(|| t!("Unknown Server").to_owned(), |(name, _)| name.clone());
                            app.images.circle(ui, icon, known.as_ref().and_then(|(_, url)| url.as_deref()), &super::initials(&name), 16.0);
                            ui.label(RichText::new(&name).font(theme::strong(16.0)).color(p.strong));
                            if known.is_some() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if form::button(ui, t!("Go to server"), Kind::Secondary).clicked() {
                                        app.actions.push(Action::CloseSettings);
                                        app.actions.push(Action::SelectGuild(Some(guild)));
                                    }
                                });
                            }
                        });
                        ui.add_space(12.0);
                    }
                    for slot in group {
                        let Some(id) = id_of(slot) else { continue };
                        let cooldown = on_cooldown(slot);
                        let is_canceled = canceled(slot);
                        let ends = subscriptions.iter().find(|s| s.get("id") == slot.get("subscription_id")).map(|s| date(s.get("current_period_end"))).unwrap_or_default();
                        let status = if is_canceled {
                            t!("Pending Cancellation: {date}", date = ends.clone())
                        } else if cooldown {
                            t!("Cooldown ends {when}", when = date(slot.get("cooldown_ends_at")))
                        } else if let Some(pgs) = slot.pointer("/premium_guild_subscription/id").and_then(sf) {
                            // A snowflake's timestamp: when this Boost was applied.
                            t!("Since {date}", date = crate::text::short_day((pgs >> 22) + 1_420_070_400_000))
                        } else {
                            t!("Looks like this Boost could use a new home!").to_owned()
                        };
                        ui.horizontal(|ui| {
                            let (icon, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                            crate::icons::paint(ui, icon, Icon::Boost, BOOST);
                            ui.label(RichText::new(status).size(14.0).color(p.muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (more, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                                if response.hovered() {
                                    ui.painter().rect_filled(more, 6.0, p.hover);
                                }
                                crate::icons::paint(ui, more.shrink(5.0), Icon::More, if response.hovered() { p.strong } else { p.muted });
                                egui::Popup::menu(&response.on_hover_text(t!("More Options")).on_hover_cursor(egui::CursorIcon::PointingHand)).show(|ui| {
                                    super::menu::begin(ui);
                                    let transfer = if applied(slot).is_some() { t!("Transfer Boost") } else { t!("Select a Server") };
                                    if cooldown {
                                        ui.add_enabled_ui(false, |ui| super::menu::item_with(ui, transfer, t!("This Boost is on cooldown"), super::menu::Tail::None));
                                    } else if let Some(Some(to)) = super::menu::submenu(ui, transfer, |ui| guild_list(ui, &guilds)) {
                                        app.modal = Some(match (applied(slot), slot.pointer("/premium_guild_subscription/id").and_then(sf)) {
                                            (Some(from), Some(boost)) => Modal::Confirm {
                                                title: t!("Transfer Boost").into(),
                                                body: t!("Are you sure you want to transfer this Server Boost? The current server will lose progress if you continue.").into(),
                                                button: t!("Transfer Boost").into(),
                                                then: Confirmed::TransferBoost { from, boost, to, slot: id },
                                            },
                                            _ => apply_modal(to, id),
                                        });
                                        ui.close();
                                    }
                                    if is_canceled {
                                        if super::menu::item(ui, t!("Uncancel Boost Subscription"), super::menu::Tail::None).clicked() {
                                            open_web("/channels/@me");
                                            ui.close();
                                        }
                                    } else if active > included {
                                        if super::menu::danger(ui, t!("Cancel Boost Subscription"), super::menu::Tail::None).clicked() {
                                            // Recurring billing: Discord\'s site, not this client.
                                            open_web("/channels/@me");
                                            ui.close();
                                        }
                                    } else {
                                        ui.add_enabled_ui(false, |ui| super::menu::item_with(ui, t!("Cancel Boost Subscription"), t!("This Boost is included with your Nitro subscription"), super::menu::Tail::None));
                                    }
                                });
                            });
                        });
                        ui.add_space(4.0);
                    }
                });
                ui.add_space(12.0);
            }
        });
    }
    // The bundle's list starts from the servers used most; the sidebar's
    // first ones stand in for them.
    form::category(ui, t!("These servers could use a Boost"), "", |ui| {
        for (guild, name) in guilds.iter().take(3) {
            let url = app.guild(*guild).and_then(|g| g.icon_url());
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                app.images.circle(ui, icon, url.as_deref(), &super::initials(name), 20.0);
                ui.label(RichText::new(name).font(theme::strong(16.0)).color(p.strong));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::button_enabled(ui, t!("Boost this server"), Kind::Secondary, !free.is_empty()).clicked()
                        && let Some(slot) = free.first()
                    {
                        app.modal = Some(apply_modal(*guild, *slot));
                    }
                });
            });
            ui.add_space(12.0);
        }
    });
}

/// Discord's boost pink.
const BOOST: Color32 = Color32::from_rgb(0xff, 0x73, 0xfa);

/// Whether a slot waits out the days after it moved.
fn on_cooldown(slot: &Value) -> bool {
    slot.get("cooldown_ends_at").and_then(Value::as_str).and_then(|d| d.parse::<jiff::Timestamp>().ok()).is_some_and(|t| t > jiff::Timestamp::now())
}

/// The servers as menu rows: the one clicked.
fn guild_list(ui: &mut egui::Ui, guilds: &[(crate::discord::model::Id, String)]) -> Option<crate::discord::model::Id> {
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for (guild, name) in guilds {
            if super::menu::item(ui, name, super::menu::Tail::None).clicked() {
                picked = Some(*guild);
                ui.close();
            }
        }
    });
    picked
}

/// A button's menu of servers: the one picked.
fn guild_menu(button: &egui::Response, guilds: &[(crate::discord::model::Id, String)]) -> Option<crate::discord::model::Id> {
    egui::Popup::menu(button)
        .show(|ui| {
            super::menu::begin(ui);
            guild_list(ui, guilds)
        })
        .and_then(|shown| shown.inner)
}

fn subscriptions(app: &mut App, ui: &mut egui::Ui) {
    let Some(list) = loaded(app, ui, "settings:subscriptions").and_then(Value::as_array).cloned() else { return };
    if list.is_empty() {
        form::note(ui, t!("You don't have any subscriptions."));
    }
    for sub in &list {
        let plan = sub.pointer("/items/0/plan_id").and_then(Value::as_str).unwrap_or_default();
        let ends = date(sub.get("current_period_end"));
        let canceled = sub.get("canceled_at").is_some_and(|c| !c.is_null());
        let detail = if canceled { t!("Ends {when}", when = ends) } else { t!("Renews {when}", when = ends) };
        item_row(ui, &format!("{} {plan}", t!("Plan")), &detail, None);
    }
}

fn gifts(app: &mut App, ui: &mut egui::Ui) {
    let codes = app.settings_data.get("settings:gift-codes").and_then(|v| v.as_ref()).and_then(|r| r.as_ref().ok()).and_then(Value::as_array).cloned().unwrap_or_default();
    let Some(gifts) = loaded(app, ui, "settings:gifts").and_then(Value::as_array).cloned() else { return };
    if gifts.is_empty() && codes.is_empty() {
        form::note(ui, t!("You don't have any gifts."));
    }
    for gift in gifts.iter().chain(codes.iter()) {
        let name = gift.pointer("/sku/name").or_else(|| gift.pointer("/store_listing/sku/name")).and_then(Value::as_str).unwrap_or_default();
        let code = gift.get("code").and_then(Value::as_str).unwrap_or_default();
        item_row(ui, if name.is_empty() { t!("Gift") } else { name }, code, None);
    }
}

fn billing(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("Payment Methods"), "", |ui| {
        let Some(sources) = loaded(app, ui, "settings:payment-sources").and_then(Value::as_array).cloned() else { return };
        if sources.is_empty() {
            form::note(ui, t!("You don't have any payment methods."));
        }
        for source in &sources {
            let brand = source.get("brand").and_then(Value::as_str).unwrap_or("PayPal");
            let last = source.get("last_4").and_then(Value::as_str).map(|l| format!("•••• {l}")).unwrap_or_default();
            let expires = match (source.get("expires_month").and_then(Value::as_u64), source.get("expires_year").and_then(Value::as_u64)) {
                (Some(m), Some(y)) => t!("Expires {date}", date = format!("{m:02}/{y}")),
                _ => String::new(),
            };
            item_row(ui, &format!("{} {last}", brand.to_uppercase()), &expires, None);
        }
    });
    form::category(ui, t!("Transaction History"), "", |ui| {
        let Some(payments) = loaded(app, ui, "settings:payments").and_then(Value::as_array).cloned() else { return };
        if payments.is_empty() {
            form::note(ui, t!("No transactions yet."));
        }
        for payment in &payments {
            let what = payment.get("description").and_then(Value::as_str).unwrap_or_default();
            let amount = payment.get("amount").and_then(Value::as_u64).unwrap_or(0);
            let currency = payment.get("currency").and_then(Value::as_str).unwrap_or_default().to_uppercase();
            let when = date(payment.get("created_at"));
            item_row(ui, what, &format!("{when} · {:.2} {currency}", amount as f64 / 100.0), None);
        }
    });
}

// ---- Experience ---------------------------------------------------------------------------

fn voice(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let (inputs, outputs) = app.devices.clone();
    form::category(ui, t!("Voice"), "", |ui| {
        ui.columns(2, |columns| {
            let pick = |ui: &mut egui::Ui, label: &str, id: &str, list: &[String], chosen: &mut Option<String>| {
                field_title(ui, label, "");
                let mut options = vec![super::default_device().to_owned()];
                options.extend(list.iter().cloned());
                let shown = chosen.clone().unwrap_or_else(|| options[0].clone());
                let width = ui.available_width() - 8.0;
                if let Some(i) = form::select(ui, id, &shown, &options, width) {
                    *chosen = (i > 0).then(|| options[i].clone());
                }
            };
            pick(&mut columns[0], t!("Microphone"), "input-device", &inputs, &mut app.settings.input_device);
            pick(&mut columns[1], t!("Speaker"), "output-device", &outputs, &mut app.settings.output_device);
        });
        ui.add_space(16.0);
        ui.columns(2, |columns| {
            form::volume_slider(&mut columns[0], t!("Microphone Volume"), &mut app.settings.input_volume, 0.0..=2.0);
            form::volume_slider(&mut columns[1], t!("Speaker Volume"), &mut app.settings.output_volume, 0.0..=2.0);
        });
        // Discord's mic test: hear yourself as others hear you, the ticks
        // lit with the level while it runs.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let label = if app.mic_test { t!("Stop Testing") } else { t!("Mic Test") };
            if form::button(ui, label, Kind::Primary).clicked() {
                app.mic_test = !app.mic_test;
                app.commands.send(crate::backend::Command::Voice(crate::voice::VoiceCommand::MicTest(app.mic_test)));
            }
            let level = if app.mic_test { ((app.voice.mic_level + 100.0) / 100.0).clamp(0.0, 1.0) } else { 0.0 };
            let width = ui.available_width();
            form::tick_meter(ui, width, level);
            if app.mic_test {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        });
        ui.add_space(24.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // Discord's sentence, its link the article it names.
            let sentence = t!("Need help? Check out our troubleshooting guide.");
            let link = t!("troubleshooting guide");
            match sentence.split_once(link) {
                Some((before, after)) => {
                    form::note(ui, before);
                    if form::link(ui, link) {
                        let _ = open::that_detached("https://support.discord.com/hc/articles/360045138471");
                    }
                    form::note(ui, after);
                }
                None => form::note(ui, sentence),
            }
        });
        form::divider(ui);
        field_title(ui, t!("Input Profile"), "");
        let current = super::InputProfile::of(&app.settings);
        let options: Vec<(super::InputProfile, &str, &str)> = [super::InputProfile::VoiceIsolation, super::InputProfile::Studio, super::InputProfile::Custom].into_iter().map(|p| (p, p.name(), p.detail())).collect();
        if let Some(profile) = form::radios(ui, current, &options)
            && profile != super::InputProfile::Custom
        {
            profile.apply(app);
        }
        ui.add_space(8.0);
        form::switch_row(ui, &mut app.settings.automatic_sensitivity, t!("Automatically Adjust Input Sensitivity"), "");
        let level = app.voice.mic_level;
        if app.settings.automatic_sensitivity {
            // The live level alone, as Discord's meter under the switch.
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, p.selected);
            let lit = if app.voice.speaking.contains(&app.me_id()) { p.green } else { p.warning };
            ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * ((level + 100.0) / 100.0).clamp(0.0, 1.0), rect.height())), 4.0, lit);
            ui.add_space(24.0);
        } else {
            field_title(ui, t!("Input Sensitivity"), t!("Controls how much sound Discord transmits from your mic."));
            form::sensitivity_slider(ui, &mut app.settings.input_sensitivity, level);
            ui.add_space(24.0);
        }
        // Discord's options are Krisp, Standard and None; CordFast's own
        // suppression (DeepFilterNet) stands as Standard.
        let options = [t!("Standard").to_owned(), t!("None").to_owned()];
        let current = if app.settings.noise_suppression { 0 } else { 1 };
        if let Some(i) = form::row(ui, t!("Noise Suppression"), t!("Reduces background noise from your mic."), |ui| form::select(ui, "noise-suppression", &options[current], &options, 200.0)) {
            app.settings.noise_suppression = i == 0;
        }
        ui.add_space(8.0);
        form::switch_row(ui, &mut app.settings.echo_cancellation, t!("Echo Cancellation"), "");
        form::switch_row(ui, &mut app.settings.push_to_talk, t!("Push to Talk"), t!("Activates your mic only when you hold down a designated key."));
        if app.settings.push_to_talk {
            push_to_talk_key(app, ui);
        }
        advanced_voice(app, ui);
    });
    form::category(ui, t!("Camera"), "", |ui| {
        form::unavailable(ui, t!("Camera and screen sharing aren't available yet."));
        account_switch(app, ui, pre::ALWAYS_PREVIEW_VIDEO, false, t!("Always preview video"), t!("Pops up preview modal every time you turn on video"));
    });
    form::category(ui, t!("Streaming"), "", |ui| {
        account_switch(app, ui, pre::DISABLE_STREAM_PREVIEWS, true, t!("Show Stream Previews"), t!("Allows others to see a preview of your stream before they join."));
    });
    form::category(ui, t!("Sounds"), t!("Enable/disable sounds that play when you’re in a call, like Mute, Unmute, Deafen, and more."), |ui| {
        use crate::sounds::Sound;
        sounds_list(app, ui, &[Sound::Deafen, Sound::Undeafen, Sound::Mute, Sound::Unmute, Sound::Disconnect, Sound::PttStart, Sound::PttStop, Sound::Join, Sound::Leave]);
    });
    form::category(ui, t!("Soundboard"), "", |ui| {
        ui.label(RichText::new(t!("Soundboard Volume")).size(14.0).color(p.text));
        let mut volume = app.preloaded.soundboard_volume();
        ui.spacing_mut().slider_width = 300.0;
        let slider = ui.add(egui::Slider::new(&mut volume, 0.0..=100.0).trailing_fill(true).custom_formatter(|v, _| format!("{v:.0}%")));
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            let patch = app.preloaded.set_soundboard_volume(volume);
            app.commands.send(crate::backend::Command::SaveSettings(patch));
        } else if slider.changed() {
            app.preloaded.set_soundboard_volume(volume);
        }
    });
    form::category(ui, t!("Voice Region"), t!("Calls always use the region with the lowest ping."), |ui| {
        for (i, (region, ms)) in app.voice.regions.iter().enumerate() {
            let best = if i == 0 { format!("  ← {}", t!("best route")) } else { String::new() };
            let color = if *ms < 60 { p.green } else if *ms < 140 { p.warning } else { p.red };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{ms:>4} ms")).monospace().color(color));
                ui.label(RichText::new(format!("{region}{best}")).color(p.text));
            });
        }
        ui.add_space(8.0);
        if form::button(ui, t!("Measure Again"), Kind::Secondary).clicked() {
            app.commands.send(crate::backend::Command::Voice(crate::voice::VoiceCommand::RankRegions));
        }
        if let Some((_, channel)) = app.voice.channel {
            ui.add_space(6.0);
            super::chat::region_picker(app, ui, channel);
        }
    });
    form::category(ui, t!("Advanced"), "", |ui| {
        // Diagnosis: 20 s of the call, saved beside the logs.
        let id = egui::Id::new("voice-recording");
        let since: Option<f64> = ui.data(|d| d.get_temp(id));
        let now = ui.input(|i| i.time);
        let recording = since.filter(|s| now - s < 21.0);
        let note = match recording {
            Some(started) => {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                t!("Recording… {seconds} s", seconds = format!("{:.0}", (20.0 - (now - started)).max(0.0)))
            }
            None => t!("Diagnostic audio recording is used for analyzing audio problems. The last 20 seconds of the call are saved beside the logs.").to_owned(),
        };
        let start = form::row(ui, t!("Diagnostic Audio Recording"), &note, |ui| form::button_enabled(ui, t!("Record 20 s of what you hear"), Kind::Secondary, app.voice.connected && recording.is_none()).clicked());
        if start {
            ui.data_mut(|d| d.insert_temp(id, now));
            app.commands.send(crate::backend::Command::Voice(crate::voice::VoiceCommand::Record));
        }
        if form::row(ui, t!("Open Logs Folder"), "", |ui| form::button(ui, t!("Open"), Kind::Secondary).clicked()) {
            let _ = open::that_detached(crate::settings::config_dir().join("logs"));
        }
        if form::row(ui, t!("Reset all Voice & Video settings"), t!("Return to the default set of Voice & Video settings."), |ui| form::button(ui, t!("Reset"), Kind::Danger).clicked()) {
            let defaults = crate::settings::Settings::default();
            let s = &mut app.settings;
            (s.input_device, s.output_device, s.input_volume, s.output_volume) = (None, None, defaults.input_volume, defaults.output_volume);
            (s.input_sensitivity, s.automatic_sensitivity, s.noise_suppression, s.echo_cancellation) = (defaults.input_sensitivity, defaults.automatic_sensitivity, defaults.noise_suppression, defaults.echo_cancellation);
            (s.automatic_gain_control, s.advanced_voice_activity, s.push_to_talk) = (defaults.automatic_gain_control, defaults.advanced_voice_activity, defaults.push_to_talk);
        }
    });
}

/// Discord's shortcut recorder for push-to-talk: press "Record Keybind",
/// then any key or mouse button; Esc cancels.
fn push_to_talk_key(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let id = egui::Id::new("ptt-recording");
    let mut recording = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if recording {
        if let Some(vk) = crate::voice::pressed_key() {
            if vk != 0x1B {
                app.settings.push_to_talk_key = vk;
            }
            recording = false;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
    form::label(ui, t!("Push to Talk Keybind"));
    ui.horizontal(|ui| {
        let text = if recording { t!("Press a key…").to_owned() } else { crate::voice::key_name(app.settings.push_to_talk_key) };
        Frame::new().fill(p.rail).stroke(Stroke::new(1.0, if recording { p.red } else { p.border })).corner_radius(8).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
            ui.set_width(220.0);
            ui.label(RichText::new(text).color(if recording { p.red } else { p.strong }));
        });
        if form::button(ui, if recording { t!("Stop Recording") } else { t!("Record Keybind") }, if recording { Kind::Danger } else { Kind::Secondary }).clicked() {
            recording = !recording;
        }
    });
    ui.data_mut(|d| d.insert_temp(id, recording));
    ui.add_space(18.0);
}

/// Discord's advanced voice block: a header that opens it, with the gain
/// control, the advanced voice activity and the switch warning.
fn advanced_voice(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    let id = ui.id().with("advanced-voice");
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    Frame::new().fill(p.sidebar).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        ui.painter().text(row.left_center(), egui::Align2::LEFT_CENTER, t!("Advanced"), theme::strong(15.0), p.strong);
        let chevron = Rect::from_center_size(egui::pos2(row.right() - 16.0, row.center().y), Vec2::splat(18.0));
        if open {
            crate::icons::paint_chevron_up(ui, chevron, p.strong);
        } else {
            crate::icons::paint(ui, chevron, Icon::ChevronDown, p.strong);
        }
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(id, open));
        }
        if open {
            ui.add_space(12.0);
            form::switch_row(ui, &mut app.settings.automatic_gain_control, t!("Automatic Gain Control"), t!("Automatically adjust microphone volume to keep it clear and consistent."));
            let usable = app.settings.automatic_sensitivity && !app.settings.push_to_talk;
            ui.add_enabled_ui(usable, |ui| {
                form::switch_row(ui, &mut app.settings.advanced_voice_activity, t!("Advanced Voice Activity"), t!("Turning this off may help if your voice is not being detected by the automatic input sensitivity."));
            });
            form::switch_row(ui, &mut app.settings.switch_voice_warning, t!("Switch Voice Channel Warning"), t!("Show a confirmation prompt before switching to a different voice channel."));
        }
    });
    ui.add_space(18.0);
}

fn appearance(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    form::category(ui, t!("Theme"), "", |ui| {
        let palettes = app.palettes.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for (name, palette) in &palettes {
                let selected = app.settings.theme == *name;
                let (rect, response) = ui.allocate_exact_size(vec2(160.0, 104.0), Sense::click());
                preview(ui, rect, palette);
                let stroke = if selected { Stroke::new(3.0, p.accent) } else if response.hovered() { Stroke::new(2.0, p.muted) } else { Stroke::new(1.0, p.selected) };
                ui.painter().rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Outside);
                ui.painter().text(rect.center_bottom() + vec2(0.0, -12.0), egui::Align2::CENTER_CENTER, name, theme::strong(12.5), palette.strong);
                if response.clicked() {
                    app.settings.theme = name.clone();
                }
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if form::button(ui, t!("Open Themes Folder"), Kind::Secondary).clicked() {
                let dir = theme::themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = open::that_detached(dir);
            }
            if form::button(ui, t!("Reload Themes"), Kind::Secondary).clicked() {
                app.palettes = theme::catalog();
            }
        });
    });
    // CordFast's "pro" customization: the accent over any theme, and every
    // colour of the theme in use saved as a theme of its own.
    form::category(ui, t!("Customize"), t!("Make any theme your own: change its colors and save it as a new theme."), |ui| {
        let mut custom = app.settings.accent.is_some();
        if form::switch_row(ui, &mut custom, t!("Use My Own Accent Color"), "") {
            app.settings.accent = custom.then_some([0x58, 0x65, 0xf2]);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for preset in [[0x58, 0x65, 0xf2], [0xeb, 0x45, 0x9e], [0x23, 0xa5, 0x5a], [0xf0, 0xb2, 0x32], [0xed, 0x42, 0x45], [0x00, 0xa8, 0xfc], [0x9b, 0x59, 0xb6]] {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                ui.painter().circle_filled(rect.center(), 13.0, Color32::from_rgb(preset[0], preset[1], preset[2]));
                if app.settings.accent == Some(preset) {
                    ui.painter().circle_stroke(rect.center(), 13.0, Stroke::new(2.0, p.strong));
                }
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.settings.accent = Some(preset);
                }
            }
            if let Some(accent) = &mut app.settings.accent {
                ui.color_edit_button_srgb(accent);
            }
        });
        ui.add_space(16.0);
        theme_editor(app, ui);
    });
    form::category(ui, t!("Messages"), "", |ui| {
        field_title(ui, t!("Show images, videos, and lolcats..."), "");
        account_switch(app, ui, pre::INLINE_LINKS, false, t!("When posted as links to chat"), "");
        account_switch(app, ui, pre::INLINE_UPLOADS, false, t!("When uploaded directly to Discord"), "");
        account_switch(app, ui, pre::RENDER_EMBEDS, false, t!("Show embeds and link previews"), "");
        account_switch(app, ui, pre::RENDER_REACTIONS, false, t!("Show emoji reactions"), "");
        let options = [t!("On click").to_owned(), t!("Always").to_owned(), t!("On servers I moderate").to_owned()];
        let values = ["ON_CLICK", "ALWAYS", "IF_MODERATOR"];
        let current = app.preloaded.text(pre::RENDER_SPOILERS).and_then(|v| values.iter().position(|x| *x == v)).unwrap_or(0);
        if let Some(i) = form::row(ui, t!("Show spoiler content"), "", |ui| form::select(ui, "spoilers", &options[current], &options, 220.0)) {
            app.set_preloaded_text(pre::RENDER_SPOILERS, values[i]);
        }
        account_switch(app, ui, pre::THREAD_SPLIT_VIEW, false, t!("Open threads in split view"), "");
        form::switch_row(ui, &mut app.settings.show_avatars, t!("Show user avatars"), "");
    });
    form::category(ui, t!("Chat Box"), "", |ui| {
        account_switch(app, ui, pre::CONVERT_EMOTICONS, false, t!("Automatically convert emoticons in your messages to emoji"), t!("For example, when you type :) Discord will convert it to 🙂"));
        account_switch(app, ui, pre::STICKERS_IN_AUTOCOMPLETE, false, t!("Show stickers in autocomplete results"), "");
        field_title(ui, t!("Expression picker"), "");
        form::switch_row(ui, &mut app.settings.emoji_button, t!("Show emoji button"), "");
        form::switch_row(ui, &mut app.settings.gif_button, t!("Show GIF button"), "");
        form::switch_row(ui, &mut app.settings.sticker_button, t!("Show sticker button"), "");
    });
}

/// Every colour of the theme in use, each with its picker; "Save as New
/// Theme" writes them to the themes folder as a theme of their own.
fn theme_editor(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::new("theme-editor");
    let mut palette: theme::Palette = ui.data(|d| d.get_temp(id)).unwrap_or_else(theme::p);
    let mut name: String = ui.data(|d| d.get_temp(id.with("name"))).unwrap_or_default();
    let fields: [(&str, &mut Color32); 13] = [
        (n!("Server List"), &mut palette.rail),
        (n!("Channel List"), &mut palette.sidebar),
        (n!("Chat"), &mut palette.chat),
        (n!("Message Box"), &mut palette.composer),
        (n!("Hover"), &mut palette.hover),
        (n!("Selected"), &mut palette.selected),
        (n!("Text"), &mut palette.text),
        (n!("Headings"), &mut palette.strong),
        (n!("Muted Text"), &mut palette.muted),
        (n!("Accent"), &mut palette.accent),
        (n!("Links"), &mut palette.link),
        (n!("Mentions"), &mut palette.mention),
        (n!("Borders"), &mut palette.border),
    ];
    egui::Grid::new("theme-colors").num_columns(4).spacing(vec2(24.0, 10.0)).show(ui, |ui| {
        for (i, (label, color)) in fields.into_iter().enumerate() {
            ui.color_edit_button_srgba(color);
            ui.label(RichText::new(crate::i18n::tr(label)).color(theme::p().text));
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        form::input(ui, &mut name, t!("Theme name"), 240.0);
        if form::button_enabled(ui, t!("Save as New Theme"), Kind::Primary, !name.trim().is_empty()).clicked() {
            match theme::save(name.trim(), &palette) {
                Ok(()) => {
                    app.palettes = theme::catalog();
                    app.settings.theme = name.trim().to_owned();
                    app.notify(t!("Theme saved."));
                }
                Err(error) => app.notify(error.to_string()),
            }
        }
        if form::button(ui, t!("Reset"), Kind::Link).clicked() {
            palette = theme::p();
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(id, palette);
        d.insert_temp(id.with("name"), name);
    });
}

/// A miniature of the window in a palette's colours.
fn preview(ui: &egui::Ui, rect: Rect, palette: &theme::Palette) {
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, palette.chat);
    let rail = Rect::from_min_size(rect.min, vec2(22.0, rect.height()));
    painter.rect_filled(rail, egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, palette.rail);
    for i in 0..3 {
        painter.circle_filled(rail.center_top() + vec2(0.0, 14.0 + i as f32 * 20.0), 7.0, if i == 0 { palette.accent } else { palette.selected });
    }
    let side = Rect::from_min_size(rect.min + vec2(22.0, 0.0), vec2(40.0, rect.height()));
    painter.rect_filled(side, 0.0, palette.sidebar);
    for i in 0..4 {
        painter.rect_filled(Rect::from_min_size(side.min + vec2(5.0, 10.0 + i as f32 * 12.0), vec2(28.0, 5.0)), 2.0, if i == 1 { palette.strong } else { palette.muted });
    }
    for i in 0..3 {
        let y = rect.top() + 12.0 + i as f32 * 20.0;
        painter.circle_filled(egui::pos2(rect.left() + 74.0, y + 4.0), 5.0, palette.selected);
        painter.rect_filled(Rect::from_min_size(egui::pos2(rect.left() + 84.0, y), vec2(60.0, 4.0)), 2.0, palette.strong);
        painter.rect_filled(Rect::from_min_size(egui::pos2(rect.left() + 84.0, y + 7.0), vec2(48.0, 4.0)), 2.0, palette.text.linear_multiply(0.6));
    }
}

fn accessibility(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("Visual Density"), "", |ui| {
        field_title(ui, t!("Chat Message Display"), t!("Change the appearance of chat messages."));
        let compact = app.settings.compact;
        if let Some(value) = form::radio_cards(ui, compact, &[(false, t!("Default"), ""), (true, t!("Compact"), "")]) {
            app.settings.compact = value;
            app.set_preloaded(pre::COMPACT, u64::from(value));
        }
        ui.add_space(12.0);
        form::slider_row(ui, t!("Zoom level"), &t!("Adjust the size of the interface. You can also change this with {modKey} +/-.", modKey = "Ctrl"), &mut app.settings.zoom, 0.6..=2.0, |v| format!("{:.0}%", v * 100.0));
    });
    form::category(ui, t!("Color & Contrast"), "", |ui| {
        let options = [t!("In names").to_owned(), t!("Next to names").to_owned(), t!("Don't show role colors").to_owned()];
        let current = usize::from(app.settings.role_colors.min(2));
        if let Some(i) = form::row(ui, t!("Role Colors"), t!("Choose how you would like role colors to be displayed."), |ui| form::select(ui, "role-colors", &options[current], &options, 220.0)) {
            app.settings.role_colors = i as u8;
        }
    });
    form::category(ui, t!("Reduced Motion"), "", |ui| {
        account_switch(app, ui, pre::GIF_AUTO_PLAY, false, t!("Play GIFs when Discord is focused"), "");
        account_switch(app, ui, pre::ANIMATE_EMOJI, false, t!("Play animated emoji"), "");
        field_title(ui, t!("Play sticker animations"), "");
        account_radio(app, ui, pre::ANIMATE_STICKERS, &[(0, t!("Always animate"), ""), (1, t!("Animate on interaction"), t!("On the desktop client, stickers will animate on hover or focus. On mobile clients, stickers will animate on long-press.")), (2, t!("Never animate"), "")]);
    });
    form::category(ui, t!("Audio & Screen Reader"), "", |ui| {
        account_switch(app, ui, pre::IMAGE_DESCRIPTIONS, false, t!("Show image descriptions"), t!("Image descriptions are used to describe images for screen readers."));
    });
}

fn system(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("General"), "", |ui| {
        let state = *app.autostart.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match state {
            None => {
                ui.spinner();
            }
            Some(fastframe_autostart::State::Unsupported) => form::note(ui, t!("Unavailable on this system.")),
            Some(state) => {
                let current = state.start();
                let mut on = current.is_some();
                let mut hidden = current == Some(fastframe_autostart::Start::Hidden);
                let before = (on, hidden);
                form::switch_row(ui, &mut on, t!("Automatically open Discord when your computer starts up"), "");
                if on {
                    form::switch_row(ui, &mut hidden, t!("Start Discord minimized"), t!("Start Discord in the background, out of your way."));
                }
                if (on, hidden) != before {
                    let start = on.then_some(if hidden { fastframe_autostart::Start::Hidden } else { fastframe_autostart::Start::Window });
                    *app.autostart.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                    app.set_autostart(start);
                }
            }
        }
        form::switch_row(ui, &mut app.settings.keep_running_in_tray, t!("Minimize Discord to System Tray"), t!("Clicking X minimizes Discord instead of closing it completely."));
        let native = app.settings.native_titlebar;
        form::switch_row(ui, &mut app.settings.native_titlebar, t!("Use the System Title Bar"), t!("Windows' frame instead of Discord's bar."));
        if app.settings.native_titlebar != native {
            // The frame changes at once: the system's, or none under ours.
            fastframe_titlebar::set_enabled(!app.settings.native_titlebar);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Decorations(app.settings.native_titlebar));
        }
    });
    form::category(ui, t!("Custom Keybinds"), t!("Keybinds are custom keyboard shortcuts that let you perform specific actions in Discord without needing to click through menus."), |ui| keybinds(app, ui));
    form::category(ui, t!("Default Keybinds"), "", |ui| fixed_keybinds(ui));
}

fn keybinds(app: &mut App, ui: &mut egui::Ui) {
    use crate::keybinds::{ALL, Bind, SECTIONS, bind};
    let p = theme::p();
    // While recording, the next key with its modifiers becomes the bind;
    // Esc cancels.
    if let Some(id) = app.recording {
        let captured = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, modifiers)) = captured {
            if !(key == egui::Key::Escape && modifiers.is_none()) {
                app.settings.keybinds.insert(id.to_owned(), Bind { modifiers, key }.text());
            }
            app.recording = None;
        }
    }
    for (section, shortcuts) in SECTIONS {
        form::label(ui, crate::i18n::tr(section));
        for shortcut in *shortcuts {
            let Some((_, id, label, default)) = ALL.iter().find(|(s, ..)| s == shortcut) else { continue };
            let current = bind(&app.settings, *shortcut);
            let recording = app.recording == Some(*id);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, p.selected));
            // From the right: "Default", "Record Keybind", then the keys.
            let reset = Rect::from_min_size(egui::pos2(rect.right() - 76.0, rect.center().y - 16.0), vec2(76.0, 32.0));
            let record = Rect::from_min_size(egui::pos2(reset.left() - 130.0, reset.top()), vec2(122.0, 32.0));
            let keys = Rect::from_min_size(egui::pos2(record.left() - 208.0, reset.top()), vec2(200.0, 32.0));
            super::paint_truncated(ui, Rect::from_min_max(rect.left_top(), egui::pos2(keys.left() - 12.0, rect.bottom())), crate::i18n::tr(label), egui::FontId::proportional(15.0), p.strong);
            ui.painter().rect(keys, 8.0, p.rail, Stroke::new(1.0, if recording { p.red } else { p.border }), egui::StrokeKind::Inside);
            let text = if recording { t!("Press the keys…").to_owned() } else { current.map_or_else(|| t!("None").to_owned(), Bind::label) };
            ui.painter().text(keys.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::monospace(13.5), if recording { p.red } else { p.text });
            let button = if recording { t!("Cancel") } else { t!("Record Keybind") };
            if ui.put(record, egui::Button::new(RichText::new(button).color(Color32::WHITE)).fill(if recording { p.red } else { p.selected }).corner_radius(8)).clicked() {
                app.recording = if recording { None } else { Some(*id) };
            }
            let differs = app.settings.keybinds.contains_key(*id);
            let default_label = Bind::parse(default).map_or_else(|| (*default).to_owned(), Bind::label);
            let reset_button = egui::Button::new(RichText::new(t!("Default")).size(13.0).color(if differs { p.text } else { p.faint })).wrap_mode(egui::TextWrapMode::Extend);
            if ui.put(reset, reset_button).on_hover_text(t!("Back to {keys}", keys = default_label)).clicked() {
                app.settings.keybinds.remove(*id);
            }
        }
        ui.add_space(16.0);
    }
    if app.recording.is_some() {
        ui.ctx().request_repaint();
    }
}

fn fixed_keybinds(ui: &mut egui::Ui) {
    let p = theme::p();
    for (keys, what) in crate::keybinds::FIXED {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
        let galley = ui.painter().layout_no_wrap(crate::i18n::tr(keys).to_owned(), egui::FontId::monospace(13.0), p.text);
        let chip = Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 12.0), vec2(galley.size().x + 16.0, 24.0));
        ui.painter().rect(chip, 4.0, p.rail, Stroke::new(1.0, p.selected), egui::StrokeKind::Inside);
        ui.painter().galley(chip.center() - galley.size() / 2.0, galley, p.text);
        ui.painter().text(egui::pos2(rect.left() + 220.0, rect.center().y), egui::Align2::LEFT_CENTER, crate::i18n::tr(what), egui::FontId::proportional(14.0), p.muted);
    }
}

/// Discord's language list: each language in its own words, then its name
/// in the interface's language and its flag; then the time format.
fn language(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    form::category(ui, t!("Language"), "", |ui| {
        let current = crate::i18n::current();
        for (locale, tag, native, name, flag) in crate::i18n::ALL {
            let chosen = *locale == current;
            let response = form::radio_card(ui, chosen, native, "");
            let rect = response.rect;
            let flag_rect = Rect::from_center_size(egui::pos2(rect.right() - 28.0, rect.center().y), Vec2::splat(24.0));
            super::markdown::paint_twemoji(app, ui, flag, flag_rect);
            ui.painter().text(egui::pos2(flag_rect.left() - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, crate::i18n::tr(name), egui::FontId::proportional(14.0), p.muted);
            if response.clicked() && !chosen {
                crate::i18n::set(*locale);
                app.settings.language = Some((*tag).to_owned());
            }
        }
    });
    form::category(ui, "", "", |ui| {
        field_title(ui, t!("Time format"), "");
        account_radio(app, ui, pre::HOUR_CYCLE, &[(0, t!("Auto"), ""), (1, t!("12-hour"), ""), (2, t!("24-hour"), "")]);
    });
}

// ---- Games & Apps ---------------------------------------------------------------------------

fn games(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    form::category(ui, t!("Current Game"), "", |ui| {
        match &app.activity {
            Some(game) => {
                item_row(ui, game, t!("Playing"), None);
            }
            None => {
                item_row(ui, t!("No game detected"), t!("What are you playing?!"), None);
            }
        }
        ui.add_space(12.0);
        let mut on = app.rich_presence();
        form::switch_row(ui, &mut on, t!("Share my activity"), t!("Games found running and apps that report through Discord's local pipe, as rich presence."));
        app.settings.plugins.insert("rich-presence".into(), on);
    });
    form::category(ui, t!("Added Games"), "", |ui| {
        let games = app.settings.played_games.clone();
        let playing = app.activity.clone();
        for game in &games {
            let detail = if playing.as_deref() == Some(game.name.as_str()) { t!("Playing").to_owned() } else { t!("Last played {when}", when = crate::text::time(game.last_played)) };
            item_row(ui, &game.name, &detail, None);
        }
        if games.is_empty() {
            form::note(ui, t!("No game detected"));
        }
    });
    let _ = p;
}

fn activity_privacy(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("What Activity I Share"), "", |ui| {
        account_switch(app, ui, pre::SHOW_CURRENT_GAME, false, t!("Share my activity"), t!("Show the game or app you're using as your status."));
        account_switch(app, ui, pre::NOTIFY_FRIENDS_ON_COME_ONLINE, false, t!("Share when I come online"), t!("Allow friends to receive a push notification when you come online."));
        account_switch(app, ui, pre::NOTIFY_MEMBERS_ON_GO_LIVE, false, t!("Share when I start streaming"), t!("Allow members that you interact with the most to get a notification when you start streaming in small servers."));
    });
    form::category(ui, t!("Where I Share Activity"), "", |ui| {
        field_title(ui, t!("Automatically share my activity when joining a server"), t!("Your activity is always shared with your friends."));
        account_radio(app, ui, pre::ACTIVITY_IN_GUILDS, &[(1, t!("Off"), ""), (2, t!("On for large servers"), ""), (3, t!("On"), "")]);
    });
    form::category(ui, t!("Activity Joining"), t!("Some games allow users to join the game you're playing when viewing your activity."), |ui| {
        account_switch(app, ui, pre::ACTIVITY_JOIN_FRIENDS, false, t!("Allow friends to join my game"), t!("Allows friends to join without sending a request."));
        account_switch(app, ui, pre::ACTIVITY_JOIN_VOICE, false, t!("Allow voice channel participants to join my game"), t!("Allow anyone in a shared voice channel to join my game without sending a request. This feature only works in non-Community enabled servers."));
    });
}

fn connected_apps(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, t!("Connections"), t!("These are accounts from other platforms. Connecting them here can unlock new features on Discord, like the ability to share what you're currently listening to, get paid safely and securely, and more."), |ui| {
        // Connecting happens on the other service's site, in the browser.
        const SERVICES: [&str; 10] = ["battlenet", "epicgames", "github", "leagueoflegends", "reddit", "riotgames", "spotify", "steam", "twitch", "youtube"];
        let names: Vec<String> = SERVICES.iter().map(|name| (*name).to_owned()).collect();
        form::label(ui, t!("Add Connection"));
        if let Some(picked) = form::select(ui, "add-connection", t!("Add Connection"), &names, 240.0) {
            app.commands.send(crate::admin::get("connection-authorize", format!("/connections/{}/authorize", SERVICES[picked])));
        }
        ui.add_space(10.0);
        let Some(list) = loaded(app, ui, "settings:connections").and_then(Value::as_array).cloned() else { return };
        if list.is_empty() {
            form::note(ui, t!("You haven't connected any accounts."));
        }
        for c in &list {
            let kind = c.get("type").and_then(Value::as_str).unwrap_or_default();
            let name = c.get("name").and_then(Value::as_str).unwrap_or_default();
            let id = c.get("id").and_then(Value::as_str).unwrap_or_default();
            let mut visible = c.get("visibility").and_then(Value::as_u64) == Some(1);
            if form::switch_row(ui, &mut visible, t!("Display on profile"), name) {
                app.commands.send(crate::backend::Command::Api { key: "connection-removed".into(), method: "PATCH", path: format!("/users/@me/connections/{kind}/{id}"), body: Some(serde_json::json!({ "visibility": u8::from(visible) })) });
                app.commands.send(crate::admin::get("settings:connections", "/users/@me/connections".into()));
            }
            if item_row(ui, name, kind, Some((t!("Remove"), Kind::Secondary))) {
                app.commands.send(crate::backend::Command::Api { key: "connection-removed".into(), method: "DELETE", path: format!("/users/@me/connections/{kind}/{id}"), body: None });
                app.commands.send(crate::admin::get("settings:connections", "/users/@me/connections".into()));
            }
        }
    });
    form::category(ui, t!("Authorized Apps"), t!("These include third-party services you've given permission to access your Discord account. You can review what each app can do and revoke access anytime."), |ui| {
        let Some(list) = loaded(app, ui, "settings:apps").and_then(Value::as_array).cloned() else { return };
        if list.is_empty() {
            form::note(ui, t!("You haven't authorized any apps."));
        }
        for token in &list {
            let name = token.pointer("/application/name").and_then(Value::as_str).unwrap_or_default();
            let scopes = token.get("scopes").and_then(Value::as_array).map(|s| s.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            let id = token.get("id").and_then(Value::as_str).unwrap_or_default();
            if item_row(ui, name, &scopes, Some((t!("Deauthorize"), Kind::Danger))) {
                app.commands.send(crate::backend::Command::Api { key: "app-removed".into(), method: "DELETE", path: format!("/oauth2/tokens/{id}"), body: None });
                app.commands.send(crate::admin::get("settings:apps", "/oauth2/tokens".into()));
            }
        }
    });
}

// ---- Other ------------------------------------------------------------------------------------

fn developer(app: &mut App, ui: &mut egui::Ui) {
    form::category(ui, "", "", |ui| {
        account_switch(app, ui, pre::DEVELOPER_MODE, false, t!("Developer Mode"), t!("Developer Mode exposes context menu items helpful for people writing apps using the Discord API."));
    });
}

/// Vencord's plugin page: cards two to a row with the name, a switch and
/// what the plugin does; the required ones first, locked on.
fn plugins_page(app: &mut App, ui: &mut egui::Ui) {
    for required in [true, false] {
        ui.label(RichText::new(if required { t!("Required Plugins") } else { t!("Plugins") }).font(theme::strong(18.0)).color(theme::p().strong));
        ui.add_space(12.0);
        let list: Vec<_> = plugins::ALL.iter().filter(|p| p.info().required == required).collect();
        let width = (ui.available_width() - 16.0) / 2.0;
        for pair in list.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for plugin in pair {
                    plugin_card(app, ui, **plugin, width);
                }
            });
            ui.add_space(16.0);
        }
        ui.add_space(16.0);
    }
}

fn plugin_card(app: &mut App, ui: &mut egui::Ui, plugin: plugins::Plugin, width: f32) {
    let p = theme::p();
    let info = plugin.info();
    Frame::new().fill(p.sidebar).stroke(Stroke::new(1.0, p.border)).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(width - 26.0);
        ui.set_min_height(72.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let name_width = ui.available_width() - 56.0;
                let (rect, _) = ui.allocate_exact_size(vec2(name_width, 24.0), Sense::hover());
                super::paint_truncated(ui, rect, info.name, theme::strong(15.0), p.strong);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = app.settings.plugin(plugin);
                    if info.required {
                        // Locked on, as Vencord's required plugins.
                        ui.add_enabled_ui(false, |ui| form::switch(ui, &mut on));
                    } else if form::switch(ui, &mut on).changed() {
                        app.settings.plugins.insert(info.id.to_owned(), on);
                    }
                });
            });
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(crate::i18n::tr(info.description)).size(13.5).color(p.muted)).wrap());
        });
    });
}

fn about(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::p();
    form::category(ui, &format!("CordFast {}", env!("CARGO_PKG_VERSION")), t!("A native, light and fast Discord."), |ui| {
        let mut install = false;
        match &*app.updates.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
            crate::updates::State::Idle => form::note(ui, t!("You're on the latest version.")),
            crate::updates::State::Downloading { version, percent } => form::note(ui, &t!("Downloading version {version}… {percent}%", version = version, percent = percent)),
            crate::updates::State::Ready { version, .. } => {
                ui.label(RichText::new(t!("Version {version} is ready.", version = version)).color(p.green));
                ui.add_space(8.0);
                install = form::button(ui, t!("Restart and Update"), Kind::Success).clicked();
            }
            crate::updates::State::Unsupported { version, url } => {
                form::note(ui, &t!("Version {version} is out, but this copy does not update itself.", version = version));
                if form::button(ui, t!("Download on GitHub"), Kind::Link).clicked() {
                    let _ = open::that_detached(url);
                }
            }
            crate::updates::State::Failed(error) => {
                ui.label(RichText::new(t!("The update failed: {error}", error = error)).color(p.red));
            }
        }
        if install {
            app.actions.push(Action::InstallUpdate);
        }
    });
    ui.label(RichText::new(t!("Third-party clients break Discord's terms of service. Use at your own risk.")).size(12.5).color(p.warning));
}
