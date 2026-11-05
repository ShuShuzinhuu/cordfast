//! Playing a video inside the app, one at a time. Windows' Media Foundation
//! reads and decodes it (in hardware where the machine can), streaming from
//! the URL; its thread keeps only the newest frame, which the window turns
//! into a texture when it draws, so a video off screen costs no uploads.
//! Elsewhere there is no player yet and videos open in the system's.

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(windows)]
use std::sync::{Arc, Mutex};

/// The newest decoded picture, RGBA, and whether the window has drawn it.
#[cfg(windows)]
#[derive(Default)]
struct Latest {
    size: [usize; 2],
    pixels: Vec<u8>,
    fresh: bool,
}

#[cfg(windows)]
struct Shared {
    latest: Mutex<Latest>,
    stop: AtomicBool,
    paused: AtomicBool,
    finished: AtomicBool,
    /// Where it is and how long it is, in milliseconds.
    position: AtomicU64,
    duration: AtomicU64,
    error: Mutex<Option<String>>,
}

#[cfg(windows)]
pub struct Player {
    pub url: String,
    shared: Arc<Shared>,
    texture: Option<egui::TextureHandle>,
}

#[cfg(windows)]
impl Player {
    pub fn start(url: String, wake: crate::backend::Wake, device: Option<String>, volume: f32) -> Self {
        let shared = Arc::new(Shared {
            latest: Mutex::new(Latest::default()),
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            position: AtomicU64::new(0),
            duration: AtomicU64::new(0),
            error: Mutex::new(None),
        });
        let (thread_shared, thread_url) = (shared.clone(), url.clone());
        let _ = std::thread::Builder::new().name("cordfast-video".into()).spawn(move || {
            if let Err(error) = run(&thread_url, &thread_shared, &wake, device, volume) {
                log::warn!("video: {error:#}");
                *thread_shared.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{error:#}"));
            }
            thread_shared.finished.store(true, Ordering::Relaxed);
            wake();
        });
        Self { url, shared, texture: None }
    }

    pub fn toggle_pause(&self) {
        self.shared.paused.fetch_xor(true, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Position and length, in milliseconds.
    pub fn times(&self) -> (u64, u64) {
        (self.shared.position.load(Ordering::Relaxed), self.shared.duration.load(Ordering::Relaxed))
    }

    /// The picture to draw, uploaded only when a new one came.
    pub fn frame(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        {
            let mut latest = self.shared.latest.lock().unwrap_or_else(|e| e.into_inner());
            if latest.fresh && latest.size[0] > 0 {
                latest.fresh = false;
                let image = egui::ColorImage::from_rgba_premultiplied(latest.size, &latest.pixels);
                match &mut self.texture {
                    Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                    None => self.texture = Some(ctx.load_texture("video", image, egui::TextureOptions::LINEAR)),
                }
            }
        }
        self.texture.as_ref()
    }
}

#[cfg(windows)]
impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// The sound: interleaved stereo waiting for the device.
#[cfg(windows)]
struct Sound {
    queue: Arc<Mutex<std::collections::VecDeque<f32>>>,
    rate: u32,
    _stream: cpal::Stream,
}

#[cfg(windows)]
fn open_sound(device: Option<String>, volume: f32, paused: Arc<Shared>) -> anyhow::Result<Sound> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = device.and_then(|name| host.output_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(&name))).or_else(|| host.default_output_device()).ok_or_else(|| anyhow::anyhow!("no output"))?;
    let config = device.default_output_config()?;
    let channels = usize::from(config.channels());
    let rate = config.sample_rate().0;
    let queue: Arc<Mutex<std::collections::VecDeque<f32>>> = Default::default();
    let source = queue.clone();
    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &_| {
            let mut queue = source.lock().unwrap_or_else(|e| e.into_inner());
            let silent = paused.paused.load(Ordering::Relaxed);
            for frame in data.chunks_mut(channels) {
                for (c, sample) in frame.iter_mut().enumerate() {
                    *sample = if silent || c > 1 { 0.0 } else { queue.pop_front().unwrap_or(0.0) * volume };
                }
            }
        },
        |e| log::debug!("video sound: {e}"),
        None,
    )?;
    stream.play()?;
    Ok(Sound { queue, rate, _stream: stream })
}

