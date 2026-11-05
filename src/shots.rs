//! `CORDFAST_SHOT=<base>` saves what the window itself draws as `<base>-N.png`,
//! a few frames apart, for checking the screen and whether animations move.
//! It reads the renderer's own framebuffer: screen captures through GDI can get
//! an empty white window where OpenGL draws. `CORDFAST_SHOT_AFTER` is the wait
//! before the first one, in seconds.

use std::time::{Duration, Instant};

const COUNT: u32 = 4;
const GAP: Duration = Duration::from_millis(400);
const WAIT: f32 = 15.0;

pub struct Shots {
    base: String,
    start: Instant,
    wait: Duration,
    taken: u32,
    asked: bool,
}

/// Whether this run takes shots: animations then play without the window's
/// focus, so that checking them does not take it from what is in use.
pub fn active() -> bool {
    std::env::var_os("CORDFAST_SHOT").is_some()
}

impl Shots {
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("CORDFAST_SHOT").ok()?;
        let wait = std::env::var("CORDFAST_SHOT_AFTER").ok().and_then(|wait| wait.parse().ok()).unwrap_or(WAIT);
        Some(Self { base, start: Instant::now(), wait: Duration::from_secs_f32(wait), taken: 0, asked: false })
    }

    /// Runs on every frame: saves what arrived and asks for the next one when due.
    pub fn frame(&mut self, ctx: &egui::Context) {
        let images: Vec<_> = ctx.input(|i| i.raw.events.iter().filter_map(|event| if let egui::Event::Screenshot { image, .. } = event { Some(image.clone()) } else { None }).collect());
        for image in images {
            self.taken += 1;
            self.asked = false;
            let path = format!("{}-{}.png", self.base, self.taken);
            let pixels = image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect();
            let saved = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, pixels).map(|shot| shot.save(&path));
            log::info!("shot: {path}: {saved:?}");
        }
        if self.taken >= COUNT {
            return;
        }
        if self.asked {
            ctx.request_repaint();
            return;
        }
        let due = self.wait + GAP * self.taken;
        match due.checked_sub(self.start.elapsed()) {
            Some(left) if !left.is_zero() => ctx.request_repaint_after(left),
            _ => {
                self.asked = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                ctx.request_repaint();
            }
        }
    }
}
