//! Application ("slash") commands: the index Discord gives a server or a
//! conversation (`/application-command-index`), the commands flattened to
//! what the picker lists (a command, or each of its subcommands), Discord's
//! own built-in commands, and the data an interaction sends back.

use serde_json::{Value, json};

use super::model::{Id, sf};

/// The application of the built-in commands, which Discord runs itself.
pub const BUILT_IN: Id = 0;

/// An application with commands: its name and icon in the picker.
#[derive(Clone, Debug)]
pub struct CommandApp {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
}

/// A command's option, as Discord types it: 3 text, 4 integer, 5 boolean,
/// 6 user, 7 channel, 8 role, 9 user or role, 10 number, 11 attachment;
/// 1 and 2 are subcommands and their groups.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandOption {
    pub kind: u64,
    pub name: String,
    pub description: String,
    pub required: bool,
    /// Fixed choices: (shown name, value sent).
    pub choices: Vec<(String, Value)>,
    /// The application suggests values as they are typed.
    pub autocomplete: bool,
    pub options: Vec<CommandOption>,
}

impl CommandOption {
    fn parse(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        Some(Self {
            kind: value.get("type")?.as_u64()?,
            name: value.get("name")?.as_str()?.to_owned(),
            description: text("description"),
            required: value.get("required").and_then(Value::as_bool).unwrap_or(false),
            choices: value
                .get("choices")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(|c| Some((c.get("name")?.as_str()?.to_owned(), c.get("value")?.clone()))).collect())
                .unwrap_or_default(),
            autocomplete: value.get("autocomplete").and_then(Value::as_bool).unwrap_or(false),
            options: options(value),
        })
    }

    /// A user, channel or role (or either of the first two): a value picked
    /// from a list, not typed.
    pub fn picks_entity(&self) -> bool {
        matches!(self.kind, 6..=9)
    }
}

fn options(value: &Value) -> Vec<CommandOption> {
    value.get("options").and_then(Value::as_array).map(|list| list.iter().filter_map(CommandOption::parse).collect()).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub struct SlashCommand {
    pub id: Id,
    pub application: Id,
    pub version: String,
    pub name: String,
    pub description: String,
    pub options: Vec<CommandOption>,
    /// The command as Discord sent it: an interaction carries it back.
    pub raw: Value,
}

/// What the picker lists: a command, or one subcommand of it, with the
/// options it takes.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The command, in `Index::commands`.
    pub command: usize,
    /// The group and subcommand after the command's name, if any.
    pub path: Vec<String>,
    pub description: String,
    pub options: Vec<CommandOption>,
}

#[derive(Clone, Debug, Default)]
pub struct Index {
    pub applications: Vec<CommandApp>,
    pub commands: Vec<SlashCommand>,
    pub entries: Vec<Entry>,
}

impl Index {
    /// The answer of `/guilds/{id}/application-command-index` (or a
    /// channel's), with Discord's built-in commands after the apps'.
    pub fn parse(value: &Value) -> Self {
        let mut index = Self {
            applications: value
                .get("applications")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    let id = a.get("id").and_then(sf)?;
                    let icon = a.get("icon").and_then(Value::as_str).map(|hash| format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=64"));
                    Some(CommandApp { id, name: a.get("name")?.as_str()?.to_owned(), icon })
                })
                .collect(),
            ..Self::default()
        };
        for raw in value.get("application_commands").and_then(Value::as_array).into_iter().flatten() {
            // Chat input commands only: user and message ones live in menus.
            if raw.get("type").and_then(Value::as_u64).unwrap_or(1) != 1 {
                continue;
            }
            let Some(command) = (|| {
                Some(SlashCommand {
                    id: raw.get("id").and_then(sf)?,
                    application: raw.get("application_id").and_then(sf)?,
                    version: raw.get("version").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    name: raw.get("name")?.as_str()?.to_owned(),
                    description: raw.get("description").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    options: options(raw),
                    raw: raw.clone(),
                })
            })() else {
                continue;
            };
            index.commands.push(command);
        }
        index.commands.sort_by(|a, b| a.name.cmp(&b.name));
        index.commands.extend(built_in());
        index.entries = index.commands.iter().enumerate().flat_map(|(i, c)| entries(i, c)).collect();
        index
    }

    /// Only Discord's built-in commands, for a conversation whose index has
    /// not come yet or has no apps.
    pub fn built_in_only() -> Self {
        Self::parse(&Value::Null)
    }

    pub fn app(&self, id: Id) -> Option<&CommandApp> {
        self.applications.iter().find(|a| a.id == id)
    }

    /// "queue loop on": a command's name with its group and subcommand, as
    /// typed after the slash.
    fn name(&self, entry: &Entry) -> String {
        std::iter::once(self.commands[entry.command].name.as_str()).chain(entry.path.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }

    /// The entries a typed "/word" finds: by the start of the name first,
    /// then anywhere in it. Subcommands go after a space ("/queue l").
    pub fn search(&self, query: &str) -> Vec<usize> {
        let query = query.to_lowercase();
        let mut found: Vec<(bool, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let n = self.name(e).to_lowercase();
                if n.starts_with(&query) {
                    Some((false, i))
                } else if n.contains(&query) {
                    Some((true, i))
                } else {
                    None
                }
            })
            .collect();
        found.sort_by_key(|(later, _)| *later);
        found.into_iter().map(|(_, i)| i).collect()
    }

    /// The entries named exactly what was typed ("queue show"), whatever
    /// the case: more than one when two apps share a name.
    pub fn exact(&self, typed: &str) -> Vec<usize> {
        let typed = typed.to_lowercase();
        (0..self.entries.len()).filter(|&i| self.name(&self.entries[i]).to_lowercase() == typed).collect()
    }

    /// "/play" or "/music play": what the picker and the composer show.
    pub fn title(&self, entry: &Entry) -> String {
        format!("/{}", self.name(entry))
    }
}

