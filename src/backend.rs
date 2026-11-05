//! Channel bridge between the interface and the asynchronous runtime, as in
//! ZapFast: a tokio runtime on its own thread owns the gateway, the API,
//! voice, rich presence, notifications and image downloads. The two sides
//! talk only through [`Command`] and [`Event`], and every event wakes the
//! window (or the headless shell, when the window is closed to the tray).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use serde_json::Value;
use tokio::sync::{Semaphore, mpsc};

use crate::discord::gateway::{self, Control, Dispatch};
use crate::discord::model::{Channel, Id, Message, User};
use crate::discord::remote_auth::{self, Progress};
use crate::discord::rest::{self, Api, Profile};
use crate::discord::rpc::{self, RpcEvent};
use crate::voice::{self, AudioSettings, VoiceCommand, VoiceEvent};

const KEYRING_SERVICE: &str = "cordfast";
const KEYRING_USER: &str = "token";

pub enum Command {
    StartQrLogin,
    PasswordLogin { login: String, password: String },
    /// A two-factor code: kind is "totp", "sms" or "backup".
    MfaCode { kind: &'static str, code: String },
    SendSms,
    LoginWithToken(String),
    Logout,
    /// Signs in to another saved account.
    SwitchAccount(Id),
    /// Leaves this account signed in and shows the login screen.
    AddAccount,
    /// Forgets a saved account's token.
    RemoveAccount(Id),
    LoadMessages { channel: Id, before: Option<Id> },
    Send {
        channel: Id,
        content: String,
        nonce: String,
        reply_to: Option<Id>,
        ping_reply: bool,
        files: Vec<Upload>,
    },
    Edit { channel: Id, message: Id, content: String },
    /// The viewer's "Save Image": into the downloads folder.
    SaveImage { url: String, name: String },
    /// The viewer's "Copy Image": onto the clipboard as a picture.
    CopyImage { url: String },
    Delete { channel: Id, message: Id },
    React { channel: Id, message: Id, emoji: String, add: bool },
    Ack { channel: Id, message: Id },
    AckBulk(Vec<(Id, Id)>),
    Typing { channel: Id },
    Subscribe { guild: Id },
    MemberList { guild: Id, channel: Id, rows: u64 },
    RequestMembers { guild: Id, users: Vec<Id> },
    /// Downloads and decodes a picture no larger than `max` pixels a side.
    FetchImage { url: String, max: u32 },
    /// The window dropped this picture; fetch it again when asked.
    Forget { url: String },
    Profile { user: Id, guild: Option<Id> },
    OpenDm { user: Id },
    ForumPosts { forum: Id },
    Notify { title: String, body: String, channel: Id, sound: bool },
    /// Turns the rich-presence server on or off.
    RichPresence(bool),
    Voice(VoiceCommand),
    /// Joins a voice channel or a DM call; rings the others for a new call.
    Call { guild: Option<Id>, channel: Id, ring: bool },
    CallRegion { guild: Option<Id>, channel: Id, region: String },
    DeclineCall { channel: Id },
    Mute { guild: Option<Id>, channel: Option<Id>, seconds: Option<i64> },
    NotifyLevel { guild: Option<Id>, channel: Option<Id>, level: u8 },
    CloseDm(Id),
    LeaveGuild(Id),
    /// A status, until a moment in Unix milliseconds or for good.
    SetStatus { status: String, settings: Vec<u8> },
    /// Part of the account's PreloadedUserSettings, merged into it.
    SaveSettings(Vec<u8>),
    /// A button press or menu choice, sent to the bot that owns it.
    Interact { origin: crate::discord::components::Origin, data: Value },
    /// A slash command (kind 2) or an autocomplete request (kind 4).
    SlashCommand { guild: Option<Id>, channel: Id, application: Id, kind: u64, data: Value, nonce: String },
    SetCustomStatus { text: Option<String>, settings: Vec<u8> },
    AddFriend(String),
    /// `Some(1)` accepts a request, `Some(2)` blocks, `None` removes.
    Relationship { user: Id, kind: Option<u64> },
    /// A call the server settings pages make; answered by `Event::Api`.
    Api { key: String, method: &'static str, path: String, body: Option<Value> },
    /// A call Discord held for a second factor, with the answer to it.
    MfaFinish { ticket: String, mfa_type: &'static str, code: String, key: String, method: &'static str, path: String, body: Option<Value> },
}

/// A file waiting in the composer, as Discord's upload card holds it: the
/// name it goes up with, its description (alt text) and whether it is a
/// spoiler.
#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    pub path: PathBuf,
    pub name: String,
    pub description: String,
    pub spoiler: bool,
}

impl Upload {
    pub fn new(path: PathBuf) -> Self {
        let name = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
        Self { path, name, description: String::new(), spoiler: false }
    }

    /// The name Discord receives: a spoiler's starts with "SPOILER_", as
    /// Discord marks them.
    pub fn sent_name(&self) -> String {
        if self.spoiler && !self.name.starts_with("SPOILER_") { format!("SPOILER_{}", self.name) } else { self.name.clone() }
    }

