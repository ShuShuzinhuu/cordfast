//! Textures for avatars, icons and attachments. The runtime downloads and
//! shrinks each picture; this keeps the textures the window draws and drops
//! the ones it has not drawn for a while, so memory follows the screen.

use std::collections::HashMap;

use egui::{Color32, Rect, TextureHandle, TextureId, Vec2};

use crate::backend::{Command, Commands};

/// Textures kept before the least recently drawn are dropped.
const KEEP: usize = 300;
/// Pixels kept before the least recently drawn are dropped: animations hold
/// many frames, and what is on screen always stays.
const BUDGET: usize = 96 << 20;
/// The fastest an animation repaints the window.
const FASTEST_FRAME_MS: u64 = 33;

/// Download attempts before a picture is left alone.
const TRIES: u8 = 5;

/// `asked` is the largest size wanted so far: one URL is drawn at many (a
/// server's icon inside a closed folder and alone in the rail), and the
/// first must not set how sharp the others look.
enum Slot {
    Loading { asked: u32, tries: u8 },
    Ready { frames: Vec<TextureHandle>, ends: Vec<u64>, bytes: usize, used: u64, asked: u32, average: Color32 },
    Failed { asked: u32, tries: u8, at: std::time::Instant },
}

/// The picture's average colour, ignoring what is see-through: a voice
/// tile's background.
fn average(image: &egui::ColorImage) -> Color32 {
    let step = (image.pixels.len() / 1024).max(1);
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for pixel in image.pixels.iter().step_by(step).filter(|p| p.a() > 128) {
        r += u64::from(pixel.r());
        g += u64::from(pixel.g());
        b += u64::from(pixel.b());
        n += 1;
    }
    let n = n.max(1);
    Color32::from_rgb((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

pub struct Images {
    commands: Commands,
    slots: HashMap<String, Slot>,
    frame: u64,
    /// The clock animations follow, in milliseconds.
    now: u64,
    /// Whether animations run: "Play GIFs when Discord is focused".
    focused: bool,
    /// When the next frame on screen changes, from now.
    next: Option<u64>,
    bytes: usize,
}

impl Images {
    pub fn new(commands: Commands) -> Self {
        Self {
            commands,
            slots: HashMap::new(),
            frame: 0,
            now: 0,
            focused: true,
            next: None,
            bytes: 0,
        }
    }

    /// The texture for `url`, asking for it at most `max` pixels a side the
    /// first time.
    pub fn get(&mut self, url: &str, max: u32) -> Option<(TextureId, Vec2)> {
        self.get_playing(url, max, true)
    }

    /// As [`Images::get`], an animation holding its first frame unless `play`.
    pub fn get_playing(&mut self, url: &str, max: u32, play: bool) -> Option<(TextureId, Vec2)> {
        match self.slots.get_mut(url) {
            Some(Slot::Ready { frames, ends, used, asked, .. }) => {
                *used = self.frame;
                let mut shown = 0;
                if play && self.focused && let Some(total) = ends.last().copied().filter(|t| *t > 0) {
                    let at = self.now % total;
                    shown = ends.partition_point(|end| *end <= at);
                    let left = ends[shown] - at;
                    self.next = Some(self.next.map_or(left, |next| next.min(left)));
                }
                let found = (frames[shown].id(), frames[0].size_vec2());
                // Drawn larger than it was fetched: the small one shows until
                // the sharper one arrives.
                if max > *asked {
                    *asked = max;
                    self.commands.send(Command::Forget { url: url.to_owned() });
                    self.commands.send(Command::FetchImage { url: url.to_owned(), max });
                }
                Some(found)
            }
            Some(Slot::Loading { .. }) => None,
            Some(Slot::Failed { asked, tries, at }) => {
                // A failed download (the network, a rate limit) would stay
                // an empty box until restart: tried again, each time later.
                if *tries < TRIES && at.elapsed().as_secs() >= 5 * u64::from(*tries) {
                    let (max, tries) = ((*asked).max(max), *tries);
                    self.slots.insert(url.to_owned(), Slot::Loading { asked: max, tries });
                    self.commands.send(Command::Forget { url: url.to_owned() });
                    self.commands.send(Command::FetchImage { url: url.to_owned(), max });
                }
                None
            }
            None => {
                self.slots.insert(url.to_owned(), Slot::Loading { asked: max, tries: 0 });
                self.commands.send(Command::FetchImage {
                    url: url.to_owned(),
                    max,
                });
                None
            }
        }
    }

    /// The average colour of a picture already here.
    pub fn average(&self, url: &str) -> Option<Color32> {
        match self.slots.get(url) {
            Some(Slot::Ready { average, .. }) => Some(*average),
            _ => None,
        }
    }

    pub fn loaded(&mut self, ctx: &egui::Context, url: String, image: Option<crate::backend::Picture>) {
        let (asked, tries) = match self.slots.get(&url) {
            Some(Slot::Loading { asked, tries } | Slot::Failed { asked, tries, .. }) => (*asked, *tries),
            Some(Slot::Ready { asked, .. }) => (*asked, 0),
            None => (0, 0),
        };
        let slot = match image.filter(|picture| !picture.frames.is_empty()) {
            Some(picture) => {
                let color = average(&picture.frames[0]);
                let bytes = picture.frames.iter().map(|f| f.pixels.len() * 4).sum();
                let mut end = 0;
                let ends = picture.delays.iter().map(|delay| {
                    end += u64::from(*delay);
                    end
                }).collect();
                let frames = picture.frames.into_iter().enumerate().map(|(i, frame)| ctx.load_texture(format!("{url}#{i}"), frame, egui::TextureOptions::LINEAR)).collect();
                Slot::Ready { frames, ends, bytes, used: self.frame, asked, average: color }
            }
            // A sharper copy that failed: the one on screen stays.
            None if matches!(self.slots.get(&url), Some(Slot::Ready { .. })) => return,
            None => Slot::Failed { asked, tries: tries + 1, at: std::time::Instant::now() },
        };
        if let Some(Slot::Ready { bytes, .. }) = self.slots.insert(url, slot) {
            self.bytes -= bytes;
        }
        self.bytes = self.slots.values().map(|slot| if let Slot::Ready { bytes, .. } = slot { *bytes } else { 0 }).sum();
    }

    /// Starts a frame: the clock animations follow, and whether they run.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        let (time, focused) = ctx.input(|i| (i.time, i.viewport().focused.unwrap_or(true)));
        self.now = (time * 1000.0) as u64;
        self.focused = focused;
        self.next = None;
    }

    /// The window is gone and its textures with it.
    pub fn clear(&mut self) {
        for url in self.slots.drain().map(|(url, _)| url) {
            self.commands.send(Command::Forget { url });
        }
    }

    /// Ends a frame: drops the least recently drawn textures past [`KEEP`].
    /// A dropped picture is fetched again if it comes back on screen.
    pub fn end_frame(&mut self, ctx: &egui::Context) {
        // Only what was drawn this frame asks for the next one.
        if let Some(left) = self.next {
            ctx.request_repaint_after(std::time::Duration::from_millis(left.max(FASTEST_FRAME_MS)));
        }
        self.frame += 1;
        let ready = self
            .slots
            .values()
            .filter(|s| matches!(s, Slot::Ready { .. }))
            .count();
        if ready <= KEEP && self.bytes <= BUDGET {
            return;
        }
        let mut ages: Vec<(u64, String)> = self
            .slots
            .iter()
            .filter_map(|(url, slot)| match slot {
                Slot::Ready { used, .. } => Some((*used, url.clone())),
                _ => None,
            })
            .collect();
        ages.sort_unstable();
        let by_count = ready.saturating_sub(KEEP * 3 / 4);
        for (index, (used, url)) in ages.into_iter().enumerate() {
            // Never what was just drawn: it would only be fetched again.
            if used + 1 >= self.frame || (index >= by_count && self.bytes <= BUDGET * 3 / 4) {
                break;
            }
            if let Some(Slot::Ready { bytes, .. }) = self.slots.remove(&url) {
                self.bytes -= bytes;
            }
            self.commands.send(Command::Forget { url });
        }
    }

    /// Paints a round picture, or a grey circle with `initials` while it loads.
    pub fn circle(&mut self, ui: &egui::Ui, rect: Rect, url: Option<&str>, initials: &str, rounding: f32) {
        let ppp = ui.ctx().pixels_per_point();
        // Never tiny: most pictures are drawn at several sizes.
        let max = ((rect.width() * ppp).ceil() as u32).max(64);
        match url.and_then(|url| self.get(url, max)) {
            Some((texture, _)) => {
                egui::Image::from_texture((texture, rect.size()))
                    .corner_radius(rounding)
                    .paint_at(ui, rect);
            }
            None => {
                ui.painter()
                    .rect_filled(rect, rounding, crate::theme::p().selected);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    initials,
                    egui::FontId::proportional(rect.height() * 0.36),
                    crate::theme::p().text,
                );
            }
        }
    }
}
