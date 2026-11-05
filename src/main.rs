//! CordFast: a fast, native Discord client in Rust and egui, built the way
//! ZapFast and Spotifast are. No browser engine.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// First, so its t!, tn! and n! reach every module after it.
#[macro_use]
mod i18n;
mod admin;
mod app;
mod backend;
mod demo;
mod discord;
mod fonts;
mod icon;
mod icons;
mod images;
mod keybinds;
mod plugins;
mod servertest;
mod settings;
mod shots;
mod sounds;
mod text;
mod theme;
mod ui;
mod updates;
mod video;
mod voice;

use std::sync::Arc;

/// One window over the app, which the shell keeps between windows.
struct Window {
    app: fastframe_shell::Held<app::App>,
}

impl eframe::App for Window {
    /// Runs even while the window is minimized, so closing it from the
    /// taskbar still hides to the tray.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.window_logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.frame(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::p().chat.to_normalized_gamma_f32()
    }
}

fn tray(waker: &fastframe_shell::Waker) -> Option<fastframe_tray::Tray> {
    use fastframe_tray::{Config, MenuItem};
    let waker = waker.clone();
    fastframe_tray::Tray::spawn(
        Config {
            id: "cordfast",
            title: "CordFast".into(),
            icon: icon::rgba,
            template_icon: None,
            themed_icon: false,
            menu: vec![
                MenuItem::action("show", t!("Open CordFast")),
                MenuItem::action("mute", t!("Mute Microphone")).visible(false),
                MenuItem::Separator,
                MenuItem::action("quit", t!("Quit CordFast")),
            ],
        },
        move || waker.wake(),
    )
}

fn main() -> eframe::Result {
    // First: run as the update helper when asked, and take the update
    // flags off the command line.
    let launch = fastframe_update::intercept(&updates::CONFIG);
    // The version probe the updater runs on a downloaded copy.
    if launch.arguments.iter().any(|a| a == "--version") {
        println!("cordfast {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if let Ok(out) = std::env::var("CORDFAST_SERVERTEST") {
        servertest::run(&out);
        return Ok(());
    }
    if let Ok(out) = std::env::var("CORDFAST_SELFTEST") {
        backend::selftest(&out);
        return Ok(());
    }
    // A log file per run (the previous one is kept), for bug reports.
    let logs = settings::config_dir().join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let _ = fastframe_log::Logging::new("CordFast", env!("CARGO_PKG_VERSION"))
        // CORDFAST_LOG widens it for a diagnosis ("songbird=debug,davey=debug").
        .filter(&std::env::var("CORDFAST_LOG").map_or_else(|_| "cordfast=info,songbird=warn,davey=warn".to_owned(), |extra| format!("cordfast=info,songbird=warn,davey=warn,{extra}")))
        .file(logs.join("cordfast.log"))
        .panic_log(logs.join("panics.log"))
        .init();
    let waker = fastframe_shell::Waker::default();
    let wake: backend::Wake = {
        let waker = waker.clone();
        Arc::new(move || waker.wake())
    };
    let mut app = app::App::new(wake.clone());
    app.tray = tray(&waker);
    if let Some(error) = launch.error {
        app.notify(if error == fastframe_update::Launch::RESTORED { t!("The update did not start; the previous version was restored.").to_owned() } else { error });
    }
    // A relaunch after an update: confirm it started, or the helper rolls back.
    if let Some(receipt) = launch.receipt {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if let Err(error) = receipt.acknowledge() {
                log::warn!("update: {error:#}");
            }
        });
    }
    updates::spawn(app.updates.clone(), wake.clone());
    app.sync_global_keybinds();
    keybinds::watch(app.keybinds.clone(), wake.clone());
    discord::detect::watch_front(wake);
    // Started at login in the tray, as the autostart entry asks.
    let hidden = launch.arguments.iter().any(|a| a == fastframe_autostart::START_HIDDEN);
    // Discord's own title bar, unless the system's was chosen.
    fastframe_titlebar::set_enabled(!app.settings.native_titlebar);
    fastframe_shell::Shell::new(app, &waker).start_hidden(hidden).run(|lease| {
        let viewport = egui::ViewportBuilder::default()
            .with_title("CordFast")
            .with_app_id("cordfast")
            .with_icon(icon::window_icon())
            .with_inner_size([1240.0, 800.0])
            .with_min_inner_size([760.0, 480.0])
            .with_drag_and_drop(true)
            .with_decorations(!fastframe_titlebar::enabled());
        let options = eframe::NativeOptions {
            viewport,
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        };
        eframe::run_native(
            "CordFast",
            options,
            Box::new(move |cc| {
                let mut app = lease.take(&cc.egui_ctx);
                app.attach(&cc.egui_ctx);
                Ok(Box::new(Window { app }))
            }),
        )
    })
}