    /// Whether the card can show the picture itself.
    pub fn is_image(&self) -> bool {
        let extension = self.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
    }
}

pub enum Auth {
    /// No saved token: show the login screen.
    LoggedOut,
    /// A session is starting (a saved token, a login just done, another
    /// account): Discord's loading screen until it is ready.
    Starting,
    Generating,
    Code(String),
    Scanned { username: String },
    Failed(String),
    /// The password worked; a second factor is needed.
    Mfa { totp: bool, sms: bool, backup: bool },
    SmsSent(String),
    /// A password or code login failed; the QR code stays as it was.
    LoginError(String),
}

pub enum Event {
    Auth(Auth),
    Gateway(Dispatch),
    Messages { channel: Id, before: Option<Id>, messages: Vec<Message> },
    MessagesFailed { channel: Id, error: String },
    SendFailed { channel: Id, nonce: String, error: String },
    Error(String),
    Image { url: String, image: Option<Picture> },
    Profile { user: Id, profile: Result<Box<Profile>, String> },
    DmOpened(Channel),
    ForumPosts { forum: Id, posts: Vec<(Channel, Option<Message>)> },
    Voice(VoiceEvent),
    NotificationClicked { channel: Id },
    /// What you are doing: the shown game's name, for the account panel,
    /// and every activity in full, for your own profile.
    Activity(Option<String>, Vec<crate::discord::gateway::Activity>),
    Api { key: String, result: Result<Value, String> },
    /// A `Command::Api` waiting for a second factor.
    Mfa { ticket: String, methods: Vec<String>, key: String, method: &'static str, path: String, body: Option<Value> },
}

/// Wakes whatever is showing: a window, or the headless shell loop.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct Events {
    tx: std::sync::mpsc::Sender<Event>,
    wake: Wake,
    /// Lazy events waiting for the next beat of the window.
    pending: Arc<std::sync::atomic::AtomicBool>,
}

impl Events {
    pub fn send(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            (self.wake)();
        }
    }

    /// An event the eye can wait for. Accounts in many servers get presences
    /// and member updates all the time, and drawing the window for each kept
    /// a core busy: these wait for a beat, four times a second.
    pub fn send_lazy(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            self.pending.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Starts the beat: it only wakes the window when something waits.
    fn beat(&self) {
        let (pending, wake) = (self.pending.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("cordfast-beat".into()).spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if pending.swap(false, std::sync::atomic::Ordering::Relaxed) {
                wake();
            }
        });
    }
}

/// The interface's end of the command queue; cheap to clone.
#[derive(Clone)]
pub struct Commands(mpsc::UnboundedSender<Command>);

impl Commands {
    pub fn send(&self, command: Command) {
        let _ = self.0.send(command);
    }
}

pub struct Backend {
    pub commands: Commands,
    pub events: std::sync::mpsc::Receiver<Event>,
}

impl Backend {
    pub fn spawn(wake: Wake, logo: Vec<u8>) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let events = Events { tx: event_tx, wake, pending: Default::default() };
        events.beat();
        std::thread::Builder::new()
            .name("cordfast-backend".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("cordfast-runtime")
                    .enable_all()
                    .build()
                    .expect("unable to start the async runtime");
                runtime.block_on(run(events, command_rx, logo));
                runtime.shutdown_timeout(Duration::from_secs(2));
            })
            .expect("unable to start the backend thread");
        Self {
            commands: Commands(command_tx),
            events: event_rx,
        }
    }
}

fn keyring_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()
}

fn saved_token() -> Option<String> {
    keyring_entry()?.get_password().ok().filter(|t| !t.is_empty())
}

fn save_token(token: &str) {
    if let Some(entry) = keyring_entry()
        && let Err(error) = entry.set_password(token)
    {
        log::warn!("keyring: {error}");
    }
}

/// Each account's token, kept to switch back to it.
fn account_entry(id: Id) -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("account:{id}")).ok()
}

fn forget_token() {
    if let Some(entry) = keyring_entry() {
        let _ = entry.delete_credential();
    }
}

/// What the gateway's sink hands the worker besides forwarding it.
enum Hook {
    Ready { me: User, status: String, custom_status: Option<String>, session: String },
    VoiceState(Value),
    VoiceServer(Value),
    Token(String),
    Mfa(Arc<crate::discord::login::Login>, crate::discord::login::Mfa),
}

struct Worker {
    events: Events,
    http: reqwest::Client,
    api: Option<Api>,
    gateway: Option<mpsc::UnboundedSender<Control>>,
    login: Option<tokio::task::JoinHandle<()>>,
    /// The session's token, saved under its account once READY says whose.
    token: Option<String>,
    images: Arc<Semaphore>,
    requested: HashSet<String>,
    hooks: mpsc::UnboundedSender<Hook>,
    notifications: Option<fastframe_notifications::Client>,
    me: Option<User>,
    status: String,
    custom_status: Option<String>,
    session_id: String,
    voice: Option<mpsc::UnboundedSender<VoiceCommand>>,
    rpc_enabled: bool,
    rpc: Option<tokio::task::JoinHandle<()>>,
    rpc_tx: mpsc::UnboundedSender<RpcEvent>,
    /// A password login waiting for its second factor.
    pending_login: Option<(Arc<crate::discord::login::Login>, crate::discord::login::Mfa)>,
    /// Each game's activity, by IPC connection.
    activities: BTreeMap<u64, (String, Value)>,
}

async fn run(events: Events, mut commands: mpsc::UnboundedReceiver<Command>, logo: Vec<u8>) {
    let (hooks, mut hooks_rx) = mpsc::unbounded_channel();
    let (rpc_tx, mut rpc_rx) = mpsc::unbounded_channel();
    let notifications = fastframe_notifications::Client::new(
        fastframe_notifications::App::new("cordfast", "CordFast").logo(&logo, [0x58, 0x65, 0xf2]),
    )
    .map_err(|e| log::warn!("notifications: {e:?}"))
    .ok();
    let mut worker = Worker {
        events,
        http: rest::http(),
        api: None,
        gateway: None,
        login: None,
        token: None,
        images: Arc::new(Semaphore::new(12)),
        requested: HashSet::new(),
        hooks,
        notifications,
        me: None,
        status: "online".into(),
        custom_status: None,
        session_id: String::new(),
        voice: None,
        rpc_enabled: false,
        rpc: None,
        rpc_tx,
        pending_login: None,
        activities: BTreeMap::new(),
    };
    let demo = crate::demo::Demo::load();
    match (&demo, saved_token()) {
        (Some(demo), _) => demo.start(&worker.events),
        (None, Some(token)) => worker.start_session(token),
        (None, None) => worker.events.send(Event::Auth(Auth::LoggedOut)),
    }
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(command) if demo.as_ref().is_some_and(|d| d.answer(&command, &worker.events)) => {}
                Some(command) => worker.handle(command),
                None => break,
            },
            Some(hook) = hooks_rx.recv() => worker.hook(hook),
            Some(event) = rpc_rx.recv() => worker.rpc_event(event).await,
        }
    }
    worker.stop_session();
}

