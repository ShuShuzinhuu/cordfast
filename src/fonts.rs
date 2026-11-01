//! More fallback faces than fastframe-fonts registers. It finds one face
//! per common script; Discord names are full of the rest: Egyptian
//! hieroglyphs (𓂃), Linear A (𐙚), Bamum (𖦹), syllabics (ᐢ), Lepcha (ᰔ),
//! small capitals (ᴀʙᴄ), braille (⣿), dingbats (✧), arrows and newer
//! geometric shapes. Chromium, and so Discord, finds some installed face
//! for each of them; without one egui draws a box. The installed fonts are
//! probed once, on a thread of its own so the window opens at once, and the
//! faces that draw something nothing registered draws are added behind the
//! others.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

/// A character from each block worth a face of its own.
const PROBES: &[char] = &[
    '\u{13083}', // Egyptian hieroglyphs 𓂃
    '\u{1065a}', // Linear A 𐙚
    '\u{10000}', // Linear B
    '\u{a6a0}',  // Bamum
    '\u{169b9}', // Bamum supplement 𖦹
    '\u{1401}',  // Canadian syllabics ᐁ
    '\u{1422}',  // ᐢ
    '\u{1c00}',  // Lepcha ᰀ
    '\u{1c5a}',  // Ol Chiki
    '\u{1d00}',  // Phonetic extensions ᴀ
    '\u{1d43}',  // Modifier letters ᵃ
    '\u{28ff}',  // Braille ⣿
    '\u{2727}',  // Dingbats ✧
    '\u{231b}',  // Miscellaneous technical ⌛
    '\u{2b1f}',  // Miscellaneous symbols and arrows ⬟
    '\u{25cc}',  // Geometric shapes ◌
    '\u{1f784}', // Geometric shapes extended 🞄
    '\u{2a00}',  // Supplemental mathematical operators
    '\u{16a0}',  // Runic ᚠ
    '\u{1680}',  // Ogham
    '\u{2d30}',  // Tifinagh
    '\u{07ca}',  // N'Ko
    '\u{a500}',  // Vai
    '\u{1820}',  // Mongolian
    '\u{0f40}',  // Tibetan
    '\u{0710}',  // Syriac
    '\u{0780}',  // Thaana
    '\u{2c80}',  // Coptic
    '\u{2c00}',  // Glagolitic
    '\u{12000}', // Cuneiform
    '\u{10330}', // Gothic
    '\u{10300}', // Old Italic
    '\u{a840}',  // Phags-pa
    '\u{1950}',  // Tai Le
    '\u{1980}',  // New Tai Lue
    '\u{1a00}',  // Buginese
    '\u{1b05}',  // Balinese
    '\u{1b83}',  // Sundanese
    '\u{aa00}',  // Cham
    '\u{1d11e}', // Musical symbols 𝄞
    '\u{1f000}', // Mahjong tiles
    '\u{1f0a1}', // Playing cards
    '\u{fe6b}',  // Small form variants ﹫
    '\u{3018}',  // CJK brackets 〘
    '\u{2e80}',  // CJK radicals
];

/// Faces to prefer when several draw the same block: the ones Windows
/// keeps for symbols and rare scripts, then Noto's and DejaVu's.
const PREFERRED: &[&str] = &[
    "segoe ui symbol",
    "segoe ui historic",
    "ebrima",
    "gadugi",
    "nirmala ui",
    "leelawadee ui",
    "mongolian baiti",
    "microsoft phagspa",
    "microsoft tai le",
    "microsoft new tai lue",
    "microsoft himalaya",
    "javanese text",
    "myanmar text",
    "sylfaen",
    "mv boli",
    "cambria math",
    "noto sans symbols 2",
    "noto sans symbols",
    "noto sans math",
    "noto sans",
    "dejavu sans",
    "freeserif",
];

/// The definitions the window has, kept to add faces to later.
fn current() -> &'static Mutex<Option<FontDefinitions>> {
    static CURRENT: OnceLock<Mutex<Option<FontDefinitions>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(None))
}

/// Installs the fonts and starts the probe; the faces it finds arrive with
/// a later frame, through `ctx.set_fonts`.
pub fn install(ctx: &egui::Context) {
    let definitions = fastframe_fonts::FontSetup::default().primary(fastframe_fonts::Primary::System).definitions();
    ctx.set_fonts(definitions.clone());
    *current().lock().unwrap_or_else(|e| e.into_inner()) = Some(definitions);
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new().name("cordfast-fonts".into()).spawn(move || {
        let started = std::time::Instant::now();
        let mut guard = current().lock().unwrap_or_else(|e| e.into_inner());
        let Some(definitions) = guard.as_mut() else { return };
        let added = add_fallbacks(definitions);
        log::info!("fonts: {} more fallback faces in {:.0} ms", added.len(), started.elapsed().as_secs_f64() * 1000.0);
        if !added.is_empty() {
            ctx.set_fonts(definitions.clone());
            ctx.request_repaint();
        }
    });
}

