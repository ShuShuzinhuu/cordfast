# Contributing to CordFast

Thank you for helping. CordFast aims to be **all of Discord, looking and
behaving the same, in a light native app**. These rules keep it that way.

## Help wanted: camera and Go Live

Camera and screen sharing (Go Live) are the largest missing pieces. Discord
has them turned off in the country where CordFast is developed, so they cannot
be built and tried against the real service here, and a VPN is not a fair
test. If you can use them where you live, your help is especially welcome:

- Video attachments already play in `src/video.rs` (Media Foundation, Windows).
- Watching a stream needs a second voice connection (gateway ops 18–22, DAVE
  and RTP video); sending needs capture, encoding and the same protocol.
- Open an issue first to say what you are taking on, and describe in the pull
  request how you tested it against Discord.

## Ground rules

1. **Faithful to Discord.** Icons, menus, colours, texts, sounds and behaviour
   come from Discord's own client, not from guesses. Look in the web client's
   bundle (`https://discord.com/app` names its `/assets/<hash>.js` files) for
   endpoints, opcodes, protobuf fields, SVG paths and English strings, and say
   in the pull request where you found them.
2. **Light.** No browser engine, no per-frame clones, no constant repaint. If
   you touch voice or anything that runs all the time, measure CPU and RAM
   before and after, and put the numbers in the pull request.
3. **Never inside a game.** Nothing may be injected into another process (no
   DLLs, no hooks): anti-cheat systems ban accounts for it. The overlay is a
   window of its own.
4. **English on GitHub.** Code, comments, logs, commit messages, issues and
   pull requests are in English. The only exception is
   `docs/README.pt-BR.md`, which follows `README.md`.
5. **Every language.** Text shown to people is English in the code (`t!`,
   `tn!`, `n!`, `tp!`). After adding strings, run `python3 scripts/i18n.py`
   and fill the new entries in every catalog in `assets/i18n/`, using
   Discord's own wording in that language where it exists.
   `python3 scripts/i18n.py --check` must list nothing.
6. **The house style.** Do not run `cargo fmt`: the code is formatted by hand
   and fmt would rewrite every file. Match the code around you, and explain
   *why* in `///` comments. Settings pages use `src/ui/form.rs`, menus use
   `src/ui/menu.rs`; no bare `ui.button`/`ui.checkbox` there.
7. **Tests pass.** `cargo test` before every pull request. Screens can be
   checked without an account:
   `CORDFAST_DEMO=tests/fixtures/demo.json CORDFAST_PREVIEW=guild:0 cargo run`.
8. **Small commits.** One change per commit, with a message that says what
   changed for the person using CordFast.
9. **No secrets.** Never commit tokens, passwords, signing keys or captures
   that show someone's private messages. Use the demo session for
   screenshots.

## Pull requests

- Branch from `master` and open the pull request against it.
- Say what changed, how you tested it (on which system, and against the real
  Discord or the demo session) and what you could not test.
- Add a screenshot for anything visible, next to Discord's own screen if you
  can.
- Releases and version numbers are the maintainer's.

## Your account

CordFast is an unofficial client: using it, or testing changes with it, breaks
Discord's terms of service and could get an account suspended. Test with an
account you accept that risk for.

## License

By contributing, you agree that your contribution is licensed under the
[MIT License](LICENSE).