impl Worker {
    fn start_session(&mut self, token: String) {
        self.events.send(Event::Auth(Auth::Starting));
        self.stop_session();
        self.token = Some(token.clone());
        self.api = Some(Api::new(self.http.clone(), token.clone()));
        let (control, control_rx) = mpsc::unbounded_channel();
        self.gateway = Some(control);
        let events = self.events.clone();
        let hooks = self.hooks.clone();
        let sink: gateway::Sink = Arc::new(move |dispatch| {
            match &dispatch {
                Dispatch::AuthFailed => forget_token(),
                Dispatch::Ready(ready) => {
                    let _ = hooks.send(Hook::Ready {
                        me: ready.me.clone(),
                        status: ready.status.clone(),
                        custom_status: ready.custom_status.clone(),
                        session: ready.session_id.clone(),
                    });
                }
                Dispatch::VoiceState(state) => {
                    let _ = hooks.send(Hook::VoiceState(state.clone()));
                }
                Dispatch::VoiceServer(server) => {
                    let _ = hooks.send(Hook::VoiceServer(server.clone()));
                }
                _ => {}
            }
            if matches!(dispatch, Dispatch::Presences(_) | Dispatch::Members { .. } | Dispatch::MemberList(_)) {
                events.send_lazy(Event::Gateway(dispatch));
            } else {
                events.send(Event::Gateway(dispatch));
            }
        });
        tokio::spawn(gateway::run(token, sink, control_rx));
    }

    fn stop_session(&mut self) {
        if let Some(gateway) = self.gateway.take() {
            let _ = gateway.send(Control::Shutdown);
        }
        if let Some(voice) = self.voice.take() {
            let _ = voice.send(VoiceCommand::Leave);
        }
        if let Some(rpc) = self.rpc.take() {
            rpc.abort();
        }
        self.activities.clear();
        self.api = None;
        self.me = None;
    }

    fn control(&self, control: Control) {
        if let Some(gateway) = &self.gateway {
            let _ = gateway.send(control);
        }
    }

    fn hook(&mut self, hook: Hook) {
        match hook {
            Hook::Ready { me, status, custom_status, session } => {
                self.session_id = session;
                let first = self.me.is_none();
                self.me = Some(me.clone());
                if let (Some(token), Some(entry)) = (&self.token, account_entry(me.id)) {
                    let _ = entry.set_password(token);
                }
                self.status = status;
                self.custom_status = custom_status;
                if first {
                    self.start_voice(me.id);
                    if self.rpc_enabled {
                        self.start_rpc();
                    }
                }
                // A new session forgets our activity: send it again.
                if !self.activities.is_empty() {
                    self.send_presence();
                }
            }
            Hook::VoiceState(state) => {
                if let Some(voice) = &self.voice {
                    // Our account from another session (another client): it
                    // never speaks for this one. The voice task decides
                    // whether it took our call.
                    let me = self.me.as_ref().map(|u| u.id);
                    let mine = me.is_some() && state.get("user_id").and_then(crate::discord::model::sf) == me;
                    let theirs = state.get("session_id").and_then(Value::as_str).is_some_and(|s| !self.session_id.is_empty() && s != self.session_id);
                    if mine && theirs {
                        let _ = voice.send(VoiceCommand::Elsewhere(state));
                    } else {
                        let _ = voice.send(VoiceCommand::State(state));
                    }
                }
            }
            Hook::VoiceServer(server) => {
                if let Some(voice) = &self.voice {
                    let _ = voice.send(VoiceCommand::Server(server));
                }
            }
            Hook::Mfa(session, mfa) => self.pending_login = Some((session, mfa)),
            Hook::Token(token) => {
                self.pending_login = None;
                save_token(&token);
                self.start_session(token);
            }
        }
    }

