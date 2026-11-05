//! `CORDFAST_SERVERTEST=<file>`: tries the settings and chat calls against the
//! real API, in a server made for it and deleted at the end, with the bodies
//! the pages send. Each step writes OK or FAIL and the answer's field names to
//! the file, never values or the token. `<file>.hold` existing keeps the server
//! until it is removed (at most two minutes), to look at it in the app.

use std::time::Duration;

use serde_json::{Value, json};

use crate::backend::Upload;
use crate::discord::rest::{Api, percent_encode};

const NAME: &str = "CordFast test (safe to delete)";

fn shape(value: &Value) -> String {
    match value {
        Value::Array(list) => format!("[{}] {}", list.len(), list.first().map(shape).unwrap_or_default()),
        Value::Object(map) => {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            format!("{{{}}}", keys.join(", "))
        }
        Value::String(_) => "string".into(),
        Value::Number(_) => "number".into(),
        Value::Bool(_) => "bool".into(),
        Value::Null => "null".into(),
    }
}

struct Run {
    api: Api,
    lines: Vec<String>,
    out: String,
}

impl Run {
    async fn step(&mut self, label: &str, method: &str, path: String, body: Option<Value>) -> Option<Value> {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let result = self.api.raw(method, &path, body, None).await;
        let line = match &result {
            Ok(value) => format!("OK   {label}: {}", shape(value)),
            Err(error) => format!("FAIL {label}: {error:#}"),
        };
        self.note(line);
        result.ok()
    }