/// A command's runnable entries: itself, or each subcommand (in its group).
fn entries(index: usize, command: &SlashCommand) -> Vec<Entry> {
    let subs: Vec<&CommandOption> = command.options.iter().filter(|o| o.kind == 1 || o.kind == 2).collect();
    if subs.is_empty() {
        return vec![Entry { command: index, path: Vec::new(), description: command.description.clone(), options: command.options.clone() }];
    }
    let mut out = Vec::new();
    for sub in subs {
        if sub.kind == 2 {
            for leaf in sub.options.iter().filter(|o| o.kind == 1) {
                out.push(Entry { command: index, path: vec![sub.name.clone(), leaf.name.clone()], description: leaf.description.clone(), options: leaf.options.clone() });
            }
        } else {
            out.push(Entry { command: index, path: vec![sub.name.clone()], description: sub.description.clone(), options: sub.options.clone() });
        }
    }
    out
}

/// Discord's built-in commands that change the message itself, run here as
/// Discord's client runs them (descriptions from memory of the client).
fn built_in() -> Vec<SlashCommand> {
    let message = |required: bool| CommandOption {
        kind: 3,
        name: "message".into(),
        description: t!("Your message").into(),
        required,
        choices: Vec::new(),
        autocomplete: false,
        options: Vec::new(),
    };
    [
        ("shrug", n!("Appends ¯\\_(ツ)_/¯ to your message."), false),
        ("tableflip", n!("Appends (╯°□°)╯︵ ┻━┻ to your message."), false),
        ("unflip", n!("Appends ┬─┬ノ( º _ ºノ) to your message."), false),
        ("me", n!("Displays text with emphasis."), true),
        ("spoiler", n!("Marks your message as a spoiler."), true),
    ]
    .into_iter()
    .map(|(name, description, required)| SlashCommand {
        id: 0,
        application: BUILT_IN,
        version: String::new(),
        name: name.into(),
        description: crate::i18n::tr(description).into(),
        options: vec![message(required)],
        raw: Value::Null,
    })
    .collect()
}

/// The text a built-in command sends, from what was written in it. The
/// shrug's backslash and underscores are escaped, so markdown shows them.
pub fn built_in_text(name: &str, message: &str) -> String {
    let message = message.trim();
    match name {
        "shrug" => format!("{message} ¯\\\\\\_(ツ)\\_/¯"),
        "tableflip" => format!("{message} (╯°□°)╯︵ ┻━┻"),
        "unflip" => format!("{message} ┬─┬ノ( º _ ºノ)"),
        "me" => format!("_{message}_"),
        "spoiler" => format!("||{message}||"),
        _ => message.to_owned(),
    }
    .trim()
    .to_owned()
}

/// An option's value from what was typed, by its type; none when it does
/// not read as one ("abc" for an integer).
pub fn typed_value(option: &CommandOption, text: &str) -> Option<Value> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match option.kind {
        4 => text.parse::<i64>().ok().map(Value::from),
        10 => text.replace(',', ".").parse::<f64>().ok().map(Value::from),
        5 => match text.to_lowercase().as_str() {
            "true" | "yes" | "1" => Some(Value::Bool(true)),
            "false" | "no" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        _ => Some(Value::String(text.to_owned())),
    }
}

