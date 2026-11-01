//! QR-code login, the way the desktop and web clients log in: the phone app
//! scans a code and confirms, and the token arrives encrypted to a key that
//! never leaves this process.
//!
//! The remote-auth gateway's exchange: `hello` → we send `init` with our
//! RSA public key → `nonce_proof` (we prove we hold the private key) →
//! `pending_remote_init` with the fingerprint the QR code carries →
//! `pending_ticket` once scanned (who scanned it) → `pending_login` with a
//! ticket, which the API trades for our encrypted token. `cancel` means the
//! phone declined.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use rsa::pkcs8::EncodePublicKey;
use rsa::{Oaep, RsaPrivateKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_tungstenite::tungstenite::{Message as WsMessage, client::IntoClientRequest};

use super::rest::{API, USER_AGENT};

const GATEWAY: &str = "wss://remote-auth-gateway.discord.gg/?v=2";

pub enum Progress {
    /// Show this URL as a QR code.
    Code(String),
    /// Scanned by this account; waiting for the phone to confirm.
    Scanned { username: String },
}

/// Runs one login until it yields a token, reporting progress. Fails when
/// the phone declines or the code expires.
pub async fn login(http: &reqwest::Client, mut progress: impl FnMut(Progress)) -> Result<String> {
    let key = tokio::task::spawn_blocking(|| {
        RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).map_err(|e| anyhow!(e))
    })
    .await??;
    let public = key
        .to_public_key()
        .to_public_key_der()
        .map_err(|e| anyhow!(e))?;
    let decrypt = |data: &str| -> Result<Vec<u8>> {
        let bytes = STANDARD.decode(data)?;
        key.decrypt(Oaep::new::<Sha256>(), &bytes)
            .map_err(|e| anyhow!(e))
    };

    let mut request = GATEWAY.into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", "https://discord.com".parse()?);
    request
        .headers_mut()
        .insert("User-Agent", USER_AGENT.parse()?);
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .context(t!("couldn't connect to Discord"))?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(40));
    heartbeat.tick().await;
    loop {
        let frame = tokio::select! {
            _ = heartbeat.tick() => {
                socket.send(WsMessage::text(json!({"op": "heartbeat"}).to_string())).await?;
                continue;
            }
            frame = socket.next() => frame,
        };
        let text = match frame {
            Some(Ok(WsMessage::Text(text))) => text,
            Some(Ok(WsMessage::Close(close))) => {
                bail!(
                    "{}{}",
                    t!("the code expired"),
                    close.map(|c| format!(" ({})", c.code)).unwrap_or_default()
                )
            }
            Some(Ok(_)) => continue,
            Some(Err(error)) => return Err(error.into()),
            None => bail!("{}", t!("Discord closed the connection")),
        };
        let message: Value = serde_json::from_str(text.as_str())?;
        let field = |name: &str| message.get(name).and_then(Value::as_str).unwrap_or_default();
        match field("op") {
            "hello" => {
                if let Some(ms) = message.get("heartbeat_interval").and_then(Value::as_u64) {
                    heartbeat = tokio::time::interval(Duration::from_millis(ms));
                    heartbeat.tick().await;
                }
                let init = json!({"op": "init", "encoded_public_key": STANDARD.encode(public.as_bytes())});
                socket.send(WsMessage::text(init.to_string())).await?;
            }
            "nonce_proof" => {
                log::info!("remote auth: proving key");
                let nonce = decrypt(field("encrypted_nonce"))?;
                let proof = URL_SAFE_NO_PAD.encode(Sha256::digest(&nonce));
                socket
                    .send(WsMessage::text(json!({"op": "nonce_proof", "proof": proof}).to_string()))
                    .await?;
            }
            "pending_remote_init" => {
                log::info!("remote auth: code ready");
                progress(Progress::Code(format!(
                    "https://discord.com/ra/{}",
                    field("fingerprint")
                )));
            }
            "pending_ticket" => {
                log::info!("remote auth: scanned");
                // "id:discriminator:avatar:username"
                let payload = String::from_utf8(decrypt(field("encrypted_user_payload"))?)?;
                let username = payload.splitn(4, ':').nth(3).unwrap_or_default().to_owned();
                progress(Progress::Scanned { username });
            }
            "pending_login" => {
                let ticket = field("ticket").to_owned();
                let _ = socket.close(None).await;
                let response: Value = http
                    .post(format!("{API}/users/@me/remote-auth/login"))
                    .header("Origin", "https://discord.com")
                    .json(&json!({ "ticket": ticket }))
                    .send()
                    .await?
                    .json()
                    .await?;
                let encrypted = response
                    .get("encrypted_token")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        anyhow!(
                            "{}",
                            t!("Discord refused the login: {reason}", reason = response.get("message").and_then(Value::as_str).unwrap_or(t!("no reason given")))
                        )
                    })?;
                return Ok(String::from_utf8(decrypt(encrypted)?)?);
            }
            "cancel" => bail!("{}", t!("login cancelled on the phone")),
            _ => {}
        }
    }
}
