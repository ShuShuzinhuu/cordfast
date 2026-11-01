//! Discord's sounds: someone joining or leaving a call, our own
//! disconnect, mute and deafen, push-to-talk's key, and the message sound of a mention or DM. Fetched from discord.com the first time, as the web
//! client loads them (names and hashes from its bundle), kept in the cache
//! folder, decoded once and played on a short-lived output stream, so they
//! sound with or without a call running.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    Join,
    Leave,
    Disconnect,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    /// "message1", the sound of a message that notifies.
    Message,
    /// The push-to-talk key pressed and let go.
    PttStart,
    PttStop,
}

/// Every sound, in Discord's list (Notifications and Voice & Video →
/// Sounds): its name there and its asset's name in the bundle.
pub const ALL: [(Sound, &str, &str); 10] = [
    (Sound::Message, n!("Message"), "message1"),
    (Sound::Deafen, n!("Deafen"), "deafen"),
    (Sound::Undeafen, n!("Undeafen"), "undeafen"),
    (Sound::Mute, n!("Mute"), "mute"),
    (Sound::Unmute, n!("Unmute"), "unmute"),
    (Sound::Disconnect, n!("Voice Disconnected"), "disconnect"),
    (Sound::PttStart, n!("PTT Activate"), "ptt_start"),
    (Sound::PttStop, n!("PTT Deactivate"), "ptt_stop"),
    (Sound::Join, n!("User Join"), "user_join"),
    (Sound::Leave, n!("User Leave"), "user_leave"),
];

impl Sound {
    /// Its name in Discord's sound list and in the settings file.
    pub fn name(self) -> &'static str {
        ALL.iter().find(|(s, ..)| *s == self).map_or("", |(_, _, name)| name)
    }

    /// The asset the web client plays for it.
    fn asset(self) -> &'static str {
        match self {
            Self::Join => "b135ff6c8e091b43.mp3",
            Self::Leave => "7b9a183742515fc2.mp3",
            Self::Disconnect => "dd0911025fe00b2e.mp3",
            Self::Mute => "2d3b4ba32c34c862.mp3",
            Self::Unmute => "e74c4a06134a20e4.mp3",
            Self::Deafen => "529ff198eac567af.mp3",
            Self::Undeafen => "b150f03c89944403.mp3",
            Self::Message => "3ed22d14f3c30bc4.mp3",
            Self::PttStart => "f5c8ec31889c5317.mp3",
            Self::PttStop => "34b5969be3a063ba.mp3",
        }
    }
}

/// Decoded sounds: interleaved stereo at their own rate.
type Decoded = Arc<(Vec<f32>, u32)>;

fn decoded() -> &'static Mutex<HashMap<Sound, Decoded>> {
    static CACHE: OnceLock<Mutex<HashMap<Sound, Decoded>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Plays a sound on `device` (or the default output) at `volume`.
pub fn play(sound: Sound, device: Option<String>, volume: f32) {
    let _ = std::thread::Builder::new().name("cordfast-sound".into()).spawn(move || {
        if let Err(error) = run(sound, device, volume) {
            log::debug!("sound {sound:?}: {error:#}");
        }
    });
}

fn run(sound: Sound, device: Option<String>, volume: f32) -> anyhow::Result<()> {
    let cached = decoded().lock().unwrap_or_else(|e| e.into_inner()).get(&sound).cloned();
    let pcm = match cached {
        Some(pcm) => pcm,
        None => {
            let pcm = Arc::new(decode(&fetch(sound)?)?);
            decoded().lock().unwrap_or_else(|e| e.into_inner()).insert(sound, pcm.clone());
            pcm
        }
    };
    output(&pcm.0, pcm.1, device, volume)
}