    fn start_voice(&mut self, me: Id) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.voice = Some(tx);
        let gateway = self.gateway.clone();
        let events = self.events.clone();
        tokio::spawn(voice::run(
            me,
            self.http.clone(),
            rx,
            move |state| {
                if let Some(gateway) = &gateway {
                    let _ = gateway.send(Control::VoiceState {
                        guild: state.guild,
                        channel: state.channel,
                        mute: state.mute,
                        deaf: state.deaf,
                        regions: state.regions,
                    });
                }
            },
            move |event| events.send(Event::Voice(event)),
        ));
    }

    fn start_rpc(&mut self) {
        if self.rpc.is_some() {
            return;
        }
        let (Some(me), Some(api)) = (self.me.clone(), self.api.clone()) else { return };
        let tx = self.rpc_tx.clone();
        // Games that set their own presence over the pipe, and the many that
        // don't, recognised by their process.
        self.rpc = Some(tokio::spawn(async move {
            tokio::join!(rpc::serve(me, tx.clone()), crate::discord::detect::run(api, tx));
        }));
    }

    async fn rpc_event(&mut self, event: RpcEvent) {
        match event {
            RpcEvent::Activity { socket, application, activity } => match activity {
                Some(activity) => {
                    let Some(api) = self.api.clone() else { return };
                    let converted = rpc::gateway_activity(&api, application, &activity).await;
                    let name = converted
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(t!("Game"))
                        .to_owned();
                    self.activities.insert(socket, (name, converted));
                }
                None => {
                    self.activities.remove(&socket);
                }
            },
            RpcEvent::Closed { socket } => {
                if self.activities.remove(&socket).is_none() {
                    return;
                }
            }
        }
        self.send_presence();
    }

    fn send_presence(&self) {
        // A detected game that also speaks over the pipe shows once, with
        // the richer activity it sets itself.
        let app = |a: &Value| a.get("application_id").and_then(Value::as_str).map(str::to_owned);
        let piped: Vec<String> = self
            .activities
            .iter()
            .filter(|(s, _)| !crate::discord::detect::detected(**s))
            .filter_map(|(_, (_, a))| app(a))
            .collect();
        let shown: Vec<&(String, Value)> = self
            .activities
            .iter()
            .filter(|(s, (_, a))| !crate::discord::detect::detected(**s) || app(a).is_none_or(|id| !piped.contains(&id)))
            .map(|(_, v)| v)
            .collect();
        let mut activities: Vec<Value> = shown.iter().map(|(_, a)| a.clone()).collect();
        // The custom status travels as an activity of type 4.
        if let Some(text) = &self.custom_status {
            activities.insert(0, serde_json::json!({ "name": "Custom Status", "type": 4, "state": text }));
        }
        let full = shown.iter().filter_map(|(_, a)| crate::discord::gateway::Activity::parse(a)).collect();
        let shown = shown.first().map(|(name, _)| name.clone());
        self.control(Control::Presence {
            status: self.status.clone(),
            activities,
        });
        self.events.send(Event::Activity(shown, full));
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::PasswordLogin { login, password } => {
                let (events, http, hooks) = (self.events.clone(), self.http.clone(), self.hooks.clone());
                let internal = self.hooks.clone();
                tokio::spawn(async move {
                    use crate::discord::login::{Login, Outcome};
                    let session = Arc::new(Login::new(http).await);
                    match session.password(&login, &password).await {
                        Ok(Outcome::Token(token)) => {
                            let _ = hooks.send(Hook::Token(token));
                        }
                        Ok(Outcome::Mfa(mfa)) => {
                            events.send(Event::Auth(Auth::Mfa { totp: mfa.totp, sms: mfa.sms, backup: mfa.backup }));
                            let _ = internal.send(Hook::Mfa(session, mfa));
                        }
                        Err(error) => events.send(Event::Auth(Auth::LoginError(format!("{error:#}")))),
                    }
                });
            }
            Command::MfaCode { kind, code } => {
                let Some((session, mfa)) = self.pending_login.clone() else { return };
                let (events, hooks) = (self.events.clone(), self.hooks.clone());
                tokio::spawn(async move {
                    match session.code(&mfa, kind, &code).await {
                        Ok(token) => {
                            let _ = hooks.send(Hook::Token(token));
                        }
                        Err(error) => events.send(Event::Auth(Auth::LoginError(format!("{error:#}")))),
                    }
                });
            }
            Command::SendSms => {
                let Some((session, mfa)) = self.pending_login.clone() else { return };
                let events = self.events.clone();
                tokio::spawn(async move {
                    match session.send_sms(&mfa).await {
                        Ok(phone) => events.send(Event::Auth(Auth::SmsSent(phone))),
                        Err(error) => events.send(Event::Auth(Auth::LoginError(format!("{error:#}")))),
                    }
                });
            }
            Command::StartQrLogin => {
                if let Some(login) = self.login.take() {
                    login.abort();
                }
                let events = self.events.clone();
                let http = self.http.clone();
                let hooks = self.hooks.clone();
                events.send(Event::Auth(Auth::Generating));
                self.login = Some(tokio::spawn(async move {
                    let progress = |p| {
                        events.send(Event::Auth(match p {
                            Progress::Code(url) => Auth::Code(url),
                            Progress::Scanned { username } => Auth::Scanned { username },
                        }))
                    };
                    match remote_auth::login(&http, progress).await {
                        Ok(token) => {
                            let _ = hooks.send(Hook::Token(token));
                        }
                        Err(error) => events.send(Event::Auth(Auth::Failed(format!("{error:#}")))),
                    }
                }));
            }
            Command::LoginWithToken(token) => {
                let token = token.trim().trim_matches('"').to_owned();
                save_token(&token);
                self.start_session(token);
            }
            Command::Logout => {
                if let Some(entry) = self.me.as_ref().and_then(|me| account_entry(me.id)) {
                    let _ = entry.delete_credential();
                }
                self.stop_session();
                forget_token();
                self.events.send(Event::Auth(Auth::LoggedOut));
            }
            Command::SwitchAccount(id) => {
                let token = account_entry(id).and_then(|e| e.get_password().ok());
                match token {
                    Some(token) => {
                        save_token(&token);
                        self.start_session(token);
                    }
                    None => self.events.send(Event::Error(t!("This account needs to log in again.").into())),
                }
            }
            Command::AddAccount => {
                self.stop_session();
                forget_token();
                self.events.send(Event::Auth(Auth::LoggedOut));
            }
            Command::RemoveAccount(id) => {
                if let Some(entry) = account_entry(id) {
                    let _ = entry.delete_credential();
                }
            }
            Command::Subscribe { guild } => self.control(Control::Subscribe(guild)),
            Command::MemberList { guild, channel, rows } => {
                self.control(Control::MemberList { guild, channel, rows });
            }
            Command::RequestMembers { guild, users } => {
                self.control(Control::RequestMembers { guild, users });
            }
            Command::Forget { url } => {
                self.requested.remove(&url);
            }
            Command::SaveImage { url, name } => {
                let (http, events) = (self.http.clone(), self.events.clone());
                tokio::spawn(async move {
                    let result = save_image(&http, &url, &name).await.map(|path| serde_json::json!({ "path": path.display().to_string() }));
                    events.send(Event::Api { key: "saved-image".into(), result: result.map_err(|e| format!("{e:#}")) });
                });
            }
            Command::CopyImage { url } => {
                let (http, events) = (self.http.clone(), self.events.clone());
                tokio::spawn(async move {
                    let result = copy_image(&http, &url).await.map(|()| serde_json::json!({}));
                    events.send(Event::Api { key: "copied-image".into(), result: result.map_err(|e| format!("{e:#}")) });
                });
            }
            Command::FetchImage { url, max } => {
                if !self.requested.insert(url.clone()) {
                    return;
                }
                let http = self.http.clone();
                let events = self.events.clone();
                let permits = self.images.clone();
                tokio::spawn(async move {
                    let _permit = permits.acquire_owned().await;
                    let image = fetch_image(&http, &url, max).await;
                    if let Err(error) = &image {
                        log::info!("image {}: {error:#}", url.split('?').next().unwrap_or_default());
                    }
                    events.send(Event::Image { url, image: image.ok() });
                });
            }
            Command::Notify { title, body, channel, sound } => {
                let Some(client) = &self.notifications else { return };
                let mut notification = fastframe_notifications::Notification::new(title)
                    .body(body)
                    .key(channel.to_string());
                if !sound {
                    notification = notification.sound(fastframe_notifications::Sound::Silent);
                }
                let events = self.events.clone();
                let shown = client.show_with(notification, move |event| {
                    if event == fastframe_notifications::Event::Activated {
                        events.send(Event::NotificationClicked { channel });
                    }
                });
                if let Err(error) = shown {
                    log::warn!("notification: {error:?}");
                }
            }
            Command::RichPresence(on) => {
                self.rpc_enabled = on;
                if on {
                    self.start_rpc();
                } else {
                    if let Some(rpc) = self.rpc.take() {
                        rpc.abort();
                    }
                    if !self.activities.is_empty() {
                        self.activities.clear();
                        self.send_presence();
                    }
                }
            }
            Command::Voice(command) => {
                if let Some(voice) = &self.voice {
                    let _ = voice.send(command);
                }
            }
            Command::Interact { origin, data } => {
                let Some(api) = self.api.clone() else { return };
                let (events, session) = (self.events.clone(), self.session_id.clone());
                tokio::spawn(async move {
                    if let Err(error) = api.interact(&origin, &session, data).await {
                        events.send(Event::Error(t!("The interaction failed: {error}", error = format!("{error:#}"))));
                    }
                });
            }
            Command::SlashCommand { guild, channel, application, kind, data, nonce } => {
                let Some(api) = self.api.clone() else { return };
                let (events, session) = (self.events.clone(), self.session_id.clone());
                tokio::spawn(async move {
                    if let Err(error) = api.command(guild, channel, application, &session, kind, data, &nonce).await
                        && kind == 2
                    {
                        events.send(Event::Error(t!("The interaction failed: {error}", error = format!("{error:#}"))));
                    }
                });
            }
            Command::SetCustomStatus { text, settings } => {
                self.custom_status = text.clone();
                self.send_presence();
                if let Some(api) = self.api.clone() {
                    let events = self.events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = api.save_settings(&settings).await {
                            events.send(Event::Error(t!("Couldn't save the status: {error}", error = format!("{error:#}"))));
                        }
                    });
                }
            }
            Command::SaveSettings(patch) => {
                if let Some(api) = self.api.clone() {
                    let events = self.events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = api.save_settings(&patch).await {
                            events.send(Event::Error(t!("Couldn't save the setting: {error}", error = format!("{error:#}"))));
                        }
                    });
                }
            }
            Command::SetStatus { status, settings } => {
                self.status = status.clone();
                self.send_presence();
                if let Some(api) = self.api.clone() {
                    let events = self.events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = api.save_settings(&settings).await {
                            events.send(Event::Error(t!("Couldn't save the status: {error}", error = format!("{error:#}"))));
                        }
                    });
                }
            }
            Command::DeclineCall { channel } => {
                if let (Some(api), Some(me)) = (self.api.clone(), self.me.as_ref().map(|m| m.id)) {
                    tokio::spawn(async move {
                        if let Err(error) = api.stop_ringing(channel, me).await {
                            log::debug!("stop ringing: {error:#}");
                        }
                    });
                }
            }
            Command::Call { guild, channel, ring } => {
                if let Some(voice) = &self.voice {
                    let _ = voice.send(VoiceCommand::Join { guild, channel });
                }
                if ring && let Some(api) = self.api.clone() {
                    tokio::spawn(async move {
                        // The call exists once our voice state lands.
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        if let Err(error) = api.ring(channel).await {
                            log::debug!("ring: {error:#}");
                        }
                    });
                }
            }
            command => {
                let Some(api) = self.api.clone() else { return };
                let events = self.events.clone();
                tokio::spawn(api_call(api, events, command));
            }
        }
    }
}