/// The interaction's `data` for an entry and its filled options
/// (`(option, value)`), wrapped in its subcommand and group. A focused
/// option asks for autocomplete instead of running.
pub fn data(command: &SlashCommand, entry: &Entry, values: &[(&CommandOption, Value)], focused: Option<&str>) -> Value {
    let mut options: Vec<Value> = values
        .iter()
        .map(|(option, value)| {
            let mut o = json!({ "type": option.kind, "name": option.name, "value": value });
            if focused == Some(option.name.as_str()) {
                o["focused"] = Value::Bool(true);
            }
            o
        })
        .collect();
    // The subcommand holds the options; a group holds the subcommand.
    for (depth, name) in entry.path.iter().enumerate().rev() {
        let kind = if depth + 1 == entry.path.len() { 1 } else { 2 };
        options = vec![json!({ "type": kind, "name": name, "options": options })];
    }
    json!({
        "version": command.version,
        "id": command.id.to_string(),
        "name": command.name,
        "type": 1,
        "options": options,
        "application_command": command.raw,
        "attachments": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        Index::parse(&json!({
            "applications": [{ "id": "10", "name": "Rythm", "icon": "abc" }],
            "application_commands": [
                { "id": "1", "application_id": "10", "version": "7", "type": 1, "name": "play", "description": "Plays a song",
                  "options": [{ "type": 3, "name": "query", "description": "Song", "required": true, "autocomplete": true },
                              { "type": 4, "name": "volume", "description": "Volume" }] },
                { "id": "2", "application_id": "10", "version": "7", "type": 1, "name": "queue", "description": "The queue",
                  "options": [{ "type": 1, "name": "show", "description": "Shows it" },
                              { "type": 2, "name": "loop", "description": "Looping", "options": [
                                  { "type": 1, "name": "on", "description": "Loops", "options": [{ "type": 5, "name": "all", "description": "All" }] }] }] },
                { "id": "3", "application_id": "10", "version": "7", "type": 2, "name": "Profile" }
            ]
        }))
    }

    #[test]
    fn flattens_subcommands_and_skips_menu_commands() {
        let index = index();
        let titles: Vec<String> = index.entries.iter().map(|e| index.title(e)).collect();
        assert!(titles.contains(&"/play".to_owned()));
        assert!(titles.contains(&"/queue show".to_owned()));
        assert!(titles.contains(&"/queue loop on".to_owned()));
        assert!(titles.contains(&"/shrug".to_owned()));
        assert!(!titles.iter().any(|t| t.contains("Profile")));
        assert_eq!(index.search("qu").iter().map(|&i| index.title(&index.entries[i])).collect::<Vec<_>>(), ["/queue show", "/queue loop on"]);
    }

    #[test]
    fn finds_subcommands_after_a_space_and_exact_names() {
        let index = index();
        let titles = |found: Vec<usize>| found.iter().map(|&i| index.title(&index.entries[i])).collect::<Vec<_>>();
        assert_eq!(titles(index.search("queue ")), ["/queue show", "/queue loop on"]);
        assert_eq!(titles(index.search("queue loop")), ["/queue loop on"]);
        assert_eq!(titles(index.exact("Queue Show")), ["/queue show"]);
        assert_eq!(titles(index.exact("play")), ["/play"]);
        // A command with subcommands is no entry of its own.
        assert!(index.exact("queue").is_empty());
    }

    #[test]
    fn wraps_options_in_their_subcommand_and_group() {
        let index = index();
        let entry = index.entries.iter().find(|e| index.title(e) == "/queue loop on").unwrap();
        let command = &index.commands[entry.command];
        let option = &entry.options[0];
        let data = data(command, entry, &[(option, Value::Bool(true))], None);
        assert_eq!(data["name"], "queue");
        assert_eq!(data["options"][0]["type"], 2);
        assert_eq!(data["options"][0]["name"], "loop");
        assert_eq!(data["options"][0]["options"][0]["type"], 1);
        assert_eq!(data["options"][0]["options"][0]["options"][0], json!({ "type": 5, "name": "all", "value": true }));
        assert_eq!(data["application_command"]["id"], "2");
    }

    #[test]
    fn reads_typed_values_by_type_and_runs_built_ins() {
        let mut option = index().entries[0].options[0].clone();
        option.kind = 4;
        assert_eq!(typed_value(&option, "42"), Some(json!(42)));
        assert_eq!(typed_value(&option, "x"), None);
        option.kind = 10;
        assert_eq!(typed_value(&option, "1,5"), Some(json!(1.5)));
        assert_eq!(built_in_text("shrug", "ok"), "ok ¯\\\\\\_(ツ)\\_/¯");
        assert_eq!(built_in_text("spoiler", "it ends"), "||it ends||");
    }
}
