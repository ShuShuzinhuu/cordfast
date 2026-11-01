//! Rich presence, the way Vesktop does it with arRPC: a local server on the
//! pipe games already talk to (`\\.\pipe\discord-ipc-N`), so a game's
//! "Playing ..." activity reaches our profile through our gateway session.
//!
//! The IPC protocol: frames of `[opcode u32 LE][length u32 LE][JSON]`.
//! Opcode 0 is the handshake (`{v, client_id}`), answered with a READY
//! dispatch; 1 carries commands (`SET_ACTIVITY` is the one that matters);
//! 2 closes; 3 is a ping answered by 4. When the official Discord is
//! running it already holds `discord-ipc-0`, and we take the next free
//! number, as arRPC does.

use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::model::{Id, User};

pub enum RpcEvent {
    /// A game set (`Some`) or cleared (`None`) its activity.
    Activity { socket: u64, application: Id, activity: Option<Value> },
    Closed { socket: u64 },
}

#[cfg(windows)]
pub async fn serve(me: User, out: mpsc::UnboundedSender<RpcEvent>) {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut name = None;
    let mut first = None;
    for index in 0..10 {
        let candidate = format!(r"\\.\pipe\discord-ipc-{index}");
        if let Ok(server) = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&candidate)
        {
            name = Some(candidate);
            first = Some(server);
            break;
        }
    }
    let (Some(name), Some(mut server)) = (name, first) else {
        log::warn!("rpc: no free discord-ipc pipe");
        return;
    };
    log::info!("rpc: listening on {name}");
    let mut socket = 0u64;
    loop {
        if server.connect().await.is_err() {
            match ServerOptions::new().create(&name) {
                Ok(next) => server = next,
                Err(_) => return,
            }
            continue;
        }
        let connected = server;
        server = match ServerOptions::new().create(&name) {
            Ok(next) => next,
            Err(error) => {
                log::warn!("rpc: {error}");
                return;
            }
        };
        socket += 1;
        let (out, me) = (out.clone(), me.clone());
        tokio::spawn(async move {
            let id = socket;
            if let Err(error) = client(connected, id, &me, &out).await {
                log::debug!("rpc client {id}: {error:#}");
            }
            let _ = out.send(RpcEvent::Closed { socket: id });
        });
    }
}

/// On Linux and macOS the IPC is a Unix socket, `discord-ipc-N` in the
/// runtime folder; a socket a running Discord answers on is left alone, a
/// stale one is replaced.
#[cfg(unix)]
pub async fn serve(me: User, out: mpsc::UnboundedSender<RpcEvent>) {
    let dir = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .find_map(std::env::var_os)
        .map_or_else(|| std::path::PathBuf::from("/tmp"), std::path::PathBuf::from);
    let mut listener = None;
    for index in 0..10 {
        let path = dir.join(format!("discord-ipc-{index}"));
        if path.exists() {
            if tokio::net::UnixStream::connect(&path).await.is_ok() {
                continue;
            }
            let _ = std::fs::remove_file(&path);
        }
        if let Ok(bound) = tokio::net::UnixListener::bind(&path) {
            log::info!("rpc: listening on {}", path.display());
            listener = Some(bound);
            break;
        }
    }
    let Some(listener) = listener else {
        log::warn!("rpc: no free discord-ipc socket");
        return;
    };
    let mut socket = 0u64;
    while let Ok((stream, _)) = listener.accept().await {
        socket += 1;
        let (out, me) = (out.clone(), me.clone());
        tokio::spawn(async move {
            let id = socket;
            if let Err(error) = client(stream, id, &me, &out).await {
                log::debug!("rpc client {id}: {error:#}");
            }
            let _ = out.send(RpcEvent::Closed { socket: id });
        });
    }
}

#[cfg(not(any(windows, unix)))]
pub async fn serve(_me: User, _out: mpsc::UnboundedSender<RpcEvent>) {}

