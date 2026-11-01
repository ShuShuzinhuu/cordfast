//! Textures for avatars, icons and attachments. The runtime downloads and
//! shrinks each picture; this keeps the textures the window draws and drops
//! the ones it has not drawn for a while, so memory follows the screen.

use std::collections::HashMap;

use egui::{Rect, TextureHandle, TextureId, Vec2};

use crate::backend::{Command, Commands};

/// Textures kept before the least recently drawn are dropped.
const KEEP: usize = 300;

enum Slot {
    Loading,
    Ready { texture: TextureHandle, used: u64 },
    Failed,
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
            Some(Slot::Ready { texture, used }) => {
                *used = self.frame;
                Some((texture.id(), texture.size_vec2()))
            }
            Some(_) => None,
            None => {
                self.slots.insert(url.to_owned(), Slot::Loading);
                self.commands.send(Command::FetchImage {
                    url: url.to_owned(),
                    max,
                });
                None
            }
        }
    }

    pub fn loaded(&mut self, ctx: &egui::Context, url: String, image: Option<egui::ColorImage>) {
        let slot = match image {
            Some(image) => Slot::Ready {
                texture: ctx.load_texture(&url, image, egui::TextureOptions::LINEAR),
                used: self.frame,
            },
            None => Slot::Failed,
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
        let max = (rect.width() * ppp).ceil() as u32;
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
