//! Textures for avatars, icons and attachments. The runtime downloads and
//! shrinks each picture; this keeps the textures the window draws and drops
//! the ones it has not drawn for a while, so memory follows the screen.

use std::collections::HashMap;

use egui::{Color32, Rect, TextureHandle, TextureId, Vec2};

use crate::backend::{Command, Commands};

/// Textures kept before the least recently drawn are dropped.
const KEEP: usize = 300;

/// Download attempts before a picture is left alone.
const TRIES: u8 = 5;

/// `asked` is the largest size wanted so far: one URL is drawn at many (a
/// server's icon inside a closed folder and alone in the rail), and the
/// first must not set how sharp the others look.
enum Slot {
    Loading { asked: u32, tries: u8 },
    Ready { texture: TextureHandle, used: u64, asked: u32, average: Color32 },
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
}

impl Images {
    pub fn new(commands: Commands) -> Self {
        Self {
            commands,
            slots: HashMap::new(),
            frame: 0,
        }
    }

    /// The texture for `url`, asking for it at most `max` pixels a side the
    /// first time.
    pub fn get(&mut self, url: &str, max: u32) -> Option<(TextureId, Vec2)> {
        match self.slots.get_mut(url) {
            Some(Slot::Ready { texture, used, asked, .. }) => {
                *used = self.frame;
                let found = (texture.id(), texture.size_vec2());
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

    pub fn loaded(&mut self, ctx: &egui::Context, url: String, image: Option<egui::ColorImage>) {
        let (asked, tries) = match self.slots.get(&url) {
            Some(Slot::Loading { asked, tries } | Slot::Failed { asked, tries, .. }) => (*asked, *tries),
            Some(Slot::Ready { asked, .. }) => (*asked, 0),
            None => (0, 0),
        };
        let slot = match image {
            Some(image) => Slot::Ready {
                average: average(&image),
                texture: ctx.load_texture(&url, image, egui::TextureOptions::LINEAR),
                used: self.frame,
                asked,
            },
            // A sharper copy that failed: the one on screen stays.
            None if matches!(self.slots.get(&url), Some(Slot::Ready { .. })) => return,
            None => Slot::Failed { asked, tries: tries + 1, at: std::time::Instant::now() },
        };
        self.slots.insert(url, slot);
    }

    /// The window is gone and its textures with it.
    pub fn clear(&mut self) {
        for url in self.slots.drain().map(|(url, _)| url) {
            self.commands.send(Command::Forget { url });
        }
    }

    /// Ends a frame: drops the least recently drawn textures past [`KEEP`].
    /// A dropped picture is fetched again if it comes back on screen.
    pub fn end_frame(&mut self) {
        self.frame += 1;
        let ready = self
            .slots
            .values()
            .filter(|s| matches!(s, Slot::Ready { .. }))
            .count();
        if ready <= KEEP {
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
        for (_, url) in ages.into_iter().take(ready - KEEP * 3 / 4) {
            self.slots.remove(&url);
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