fn fetch(sound: Sound) -> anyhow::Result<Vec<u8>> {
    let dir = crate::settings::config_dir().join("sounds");
    let path = dir.join(sound.asset());
    if let Ok(bytes) = std::fs::read(&path) {
        return Ok(bytes);
    }
    let url = format!("https://discord.com/assets/{}", sound.asset());
    let bytes = reqwest::blocking::get(url)?.error_for_status()?.bytes()?.to_vec();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&path, &bytes);
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> anyhow::Result<(Vec<f32>, u32)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let source = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe().format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| anyhow::anyhow!("no track"))?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut out = Vec::new();
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != id {
            continue;
        }
        let Ok(audio) = decoder.decode(&packet) else { continue };
        let spec = *audio.spec();
        rate = spec.rate;
        let channels = spec.channels.count();
        let mut buffer = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
        buffer.copy_interleaved_ref(audio);
        for frame in buffer.samples().chunks(channels) {
            let left = frame[0];
            let right = *frame.get(1).unwrap_or(&left);
            out.push(left);
            out.push(right);
        }
    }
    Ok((out, rate))
}

/// Plays interleaved stereo through a stream of its own, resampling
/// linearly to the device's rate.
fn output(pcm: &[f32], rate: u32, device: Option<String>, volume: f32) -> anyhow::Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = device
        .and_then(|name| host.output_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(&name)))
        .or_else(|| host.default_output_device())
        .ok_or_else(|| anyhow::anyhow!("no output"))?;
    let config = device.default_output_config()?;
    let channels = usize::from(config.channels());
    let device_rate = config.sample_rate().0;
    let step = f64::from(rate) / f64::from(device_rate);
    let frames = pcm.len() / 2;
    let samples: Arc<Vec<f32>> = Arc::new(pcm.to_vec());
    let position = Arc::new(Mutex::new(0.0f64));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let fill = {
        let (samples, position) = (samples.clone(), position.clone());
        move |data: &mut [f32]| {
            let mut at = position.lock().unwrap_or_else(|e| e.into_inner());
            for frame in data.chunks_mut(channels) {
                let i = *at as usize;
                let t = (*at - i as f64) as f32;
                for (c, sample) in frame.iter_mut().enumerate() {
                    let c = c.min(1);
                    *sample = if i + 1 < frames {
                        let a = samples[i * 2 + c];
                        let b = samples[(i + 1) * 2 + c];
                        (a + (b - a) * t) * volume
                    } else {
                        0.0
                    };
                }
                *at += step;
            }
            if *at as usize >= frames {
                let _ = done_tx.send(());
            }
        }
    };
    let error = |e| log::debug!("sound: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_output_stream(&config.into(), move |data: &mut [f32], _: &_| fill(data), error, None)?
        }
        cpal::SampleFormat::I16 => {
            device.build_output_stream(
                &config.into(),
                move |data: &mut [i16], _: &_| {
                    let mut f = vec![0.0f32; data.len()];
                    fill(&mut f);
                    for (o, s) in data.iter_mut().zip(f) {
                        *o = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                },
                error,
                None,
            )?
        }
        other => anyhow::bail!("format {other:?}"),
    };
    stream.play()?;
    let length = std::time::Duration::from_secs_f64(frames as f64 / f64::from(rate));
    let _ = done_rx.recv_timeout(length + std::time::Duration::from_millis(500));
    // Lets the device drain its last buffer.
    std::thread::sleep(std::time::Duration::from_millis(80));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fetches and decodes every sound: `cargo test sounds -- --ignored`.
    #[test]
    #[ignore]
    fn sounds_decode() {
        for sound in [Sound::Join, Sound::Leave, Sound::Disconnect, Sound::Mute, Sound::Unmute, Sound::Deafen, Sound::Undeafen, Sound::Message, Sound::PttStart, Sound::PttStop] {
            let (pcm, rate) = decode(&fetch(sound).unwrap()).unwrap();
            println!("{sound:?}: {:.2} s a {rate} Hz", pcm.len() as f64 / 2.0 / f64::from(rate));
            assert!(pcm.len() > 1000);
        }
    }
}