async fn api_call(api: Api, events: Events, command: Command) {
    let fail = |what: &str, error: anyhow::Error| {
        events.send(Event::Error(format!("{what}: {error:#}")));
    };
    match command {
        Command::LoadMessages { channel, before } => match api.messages(channel, before).await {
            Ok(messages) => events.send(Event::Messages { channel, before, messages }),
            Err(error) => events.send(Event::MessagesFailed {
                channel,
                error: format!("{error:#}"),
            }),
        },
        Command::Send { channel, content, nonce, reply_to, ping_reply, files } => {
            if let Err(error) = api
                .send_message(channel, &content, &nonce, reply_to, ping_reply, files)
                .await
            {
                events.send(Event::SendFailed {
                    channel,
                    nonce,
                    error: format!("{error:#}"),
                });
            }
        }
        Command::Edit { channel, message, content } => {
            if let Err(error) = api.edit_message(channel, message, &content).await {
                fail(t!("Couldn't edit the message"), error);
            }
        }
        Command::Delete { channel, message } => {
            if let Err(error) = api.delete_message(channel, message).await {
                fail(t!("Couldn't delete the message"), error);
            }
        }
        Command::React { channel, message, emoji, add } => {
            if let Err(error) = api.react(channel, message, &emoji, add).await {
                fail(t!("Couldn't react"), error);
            }
        }
        Command::Ack { channel, message } => {
            if let Err(error) = api.ack(channel, message).await {
                log::debug!("ack: {error:#}");
            }
        }
        Command::AckBulk(reads) => {
            if let Err(error) = api.ack_bulk(&reads).await {
                fail(t!("Couldn't mark as read"), error);
            }
        }
        Command::Typing { channel } => {
            let _ = api.typing(channel).await;
        }
        Command::Profile { user, guild } => {
            let profile = api
                .profile(user, guild)
                .await
                .map(Box::new)
                .map_err(|e| format!("{e:#}"));
            events.send(Event::Profile { user, profile });
        }
        Command::OpenDm { user } => match api.open_dm(user).await {
            Ok(channel) => events.send(Event::DmOpened(channel)),
            Err(error) => fail(t!("Couldn't open the conversation"), error),
        },
        Command::ForumPosts { forum } => match api.forum_posts(forum).await {
            Ok(posts) => events.send(Event::ForumPosts { forum, posts }),
            Err(error) => fail(t!("Couldn't load the forum"), error),
        },
        Command::Api { key, method, path, body } => match api.raw(method, &path, body.clone(), None).await {
            Err(error) if error.is::<crate::discord::rest::MfaRequired>() => {
                if let Ok(mfa) = error.downcast::<crate::discord::rest::MfaRequired>() {
                    events.send(Event::Mfa { ticket: mfa.ticket, methods: mfa.methods, key, method, path, body });
                }
            }
            result => events.send(Event::Api { key, result: result.map_err(|e| format!("{e:#}")) }),
        },
        Command::MfaFinish { ticket, mfa_type, code, key, method, path, body } => {
            let result = match api.mfa_finish(&ticket, mfa_type, &code).await {
                Ok(token) => api.raw_with_mfa(method, &path, body, &token).await,
                Err(error) => Err(error),
            };
            events.send(Event::Api { key, result: result.map_err(|e| format!("{e:#}")) });
        }
        Command::AddFriend(username) => match api.add_friend(&username).await {
            Ok(()) => events.send(Event::Error(t!("Friend request sent to {name}.", name = username))),
            Err(error) => fail(t!("Couldn't send the request"), error),
        },
        Command::Relationship { user, kind } => {
            if let Err(error) = api.relationship(user, kind).await {
                fail(t!("Couldn't finish"), error);
            }
        }
        Command::Mute { guild, channel, seconds } => {
            if let Err(error) = api.set_muted(guild, channel, seconds).await {
                fail(t!("Couldn't mute"), error);
            }
        }
        Command::NotifyLevel { guild, channel, level } => {
            if let Err(error) = api.set_notify_level(guild, channel, level).await {
                fail(t!("Couldn't change the notifications"), error);
            }
        }
        Command::CloseDm(channel) => {
            if let Err(error) = api.close_dm(channel).await {
                fail(t!("Couldn't close the conversation"), error);
            }
        }
        Command::LeaveGuild(guild) => {
            if let Err(error) = api.leave_guild(guild).await {
                fail(t!("Couldn't leave the server"), error);
            }
        }
        Command::CallRegion { guild, channel, region } => {
            let result = match guild {
                None => api.set_call_region(channel, &region).await,
                Some(_) => {
                    let region = (region != "auto").then_some(region.as_str());
                    api.set_channel_region(channel, region).await
                }
            };
            if let Err(error) = result {
                fail(t!("Couldn't change the region"), error);
            }
        }
        _ => {}
    }
}

