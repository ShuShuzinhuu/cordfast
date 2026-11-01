//! E-mail (or phone) and password login with two-factor authentication,
//! the web client's flow: a fingerprint from `/experiments`, then
//! `/auth/login`, which answers with the token or with an MFA ticket that
//! `/auth/mfa/{totp,sms,backup}` trades for it.

use anyhow::{Result, anyhow};
use base64::Engine;
use serde_json::{Value, json};

use super::rest::{API, USER_AGENT, super_properties};

pub enum Outcome {
    Token(String),
    /// Two-factor authentication: which kinds this account accepts.
    Mfa(Mfa),
}

#[derive(Clone, Debug)]
pub struct Mfa {
    pub ticket: String,
    pub instance: Option<String>,
    pub totp: bool,
    pub sms: bool,
    pub backup: bool,
}

pub struct Login {
    http: reqwest::Client,
    fingerprint: Option<String>,
    properties: String,
}

impl Login {
    pub async fn new(http: reqwest::Client) -> Self {
        let properties = base64::engine::general_purpose::STANDARD.encode(super_properties().to_string());
        let fingerprint = async {
            let value: Value = http
                .get(format!("{API}/experiments"))
                .header("X-Super-Properties", &properties)
                .send()
                .await
                .ok()?
                .json()
                .await
                .ok()?;
            value.get("fingerprint")?.as_str().map(str::to_owned)
        }
        .await;
        Self { http, fingerprint, properties }
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let mut request = self
            .http
            .post(format!("{API}{path}"))
            .header("User-Agent", USER_AGENT)
            .header("X-Super-Properties", &self.properties)
            .header("X-Discord-Locale", crate::i18n::current().discord_tag())
            .header("Origin", "https://discord.com")
            .header("Referer", "https://discord.com/login")
            .json(&body);
        if let Some(fingerprint) = &self.fingerprint {
            request = request.header("X-Fingerprint", fingerprint);
        }
        let response = request.send().await?;
        let status = response.status();
        let value: Value = response.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(value);
        }
        Err(anyhow!(explain(&value)))
    }

    pub async fn password(&self, login: &str, password: &str) -> Result<Outcome> {
        let value = self
            .post(
                "/auth/login",
                json!({
                    "login": login.trim(),
                    "password": password,
                    "undelete": false,
                    "login_source": null,
                    "gift_code_sku_id": null,
                }),
            )
            .await?;
        outcome(&value)
    }

    /// Sends the code by SMS; returns the (masked) phone it went to.
    pub async fn send_sms(&self, mfa: &Mfa) -> Result<String> {
        let value = self.post("/auth/mfa/sms/send", json!({ "ticket": mfa.ticket })).await?;
        Ok(value.get("phone").and_then(Value::as_str).unwrap_or(t!("your phone")).to_owned())
    }

    /// `kind` is "totp", "sms" or "backup".
    pub async fn code(&self, mfa: &Mfa, kind: &str, code: &str) -> Result<String> {
        let code: String = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
        let value = self
            .post(
                &format!("/auth/mfa/{kind}"),
                json!({
                    "code": code,
                    "ticket": mfa.ticket,
                    "login_source": null,
                    "gift_code_sku_id": null,
                    "login_instance_id": mfa.instance,
                }),
            )
            .await?;
        value
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!(t!("Discord did not return the token.")))
    }
}

fn outcome(value: &Value) -> Result<Outcome> {
    if value.get("mfa").and_then(Value::as_bool) == Some(true) {
        let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
        return Ok(Outcome::Mfa(Mfa {
            ticket: value.get("ticket").and_then(Value::as_str).unwrap_or_default().to_owned(),
            instance: value.get("login_instance_id").and_then(Value::as_str).map(str::to_owned),
            totp: flag("totp"),
            sms: flag("sms"),
            backup: flag("backup"),
        }));
    }
    value
        .get("token")
        .and_then(Value::as_str)
        .map(|t| Outcome::Token(t.to_owned()))
        .ok_or_else(|| anyhow!(t!("Discord did not return the token.")))
}

/// A readable reason from Discord's error body.
fn explain(value: &Value) -> String {
    if value.get("captcha_key").is_some() {
        return t!("Discord asked for a captcha for this login. Log in with the QR code.").into();
    }
    // Field errors: { errors: { login: { _errors: [{ message }] } } }.
    if let Some(errors) = value.get("errors").and_then(Value::as_object) {
        for field in errors.values() {
            if let Some(message) = field.pointer("/_errors/0/message").and_then(Value::as_str) {
                return message.to_owned();
            }
        }
    }
    value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(t!("Couldn't log in."))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mfa_and_errors() {
        let mfa = json!({ "mfa": true, "ticket": "t", "totp": true, "sms": false, "backup": true, "login_instance_id": "i" });
        match outcome(&mfa).unwrap() {
            Outcome::Mfa(m) => assert!(m.totp && m.backup && !m.sms && m.ticket == "t"),
            Outcome::Token(_) => panic!(),
        }
        let error = json!({ "message": "Invalid Form Body", "errors": { "login": { "_errors": [{ "message": "Login or password is invalid." }] } } });
        assert_eq!(explain(&error), "Login or password is invalid.");
        assert!(explain(&json!({ "captcha_key": ["captcha-required"] })).contains("captcha"));
    }
}