    fn note(&mut self, line: String) {
        self.lines.push(line);
        // Kept as it goes, so a stop midway still says where the server is.
        let _ = std::fs::write(&self.out, self.lines.join("\n"));
    }
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

pub fn run(out: &str) {
    let Some(token) = crate::backend::saved_token() else {
        let _ = std::fs::write(out, "no saved session\n");
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("unable to start the async runtime");
    runtime.block_on(async {
        let mut run = Run { api: Api::new(crate::discord::rest::http(), token), lines: Vec::new(), out: out.to_owned() };
        // CORDFAST_SERVERTEST_ON=<id>: only the rechecks, in a server that already exists.
        if let Ok(g) = std::env::var("CORDFAST_SERVERTEST_ON") {
            recheck(&mut run, &g).await;
            return;
        }
        let Some(guild) = run.step("create server", "POST", "/guilds".into(), Some(json!({ "name": NAME }))).await else { return };
        let g = text(&guild, "id");
        run.note(format!("NOTE server id {g}"));
        tests(&mut run, &g, out).await;
        // Always, whatever failed: the server was made here and only for this.
        run.step("delete server", "POST", format!("/guilds/{g}/delete"), Some(json!({}))).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        let gone = run.api.raw("GET", &format!("/guilds/{g}"), None, None).await.is_err();
        run.note(format!("NOTE server gone: {gone}"));
    });
}

async fn tests(run: &mut Run, g: &str, out: &str) {
    let channels = run.step("list channels", "GET", format!("/guilds/{g}/channels"), None).await.unwrap_or_default();
    let list = channels.as_array().cloned().unwrap_or_default();
    let t = list.iter().find(|c| c.get("type").and_then(Value::as_u64) == Some(0)).map(|c| text(c, "id")).unwrap_or_default();
    let v = run.step("create voice channel", "POST", format!("/guilds/{g}/channels"), Some(json!({ "name": "voice", "type": 2 }))).await.map(|c| text(&c, "id")).unwrap_or_default();
    let me = run.step("me", "GET", "/users/@me".into(), None).await.map(|u| text(&u, "id")).unwrap_or_default();

    // ---- channel overwrites, as guild_more::overwrites sends them
    run.step("overwrite deny", "PUT", format!("/channels/{t}/permissions/{g}"), Some(json!({ "type": 0, "allow": "0", "deny": (1u64 << 11).to_string() }))).await;
    if let Some(channel) = run.step("read overwrite", "GET", format!("/channels/{t}"), None).await {
        let found = channel.get("permission_overwrites").and_then(Value::as_array).is_some_and(|o| o.iter().any(|o| text(o, "deny") == (1u64 << 11).to_string()));
        run.note(format!("NOTE overwrite stored: {found}"));
    }
    run.step("overwrite clear", "PUT", format!("/channels/{t}/permissions/{g}"), Some(json!({ "type": 0, "allow": "0", "deny": "0" }))).await;

    // ---- settings pages
    if let Some(hook) = run.step("webhook create", "POST", format!("/channels/{t}/webhooks"), Some(json!({ "name": "Webhook" }))).await {
        run.step("webhook list", "GET", format!("/guilds/{g}/webhooks"), None).await;
        run.step("webhook delete", "DELETE", format!("/webhooks/{}", text(&hook, "id")), None).await;
    }
    if let Some(rule) = run.step("automod create", "POST", format!("/guilds/{g}/auto-moderation/rules"), Some(json!({ "name": "test", "event_type": 1, "trigger_type": 1, "trigger_metadata": { "keyword_filter": ["cordfasttest"] }, "actions": [{ "type": 1 }], "enabled": true }))).await {
        let id = text(&rule, "id");
        run.step("automod off", "PATCH", format!("/guilds/{g}/auto-moderation/rules/{id}"), Some(json!({ "enabled": false }))).await;
        run.step("automod delete", "DELETE", format!("/guilds/{g}/auto-moderation/rules/{id}"), None).await;
    }
    sound(run, g).await;
    sticker(run, g).await;
    if let Some(template) = run.step("template create", "POST", format!("/guilds/{g}/templates"), Some(json!({ "name": "test" }))).await {
        let code = text(&template, "code");
        run.step("template sync", "PUT", format!("/guilds/{g}/templates/{code}"), None).await;
        run.step("template delete", "DELETE", format!("/guilds/{g}/templates/{code}"), None).await;
    }
    run.step("widget on", "PATCH", format!("/guilds/{g}/widget"), Some(json!({ "enabled": true }))).await;
    run.step("prune count", "GET", format!("/guilds/{g}/prune?days=30"), None).await;
    run.step("prune", "POST", format!("/guilds/{g}/prune"), Some(json!({ "days": 30, "compute_prune_count": false }))).await;

    // ---- messages
    let nonce = crate::discord::model::now_snowflake().to_string();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let sent = run.api.send_message(t.parse().unwrap_or(0), "cordfast test https://example.com", &nonce, None, false, true, Vec::new()).await;
    run.note(format!("{} silent message: {:?}", if sent.is_ok() { "OK  " } else { "FAIL" }, sent.err().map(|e| format!("{e:#}"))));
    let messages = run.step("read messages", "GET", format!("/channels/{t}/messages?limit=5"), None).await.unwrap_or_default();
    let first = messages.get(0).cloned().unwrap_or_default();
    let m = text(&first, "id");
    run.note(format!("NOTE silent flag: {}", first.get("flags").and_then(Value::as_u64).is_some_and(|f| f & 4096 != 0)));
    let emoji = percent_encode("👍");
    run.step("react", "PUT", format!("/channels/{t}/messages/{m}/reactions/{emoji}/@me?location=Message&type=0"), None).await;
    run.step("who reacted", "GET", format!("/channels/{t}/messages/{m}/reactions/{emoji}?limit=100&type=0"), None).await;
    run.step("remove one reaction", "DELETE", format!("/channels/{t}/messages/{m}/reactions/{emoji}/0/{me}"), None).await;
    run.step("react again", "PUT", format!("/channels/{t}/messages/{m}/reactions/{emoji}/@me?location=Message&type=0"), None).await;
    run.step("remove all reactions", "DELETE", format!("/channels/{t}/messages/{m}/reactions"), None).await;
    let flags = first.get("flags").and_then(Value::as_u64).unwrap_or(0);
    run.step("remove embeds", "PATCH", format!("/channels/{t}/messages/{m}"), Some(json!({ "flags": flags | 4 }))).await;
    let saved = format!("/users/@me/saved-messages/{t}/{m}");
    run.step("bookmark", "PUT", saved.clone(), Some(json!({ "due_at": null }))).await;
    if let Some(ids) = run.step("bookmark ids", "GET", "/users/@me/saved-messages?bookmark_ids=true".into(), None).await {
        let first = ids.pointer("/bookmark_ids/0").and_then(Value::as_str).unwrap_or_default();
        run.note(format!("NOTE bookmark id is the message's: {}, with a dash: {}", first == m, first.contains('-')));
    }
    run.step("bookmark off", "DELETE", saved.clone(), None).await;
    let due = jiff::Timestamp::now().checked_add(jiff::SignedDuration::from_hours(1)).map(|at| at.to_string()).unwrap_or_default();
    run.step("reminder", "PUT", saved.clone(), Some(json!({ "due_at": due }))).await;
    if let Some(all) = run.step("reminder list", "GET", "/users/@me/saved-messages".into(), None).await {
        run.note(format!("NOTE reminder has its message: {}", all.pointer("/reminders/0/message").is_some()));
    }
    run.step("reminder off", "DELETE", saved, None).await;
    if let Some(poll) = run.step("poll", "POST", format!("/channels/{t}/messages"), Some(json!({ "content": "", "poll": { "question": { "text": "Test?" }, "answers": [{ "poll_media": { "text": "Yes" } }, { "poll_media": { "text": "No" } }], "duration": 24, "allow_multiselect": false, "layout_type": 1 } }))).await {
        let id = text(&poll, "id");
        run.step("poll vote", "PUT", format!("/channels/{t}/polls/{id}/answers/@me"), Some(json!({ "answer_ids": ["1"] }))).await;
    }
    upload(run, &t, 64 * 1024, "small").await;
    upload(run, &t, 9 * 1024 * 1024, "large").await;

    // ---- threads
    if let Some(thread) = run.step("thread create", "POST", format!("/channels/{t}/threads"), Some(json!({ "name": "test", "type": 11, "auto_archive_duration": 1440 }))).await {
        let id = text(&thread, "id");
        run.step("thread leave", "DELETE", format!("/channels/{id}/thread-members/@me?location=Context%20Menu"), None).await;
        run.step("thread join", "PUT", format!("/channels/{id}/thread-members/@me?location=Context%20Menu"), None).await;
        run.step("thread lock", "PATCH", format!("/channels/{id}"), Some(json!({ "locked": true }))).await;
        run.step("thread unlock", "PATCH", format!("/channels/{id}"), Some(json!({ "locked": false }))).await;
        run.step("thread close", "PATCH", format!("/channels/{id}"), Some(json!({ "archived": true }))).await;
        run.step("archived list", "GET", format!("/channels/{t}/threads/archived/public?limit=25"), None).await;
    }

    // ---- events
    let start = jiff::Timestamp::now().checked_add(jiff::SignedDuration::from_hours(24)).map(|at| at.to_string()).unwrap_or_default();
    let end = jiff::Timestamp::now().checked_add(jiff::SignedDuration::from_hours(26)).map(|at| at.to_string()).unwrap_or_default();
    if let Some(event) = run.step("event elsewhere", "POST", format!("/guilds/{g}/scheduled-events"), Some(json!({ "name": "test", "description": "", "privacy_level": 2, "scheduled_start_time": start, "entity_type": 3, "entity_metadata": { "location": "somewhere" }, "scheduled_end_time": end }))).await {
        let id = text(&event, "id");
        run.step("event interested", "PUT", format!("/guilds/{g}/scheduled-events/{id}/users/@me"), Some(json!({ "response": 1 }))).await;
        run.step("event list", "GET", format!("/guilds/{g}/scheduled-events?with_user_count=true"), None).await;
        run.step("event not interested", "PUT", format!("/guilds/{g}/scheduled-events/{id}/users/@me"), Some(json!({ "response": 0 }))).await;
        run.step("event cancel", "PATCH", format!("/guilds/{g}/scheduled-events/{id}"), Some(json!({ "status": 4 }))).await;
    }
    if let Some(event) = run.step("event in voice", "POST", format!("/guilds/{g}/scheduled-events"), Some(json!({ "name": "test voice", "description": "", "privacy_level": 2, "scheduled_start_time": start, "entity_type": 2, "channel_id": v }))).await {
        let id = text(&event, "id");
        run.step("event start", "PATCH", format!("/guilds/{g}/scheduled-events/{id}"), Some(json!({ "status": 2 }))).await;
        run.step("event end", "PATCH", format!("/guilds/{g}/scheduled-events/{id}"), Some(json!({ "status": 3 }))).await;
    }

    // ---- Community: announcements, forums, Stage, rules and onboarding
    let community = run.step("community on", "PATCH", format!("/guilds/{g}"), Some(json!({ "verification_level": 1, "explicit_content_filter": 2, "features": ["COMMUNITY"], "rules_channel_id": t, "public_updates_channel_id": t }))).await;
    if community.is_some() {
        run.step("verification form", "GET", format!("/guilds/{g}/member-verification?with_guild=false"), None).await;
        run.step("onboarding", "GET", format!("/guilds/{g}/onboarding"), None).await;
        if let Some(news) = run.step("announcement channel", "POST", format!("/guilds/{g}/channels"), Some(json!({ "name": "news", "type": 5 }))).await {
            let id = text(&news, "id");
            if let Some(message) = run.step("announcement", "POST", format!("/channels/{id}/messages"), Some(json!({ "content": "test" }))).await {
                run.step("publish", "POST", format!("/channels/{id}/messages/{}/crosspost", text(&message, "id")), None).await;
            }
        }
        if let Some(forum) = run.step("forum channel", "POST", format!("/guilds/{g}/channels"), Some(json!({ "name": "forum", "type": 15 }))).await {
            let id = text(&forum, "id");
            run.step("forum post", "POST", format!("/channels/{id}/threads"), Some(json!({ "name": "test", "auto_archive_duration": 1440, "message": { "content": "test" } }))).await;
        }
        if let Some(stage) = run.step("stage channel", "POST", format!("/guilds/{g}/channels"), Some(json!({ "name": "stage", "type": 13 }))).await {
            let id = text(&stage, "id");
            if run.step("stage start", "POST", "/stage-instances".into(), Some(json!({ "channel_id": id, "topic": "test", "privacy_level": 2, "send_start_notification": false }))).await.is_some() {
                run.step("stage end", "DELETE", format!("/stage-instances/{id}"), None).await;
            }
        }
    }

    // Keeps the server while the app looks at it.
    let hold = format!("{out}.hold");
    let since = std::time::Instant::now();
    while std::path::Path::new(&hold).exists() && since.elapsed() < Duration::from_secs(120) {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn recheck(run: &mut Run, g: &str) {
    sticker(run, g).await;
    let channels = run.step("list channels", "GET", format!("/guilds/{g}/channels"), None).await.unwrap_or_default();
    let t = channels.as_array().into_iter().flatten().find(|c| c.get("type").and_then(Value::as_u64) == Some(0)).map(|c| text(c, "id")).unwrap_or_default();
    let messages = run.step("read messages", "GET", format!("/channels/{t}/messages?limit=1"), None).await.unwrap_or_default();
    let m = messages.pointer("/0/id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let saved = format!("/users/@me/saved-messages/{t}/{m}");
    let due = jiff::Timestamp::now().checked_add(jiff::SignedDuration::from_hours(1)).map(|at| at.to_string()).unwrap_or_default();
    run.step("reminder", "PUT", saved.clone(), Some(json!({ "due_at": due }))).await;
    if let Some(all) = run.step("reminder list", "GET", "/users/@me/saved-messages".into(), None).await {
        run.note(format!("NOTE reminder: {} with message {}, due {}", shape(all.get("results").unwrap_or(&Value::Null)), all.pointer("/results/0/message").is_some(), all.pointer("/results/0/save_data/due_at").is_some_and(|d| !d.is_null())));
    }
    run.step("reminder off", "DELETE", saved.clone(), None).await;
    run.step("bookmark", "PUT", saved.clone(), Some(json!({ "due_at": null }))).await;
    if let Some(all) = run.step("list with a bookmark", "GET", "/users/@me/saved-messages".into(), None).await {
        run.note(format!("NOTE plain list holds bookmarks: {}", all.get("results").and_then(Value::as_array).is_some_and(|l| !l.is_empty())));
    }
    run.step("bookmark off", "DELETE", saved, None).await;
}

async fn sound(run: &mut Run, g: &str) {
    use base64::Engine;
    let dir = crate::settings::config_dir().join("sounds");
    let file = std::fs::read_dir(dir).ok().and_then(|files| files.flatten().find(|f| f.path().extension().is_some_and(|e| e == "mp3")));
    let Some(bytes) = file.and_then(|f| std::fs::read(f.path()).ok()) else {
        run.note("SKIP sound upload: no mp3 cached".into());
        return;
    };
    let data = format!("data:audio/mpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    if let Some(sound) = run.step("sound upload", "POST", format!("/guilds/{g}/soundboard-sounds"), Some(json!({ "name": "test", "sound": data, "volume": 1 }))).await {
        run.step("sound list", "GET", format!("/guilds/{g}/soundboard-sounds"), None).await;
        run.step("sound delete", "DELETE", format!("/guilds/{g}/soundboard-sounds/{}", text(&sound, "sound_id")), None).await;
    }
}

async fn sticker(run: &mut Run, g: &str) {
    let path = std::env::temp_dir().join("cordfast-test-sticker.png");
    if image::RgbaImage::from_pixel(320, 320, image::Rgba([88, 101, 242, 255])).save(&path).is_err() {
        run.note("SKIP sticker: no image".into());
        return;
    }
    tokio::time::sleep(Duration::from_millis(700)).await;
    let made = run.api.upload_sticker(g.parse().unwrap_or(0), "test", "👍", &path).await;
    let _ = std::fs::remove_file(&path);
    match made {
        Ok(sticker) => {
            run.note(format!("OK   sticker upload: {}", shape(&sticker)));
            run.step("sticker delete", "DELETE", format!("/guilds/{g}/stickers/{}", text(&sticker, "id")), None).await;
        }
        Err(error) => run.note(format!("FAIL sticker upload: {error:#}")),
    }
}

async fn upload(run: &mut Run, t: &str, size: usize, label: &str) {
    let path = std::env::temp_dir().join(format!("cordfast-test-{label}.bin"));
    if std::fs::write(&path, vec![7u8; size]).is_err() {
        run.note(format!("SKIP upload {label}"));
        return;
    }
    let nonce = crate::discord::model::now_snowflake().to_string();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let sent = run.api.send_message(t.parse().unwrap_or(0), "", &nonce, None, false, false, vec![Upload::new(path.clone())]).await;
    let _ = std::fs::remove_file(&path);
    run.note(format!("{} upload {label}: {:?}", if sent.is_ok() { "OK  " } else { "FAIL" }, sent.err().map(|e| format!("{e:#}"))));
    if let Some(messages) = run.step("read upload", "GET", format!("/channels/{t}/messages?limit=1"), None).await {
        let stored = messages.pointer("/0/attachments/0/size").and_then(Value::as_u64);
        run.note(format!("NOTE {label}: stored {stored:?} of {size} bytes"));
    }
}