/// Downloads a picture and shrinks it on this thread, so the interface only
/// uploads a texture no larger than it draws.
async fn fetch_image(
    http: &reqwest::Client,
    url: &str,
    max: u32,
) -> anyhow::Result<Picture> {
    // A file waiting to be sent: its picture for the upload card.
    // A picture only ever drawn still (a grid of stickers) skips its other frames.
    let (url, still) = url.strip_suffix(STILL).map_or((url, false), |url| (url, true));
    let bytes = match url.strip_prefix("file://") {
        Some(path) => tokio::fs::read(path).await?.into(),
        None => http.get(url).send().await?.error_for_status()?.bytes().await?,
    };
    let lottie = url.ends_with(".json");
    tokio::task::spawn_blocking(move || {
        if lottie {
            return lottie_frames(&bytes, max, still);
        }
        decode_picture(&bytes, max, still)
    })
    .await?
}

/// Ending a picture's URL: only its first frame is wanted.
pub const STILL: &str = "#still";

/// A decoded picture: one frame, or an animation's with how long each shows
/// in milliseconds.
pub struct Picture {
    pub frames: Vec<egui::ColorImage>,
    pub delays: Vec<u32>,
}

/// Most an animation may hold in memory: past it, frames are dropped evenly
/// and the ones left show longer, so a long GIF plays coarser, not shorter.
const ANIMATION_BYTES: usize = 12 << 20;
/// Most frames, for tiny animations the bytes would not stop.
const ANIMATION_FRAMES: usize = 300;