/// One game's connection, over a named pipe or a Unix socket.
async fn client<S>(mut pipe: S, socket: u64, me: &User, out: &mpsc::UnboundedSender<RpcEvent>) -> anyhow::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn write<S: tokio::io::AsyncWrite + Unpin>(
        pipe: &mut S,
        op: u32,
        body: &Value,
    ) -> anyhow::Result<()> {
        let data = body.to_string();
        let mut frame = Vec::with_capacity(8 + data.len());
        frame.extend(op.to_le_bytes());
        frame.extend((data.len() as u32).to_le_bytes());
        frame.extend(data.as_bytes());
        pipe.write_all(&frame).await?;
        Ok(())
    }

    let mut application: Id = 0;
    loop {
        let mut header = [0u8; 8];
        pipe.read_exact(&mut header).await?;
        let op = u32::from_le_bytes(header[..4].try_into()?);
        let len = u32::from_le_bytes(header[4..].try_into()?) as usize;
        anyhow::ensure!(len <= 1 << 20, "frame too large");
        let mut data = vec![0u8; len];
        pipe.read_exact(&mut data).await?;
        let message: Value = serde_json::from_slice(&data).unwrap_or(Value::Null);
        match op {
            0 => {
                application = message
                    .get("client_id")
                    .and_then(|c| c.as_str().and_then(|s| s.parse().ok()).or_else(|| c.as_u64()))
                    .unwrap_or(0);
                let ready = json!({
                    "cmd": "DISPATCH",
                    "evt": "READY",
                    "nonce": null,
                    "data": {
                        "v": 1,
                        "config": {
                            "cdn_host": "cdn.discordapp.com",
                            "api_endpoint": "//discord.com/api",
                            "environment": "production",
                        },
                        "user": {
                            "id": me.id.to_string(),
                            "username": me.username,
                            "discriminator": me.discriminator,
                            "global_name": me.global_name,
                            "avatar": me.avatar,
                            "bot": false,
                            "flags": 0,
                            "premium_type": 0,
                        },
                    },
                });
                write(&mut pipe, 1, &ready).await?;
            }
            1 => {
                let command = message.get("cmd").and_then(Value::as_str).unwrap_or_default();
                let nonce = message.get("nonce").cloned().unwrap_or(Value::Null);
                let args = message.get("args").cloned().unwrap_or(Value::Null);
                let data = match command {
                    "SET_ACTIVITY" => {
                        let activity = args.get("activity").filter(|a| !a.is_null()).cloned();
                        let _ = out.send(RpcEvent::Activity {
                            socket,
                            application,
                            activity: activity.clone(),
                        });
                        activity.unwrap_or(Value::Null)
                    }
                    "SUBSCRIBE" | "UNSUBSCRIBE" => json!({ "evt": args.get("evt") }),
                    _ => Value::Null,
                };
                let reply = json!({ "cmd": command, "data": data, "evt": null, "nonce": nonce });
                write(&mut pipe, 1, &reply).await?;
            }
            2 => return Ok(()),
            3 => write(&mut pipe, 4, &message).await?,
            _ => {}
        }
    }
}

/// Turns a game's activity into what the gateway's presence update takes:
/// the application's name as the title, art keys resolved to asset ids
/// or proxied URLs, and buttons split into labels and URLs.
pub async fn gateway_activity(
    api: &super::rest::Api,
    application: Id,
    activity: &Value,
) -> Value {
    // A detected game carries the detectable list's name ("PUBG:
    // BATTLEGROUNDS"), which Discord shows rather than the application's
    // older one ("PLAYERUNKNOWN'S BATTLEGROUNDS"); a game on the pipe sets
    // none and takes its application's.
    let name = match activity.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) {
        Some(name) => name.to_owned(),
        None => api.application_name(application).await.ok().unwrap_or_else(|| t!("Game").into()),
    };
    let mut out = json!({
        "application_id": application.to_string(),
        "name": name,
        "type": activity.get("type").and_then(Value::as_u64).unwrap_or(0),
        "flags": 1,
    });
    for key in ["details", "state", "timestamps", "party", "secrets", "instance"] {
        if let Some(value) = activity.get(key).filter(|v| !v.is_null()) {
            out[key] = value.clone();
        }
    }
    if let Some(assets) = activity.get("assets").and_then(Value::as_object) {
        let mut resolved = serde_json::Map::new();
        let named = api.application_assets(application).await.unwrap_or_default();
        for (key, value) in assets {
            let Some(text) = value.as_str() else { continue };
            let image = key.ends_with("_image");
            let converted = if !image || text.starts_with("mp:") {
                Some(text.to_owned())
            } else if text.starts_with("http://") || text.starts_with("https://") {
                api.external_assets(application, &[text.to_owned()])
                    .await
                    .ok()
                    .and_then(|v| v.into_iter().next())
            } else {
                named
                    .iter()
                    .find(|(n, _)| n == text)
                    .map(|(_, id)| id.clone())
                    .or_else(|| Some(text.to_owned()))
            };
            if let Some(converted) = converted {
                resolved.insert(key.clone(), Value::String(converted));
            }
        }
        out["assets"] = Value::Object(resolved);
    }
    if let Some(buttons) = activity.get("buttons").and_then(Value::as_array) {
        let labels: Vec<Value> = buttons.iter().filter_map(|b| b.get("label").cloned()).collect();
        let urls: Vec<Value> = buttons.iter().filter_map(|b| b.get("url").cloned()).collect();
        out["buttons"] = Value::Array(labels);
        out["metadata"] = json!({ "button_urls": urls });
    }
    out
}