/// Whether some face in `definitions`' proportional family draws `c`.
fn drawn(definitions: &FontDefinitions, c: char) -> bool {
    definitions.families.get(&FontFamily::Proportional).into_iter().flatten().any(|name| {
        definitions.font_data.get(name).is_some_and(|data| {
            skrifa::FontRef::from_index(&data.font, data.index).is_ok_and(|font| {
                use skrifa::MetadataProvider as _;
                font.charmap().map(c).is_some()
            })
        })
    })
}

/// A font file, mapped for the life of the process.
struct Face {
    family: String,
    bytes: &'static [u8],
    index: u32,
}

/// Every face under the system's font folders, mapped. Collections give
/// each face.
fn installed() -> Vec<Face> {
    use skrifa::MetadataProvider as _;
    let mut faces = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<(PathBuf, u8)> = fastframe_fonts::system::font_directories().into_iter().map(|d| (d, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc")) || !seen.insert(path.clone()) {
                continue;
            }
            let Ok(file) = std::fs::File::open(&path) else { continue };
            // SAFETY: font files are mapped read-only and never written by
            // this process; another program changing one while it is mapped
            // would at worst draw a wrong glyph, as for any app's fonts.
            let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else { continue };
            let bytes: &'static [u8] = Box::leak(Box::new(map));
            let count = skrifa::raw::FileRef::new(bytes).map_or(0, |f| f.fonts().count() as u32);
            for index in 0..count.max(1) {
                let Ok(font) = skrifa::FontRef::from_index(bytes, index) else { continue };
                let family = font
                    .localized_strings(skrifa::string::StringId::FAMILY_NAME)
                    .english_or_first()
                    .map(|n| n.to_string().to_ascii_lowercase())
                    .unwrap_or_default();
                faces.push(Face { family, bytes, index });
            }
        }
    }
    faces
}

/// Adds a face for each probe nothing draws yet, preferring the platform's
/// symbol and historic faces. Returns the families added.
fn add_fallbacks(definitions: &mut FontDefinitions) -> Vec<String> {
    use skrifa::MetadataProvider as _;
    let mut missing: Vec<char> = PROBES.iter().copied().filter(|c| !drawn(definitions, *c)).collect();
    if missing.is_empty() {
        return Vec::new();
    }
    let mut faces = installed();
    // Preferred faces first, then the rest in folder order; regular cuts
    // before bold or italic ones.
    faces.sort_by_key(|f| {
        let rank = PREFERRED.iter().position(|p| f.family == *p).unwrap_or(PREFERRED.len());
        let styled = ["bold", "italic", "light", "black"].iter().any(|s| f.family.contains(s));
        (rank, styled)
    });
    let mut added = Vec::new();
    for face in &faces {
        if missing.is_empty() {
            break;
        }
        let Ok(font) = skrifa::FontRef::from_index(face.bytes, face.index) else { continue };
        let charmap = font.charmap();
        let covers: Vec<char> = missing.iter().copied().filter(|c| charmap.map(*c).is_some()).collect();
        if covers.is_empty() {
            continue;
        }
        missing.retain(|c| !covers.contains(c));
        let name = format!("cordfast-{}-{}", face.family.replace(' ', "-"), face.index);
        if definitions.font_data.contains_key(&name) {
            continue;
        }
        let mut data = FontData::from_static(face.bytes);
        data.index = face.index;
        definitions.font_data.insert(name.clone(), Arc::new(data));
        for family in definitions.families.values_mut() {
            family.push(name.clone());
        }
        added.push(face.family.clone());
    }
    added
}

#[cfg(test)]
mod tests {
    /// From egui's own fonts alone, the probe finds installed faces for the
    /// blocks they lack: `cargo test fallback_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn fallback_probe() {
        let mut definitions = egui::FontDefinitions::default();
        let before: Vec<char> = super::PROBES.iter().copied().filter(|c| !super::drawn(&definitions, *c)).collect();
        let added = super::add_fallbacks(&mut definitions);
        let after: Vec<char> = super::PROBES.iter().copied().filter(|c| !super::drawn(&definitions, *c)).collect();
        println!("faltavam {}, faltam {}; faces: {added:?}", before.len(), after.len());
        assert!(after.len() <= before.len());
    }
}