/// Frames as they decode, never holding more than the limits.
struct Reel {
    max: u32,
    frames: Vec<egui::ColorImage>,
    delays: Vec<u32>,
    stride: usize,
    seen: usize,
}

impl Reel {
    fn new(max: u32) -> Self {
        Self { max, frames: Vec::new(), delays: Vec::new(), stride: 1, seen: 0 }
    }

    fn push(&mut self, mut image: image::RgbaImage, delay: u32) {
        // As browsers read them: a delay too short to mean anything is 100 ms.
        let delay = if delay < 20 { 100 } else { delay };
        self.seen += 1;
        if (self.seen - 1) % self.stride != 0 {
            if let Some(last) = self.delays.last_mut() {
                *last += delay;
            }
            return;
        }
        if image.width() > self.max || image.height() > self.max {
            let scale = self.max as f32 / image.width().max(image.height()) as f32;
            let (w, h) = (((image.width() as f32 * scale) as u32).max(1), ((image.height() as f32 * scale) as u32).max(1));
            image = image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
        }
        self.frames.push(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw()));
        self.delays.push(delay);
        let bytes: usize = self.frames.iter().map(|f| f.pixels.len() * 4).sum();
        if bytes > ANIMATION_BYTES || self.frames.len() >= ANIMATION_FRAMES {
            self.halve();
        }
    }

    /// Every other frame goes, its time to the one before.
    fn halve(&mut self) {
        let frames = std::mem::take(&mut self.frames);
        let delays = std::mem::take(&mut self.delays);
        for (i, frame) in frames.into_iter().enumerate() {
            if i % 2 == 0 {
                self.frames.push(frame);
            }
        }
        for pair in delays.chunks(2) {
            self.delays.push(pair.iter().sum());
        }
        self.stride *= 2;
    }

    fn take(mut self) -> Picture {
        if self.frames.len() < 2 {
            self.delays.clear();
        }
        Picture { frames: self.frames, delays: self.delays }
    }
}

/// Decodes a picture no larger than `max` a side, every frame of an animated
/// GIF, WebP or APNG.
fn decode_picture(bytes: &[u8], max: u32, still: bool) -> anyhow::Result<Picture> {
    use image::AnimationDecoder;
    use std::io::Cursor;
    let frames: Option<image::Frames<'_>> = match image::guess_format(bytes) {
        Ok(image::ImageFormat::Gif) => Some(image::codecs::gif::GifDecoder::new(Cursor::new(bytes))?.into_frames()),
        Ok(image::ImageFormat::WebP) => {
            let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
            decoder.has_animation().then(|| decoder.into_frames())
        }
        Ok(image::ImageFormat::Png) => {
            let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes))?;
            if decoder.is_apng()? { Some(decoder.apng()?.into_frames()) } else { None }
        }
        _ => None,
    };
    let mut reel = Reel::new(max);
    if let Some(frames) = frames {
        for frame in frames {
            // A broken frame ends it; what came before still plays.
            let Ok(frame) = frame else { break };
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            reel.push(frame.into_buffer(), numerator / denominator.max(1));
            if still {
                break;
            }
        }
    }
    if reel.frames.is_empty() {
        reel.push(image::load_from_memory(bytes)?.to_rgba8(), 0);
    }
    Ok(reel.take())
}

/// Downloads a picture into the downloads folder under its own name, with
/// " (2)" and on when that is taken. Returns where it went.
async fn save_image(http: &reqwest::Client, url: &str, name: &str) -> anyhow::Result<PathBuf> {
    let bytes = http.get(url).send().await?.error_for_status()?.bytes().await?;
    let folder = directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(std::path::Path::to_path_buf))
        .context("no downloads folder")?;
    let name = std::path::Path::new(name);
    let (stem, extension) = (name.file_stem().map_or_else(|| "image".into(), |s| s.to_string_lossy().into_owned()), name.extension().map(|e| e.to_string_lossy().into_owned()));
    let named = |n: u32| {
        let stem = if n == 1 { stem.clone() } else { format!("{stem} ({n})") };
        folder.join(match &extension { Some(e) => format!("{stem}.{e}"), None => stem })
    };
    let path = (1..).map(named).find(|p| !p.exists()).context("no free name")?;
    tokio::fs::write(&path, &bytes).await?;
    Ok(path)
}

/// Downloads a picture and puts it on the clipboard as one.
async fn copy_image(http: &reqwest::Client, url: &str) -> anyhow::Result<()> {
    let bytes = http.get(url).send().await?.error_for_status()?.bytes().await?;
    tokio::task::spawn_blocking(move || {
        let rgba = image::load_from_memory(&bytes)?.to_rgba8();
        let (width, height) = (rgba.width() as usize, rgba.height() as usize);
        arboard::Clipboard::new()?.set_image(arboard::ImageData { width, height, bytes: rgba.into_raw().into() })?;
        anyhow::Ok(())
    })
    .await?
}