#[cfg(windows)]
fn run(url: &str, shared: &Arc<Shared>, wake: &crate::backend::Wake, device: Option<String>, volume: f32) -> anyhow::Result<()> {
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::core::HSTRING;

    // Its sample times count in units of 100 ns.
    const MS: i64 = 10_000;
    let video_stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let audio_stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 2)?;
        let attributes = attributes.ok_or_else(|| anyhow::anyhow!("no attributes"))?;
        // Converts whatever the decoder gives to the RGB asked for below.
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(url), &attributes)?;
        let wanted = MFCreateMediaType()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(video_stream, None, &wanted)?;
        reader.SetStreamSelection(video_stream, true)?;
        let current = reader.GetCurrentMediaType(video_stream)?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (width, height) = ((size >> 32) as usize, (size & 0xffff_ffff) as usize);
        // Negative, or unset: the picture's rows come bottom first.
        let stride = current.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(width as i32 * 4, |stride| stride as i32);
        if let Ok(length) = reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION) {
            let ticks = u64::try_from(&length).unwrap_or(0);
            shared.duration.store(ticks / MS as u64, Ordering::Relaxed);
        }
        // Without an audio track, or a device, the picture still plays.
        let sound = open_sound(device, volume, shared.clone()).ok();
        let mut has_audio = false;
        if let Some(sound) = &sound {
            let wanted = MFCreateMediaType()?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
            wanted.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            wanted.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sound.rate)?;
            has_audio = reader.SetCurrentMediaType(audio_stream, None, &wanted).is_ok() && reader.SetStreamSelection(audio_stream, true).is_ok();
        }
        let started = std::time::Instant::now();
        let mut paused_for = std::time::Duration::ZERO;
        let mut spare: Vec<u8> = Vec::new();
        loop {
            while shared.paused.load(Ordering::Relaxed) && !shared.stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
                paused_for += std::time::Duration::from_millis(50);
            }
            if shared.stop.load(Ordering::Relaxed) {
                break;
            }
            let (mut which, mut flags, mut time) = (0u32, 0u32, 0i64);
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(MF_SOURCE_READER_ANY_STREAM.0 as u32, 0, Some(&mut which), Some(&mut flags), Some(&mut time), Some(&mut sample))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                if which == video_stream || !has_audio {
                    break;
                }
                continue;
            }
            let Some(sample) = sample else { continue };
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut length = 0u32;
            buffer.Lock(&mut data, None, Some(&mut length))?;
            let bytes = std::slice::from_raw_parts(data, length as usize);
            if has_audio && which == audio_stream {
                if let Some(sound) = &sound {
                    let samples = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                    sound.queue.lock().unwrap_or_else(|e| e.into_inner()).extend(samples);
                }
                buffer.Unlock()?;
                continue;
            }
            // Convert into the spare buffer, swapped with the shared one: no new allocation a frame.
            spare.resize(width * height * 4, 0);
            let row = stride.unsigned_abs() as usize;
            for y in 0..height {
                let from_row = if stride < 0 { height - 1 - y } else { y };
                let Some(from) = bytes.get(from_row * row..from_row * row + width * 4) else { continue };
                let to = &mut spare[y * width * 4..(y + 1) * width * 4];
                for (out, pixel) in to.chunks_exact_mut(4).zip(from.chunks_exact(4)) {
                    out[0] = pixel[2];
                    out[1] = pixel[1];
                    out[2] = pixel[0];
                    out[3] = 255;
                }
            }
            buffer.Unlock()?;
            // Holds the frame until its time, which also paces the reading.
            let due = std::time::Duration::from_millis((time / MS).max(0) as u64);
            let elapsed = started.elapsed().saturating_sub(paused_for);
            if let Some(wait) = due.checked_sub(elapsed) {
                std::thread::sleep(wait.min(std::time::Duration::from_secs(1)));
            }
            {
                let mut latest = shared.latest.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::swap(&mut latest.pixels, &mut spare);
                latest.size = [width, height];
                latest.fresh = true;
            }
            shared.position.store((time / MS).max(0) as u64, Ordering::Relaxed);
            wake();
        }
        // Lets what is left of the sound finish.
        if let Some(sound) = &sound {
            while !shared.stop.load(Ordering::Relaxed) && !sound.queue.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Streams a public clip and waits for pictures:
    /// `cargo test video_plays -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn video_plays() {
        let url = std::env::var("CORDFAST_VIDEO_TEST").expect("a video's URL");
        let player = Player::start(url, std::sync::Arc::new(|| {}), None, 0.0);
        std::thread::sleep(std::time::Duration::from_secs(4));
        println!("{:?} at {:?}, error {:?}", player.shared.latest.lock().unwrap().size, player.times(), player.error());
        assert!(player.error().is_none());
        assert!(player.times().0 > 500);
    }
}