/// A Lottie sticker (Discord's own packs) as a picture `max` pixels a
/// side: its first frame, or its middle one when it starts empty. Features
/// the renderer lacks are left out rather than losing the whole sticker.
pub fn lottie_frames(bytes: &[u8], max: u32, still: bool) -> anyhow::Result<Picture> {
    let animation = rasterlottie::Animation::from_json_str(std::str::from_utf8(bytes)?)?;
    let mut profile = rasterlottie::SupportProfile::target_corpus();
    profile.allow_unknown_shape_items = true;
    profile.allow_effects = true;
    profile.allow_expressions = true;
    let prepared = rasterlottie::Renderer::new(profile).prepare(&animation)?;
    let scale = max as f32 / animation.width.max(animation.height).max(1) as f32;
    let config = rasterlottie::RenderConfig::new(rasterlottie::Rgba8::TRANSPARENT, scale);
    if still {
        let mut frame = prepared.render_frame(animation.in_point, config)?;
        if frame.pixels.chunks_exact(4).all(|pixel| pixel[3] == 0) {
            frame = prepared.render_frame((animation.in_point + animation.out_point) / 2.0, config)?;
        }
        return Ok(Picture { frames: vec![egui::ColorImage::from_rgba_premultiplied([frame.width as usize, frame.height as usize], &frame.pixels)], delays: Vec::new() });
    }
    // At most 30 frames a second, and 90 in all: stickers run a few seconds.
    let span = (animation.out_point - animation.in_point).max(1.0);
    let rate = animation.frame_rate.max(1.0);
    let step = (rate / 30.0).max(span / 90.0).max(1.0);
    let delay = (step / rate * 1000.0) as u32;
    let mut reel = Reel::new(max);
    let mut at = animation.in_point;
    while at < animation.out_point {
        let frame = prepared.render_frame(at, config)?;
        // tiny-skia's pixels come premultiplied.
        reel.frames.push(egui::ColorImage::from_rgba_premultiplied([frame.width as usize, frame.height as usize], &frame.pixels));
        reel.delays.push(delay);
        at += step;
    }
    anyhow::ensure!(!reel.frames.is_empty(), "an empty animation");
    Ok(reel.take())
}

/// Audio settings for the voice task, from the app's preferences.
pub fn audio_settings(settings: &crate::settings::Settings) -> AudioSettings {
    AudioSettings {
        input_device: settings.input_device.clone(),
        output_device: settings.output_device.clone(),
        input_volume: settings.input_volume,
        output_volume: settings.output_volume,
        sensitivity_db: settings.input_sensitivity,
        automatic: settings.automatic_sensitivity,
        noise_suppression: settings.noise_suppression,
        echo_cancellation: settings.echo_cancellation,
        gain_control: settings.automatic_gain_control,
        advanced_activity: settings.advanced_voice_activity,
        ptt_sounds: !settings.sounds_off.iter().any(|s| s == "ptt_start" || s == "ptt_stop"),
        push_to_talk: settings.push_to_talk.then_some(settings.push_to_talk_key),
        user_volumes: settings
            .user_volumes
            .iter()
            .filter_map(|(id, v)| Some((id.parse().ok()?, *v)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::lottie_frames;

    /// A Lottie sticker's frame: a blue circle in the middle of a clear
    /// square, drawn at the size asked.
    #[test]
    fn draws_a_lottie_sticker() {
        let circle = r#"{"v":"5.7.4","fr":30,"ip":0,"op":30,"w":100,"h":100,"layers":[
            {"ty":4,"ind":1,"ip":0,"op":30,"st":0,
             "ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[50,50,0]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]}},
             "shapes":[{"ty":"gr","it":[
                {"ty":"el","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[60,60]}},
                {"ty":"fl","c":{"a":0,"k":[0.35,0.4,0.95,1]},"o":{"a":0,"k":100}},
                {"ty":"tr","p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}]}]}]}"#;
        let picture = lottie_frames(circle.as_bytes(), 160, false).unwrap();
        assert_eq!(picture.frames.len(), 30);
        assert_eq!(picture.delays[0], 33);
        assert_eq!(lottie_frames(circle.as_bytes(), 160, true).unwrap().frames.len(), 1);
        let image = &picture.frames[0];
        assert_eq!(image.size, [160, 160]);
        let middle = image.pixels[80 * 160 + 80];
        assert!(middle.a() == 255 && middle.b() > 200 && middle.r() < 120, "{middle:?}");
        assert_eq!(image.pixels[0].a(), 0);
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    fn gif(frames: usize, side: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for i in 0..frames {
                let buffer = image::RgbaImage::from_pixel(side, side, image::Rgba([(i * 40 % 255) as u8, 0, 0, 255]));
                let delay = image::Delay::from_numer_denom_ms(50, 1);
                encoder.encode_frame(image::Frame::from_parts(buffer, 0, 0, delay)).unwrap();
            }
        }
        bytes
    }

    #[test]
    fn a_gif_keeps_its_frames_and_times() {
        assert_eq!(decode_picture(&gif(4, 16), 64, true).unwrap().frames.len(), 1);
        let picture = decode_picture(&gif(4, 16), 64, false).unwrap();
        assert_eq!(picture.frames.len(), 4);
        assert_eq!(picture.delays, [50, 50, 50, 50]);
    }

    #[test]
    fn a_still_picture_has_one_frame_and_no_clock() {
        let mut bytes = Vec::new();
        image::RgbaImage::new(8, 8).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        let picture = decode_picture(&bytes, 64, false).unwrap();
        assert_eq!(picture.frames.len(), 1);
        assert!(picture.delays.is_empty());
    }

    #[test]
    fn a_long_animation_stays_in_its_budget_and_its_length() {
        let mut reel = Reel::new(512);
        for _ in 0..200 {
            reel.push(image::RgbaImage::new(512, 512), 50);
        }
        let picture = reel.take();
        let bytes: usize = picture.frames.iter().map(|f| f.pixels.len() * 4).sum();
        assert!(bytes <= ANIMATION_BYTES, "{bytes} bytes");
        assert_eq!(picture.frames.len(), picture.delays.len());
        assert_eq!(picture.delays.iter().sum::<u32>(), 200 * 50, "it lasts as long");
    }

    #[test]
    fn large_frames_shrink_to_what_is_drawn() {
        let picture = decode_picture(&gif(2, 100), 25, false).unwrap();
        assert_eq!(picture.frames[0].size, [25, 25]);
    }
}
