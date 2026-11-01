//! Voice: guild voice channels and DM calls.
//!
//! The protocol (voice gateway, UDP, Opus, transport encryption and DAVE
//! end-to-end encryption) is songbird's driver. This file joins it to the
//! main gateway's voice state, to the microphone and speakers (cpal, on a
//! thread of their own since its streams cannot move between threads), and
//! picks the voice region with the lowest measured latency.
//!
//! The microphone feeds one live track. A voice-activity gate pauses that
//! track while nobody speaks, so nothing is sent then, as the official
//! client does; received voice is mixed per user with their own volume.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use songbird::driver::DecodeMode;
use songbird::events::{CoreEvent, Event, EventContext, EventHandler};
use songbird::id::{ChannelId, GuildId, UserId};
use songbird::input::RawAdapter;
use songbird::tracks::TrackHandle;
use songbird::{Config, ConnectionInfo, Driver};
use tokio::sync::mpsc;

use crate::discord::model::{Id, sf};

const RATE: u32 = 48_000;
/// 10 ms at 48 kHz: the frame RNNoise, DeepFilterNet and WebRTC all take.
const FRAME: usize = RATE as usize / 100;
/// Received audio kept ahead of the speakers; more is dropped as latency.
const MAX_OUTPUT: usize = RATE as usize * 2 / 5;
/// How long the gate stays open after the voice drops below it.
const HANGOVER: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub input_volume: f32,
    pub output_volume: f32,
    pub sensitivity_db: f32,
    pub automatic: bool,
    /// RNNoise: removes background noise and gates on its voice detection.
    pub noise_suppression: bool,
    /// WebRTC's echo canceller on the microphone, fed what the speakers play.
    pub echo_cancellation: bool,
    /// WebRTC's adaptive gain on the microphone, after the noise suppression.
    pub gain_control: bool,
    /// With the automatic sensitivity, voice detection opens the gate
    /// rather than the level alone (Discord's "Advanced Voice Activity").
    pub advanced_activity: bool,
    /// Push-to-talk: the virtual key that opens the microphone.
    pub push_to_talk: Option<u32>,
    /// Push-to-talk plays Discord's ptt_start and ptt_stop.
    pub ptt_sounds: bool,
    pub user_volumes: HashMap<Id, f32>,
}

pub enum VoiceEvent {
    /// We are in (`Some`) or out of (`None`) a voice channel.
    Channel(Option<(Option<Id>, Id)>),
    Connected,
    /// The connection dropped; songbird tries again by itself.
    Disconnected,
    /// The call went on without us: another device of the account joined
    /// voice (`elsewhere`), or Discord ended this session (the channel gone,
    /// a kick). We left it without a word to the gateway, as Discord's
    /// client does, so the other device keeps it.
    Ended { elsewhere: bool, guild: Option<Id>, channel: Id },
    /// Who is speaking now, us included.
    Speaking(HashSet<Id>),
    /// Our microphone's level in dBFS, for the sensitivity meter.
    MicLevel(f32),
    /// Regions ranked by latency: (region, milliseconds).
    Regions(Vec<(String, u32)>),
}

// ---- region ranking ---------------------------------------------------------

/// Measures each nearby region (Discord's own list, geo-ranked) by the
/// median TCP handshake time to its media servers on port 443, the way the
/// desktop client ranks them before every call. Fastest first.
pub async fn rank_regions(http: &reqwest::Client) -> Vec<(String, u32)> {
    let Ok(response) = http.get("https://latency.discord.media/rtc?v=1").send().await else {
        return Vec::new();
    };
    let Ok(list) = response.json::<Value>().await else {
        return Vec::new();
    };
    let mut tasks = Vec::new();
    for region in list.as_array().into_iter().flatten() {
        let Some(name) = region.get("region").and_then(Value::as_str) else { continue };
        let ips: Vec<String> = region
            .get("ips")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|i| i.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let name = name.to_owned();
        tasks.push(tokio::spawn(async move {
            let mut times = Vec::new();
            for ip in ips.iter().take(3) {
                let start = Instant::now();
                let connect = tokio::net::TcpStream::connect((ip.as_str(), 443));
                if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(1200), connect).await {
                    times.push(start.elapsed().as_millis() as u32);
                }
            }
            times.sort_unstable();
            times.get(times.len() / 2).map(|ms| (name, *ms))
        }));
    }
    let mut ranked = Vec::new();
    for task in tasks {
        if let Ok(Some(result)) = task.await {
            ranked.push(result);
        }
    }
    ranked.sort_by_key(|(_, ms)| *ms);
    ranked
}

// ---- microphone source ------------------------------------------------------

/// Samples from the microphone waiting to be sent: stereo f32 at 48 kHz.
#[derive(Default)]
struct MicBuffer {
    samples: Mutex<VecDeque<f32>>,
    /// Enough is buffered to send without gaps; cleared when the gate
    /// opens and whenever the buffer runs dry.
    primed: std::sync::atomic::AtomicBool,
    /// Diagnostics: samples sent as silence because the microphone was
    /// late, and samples the limiter had to bring down.
    late: std::sync::atomic::AtomicU64,
    limited: std::sync::atomic::AtomicU64,
}

/// How much the source keeps ahead before sending: 40 ms of stereo. The
/// microphone's thread hands blocks over unevenly (the denoiser runs a
/// frame behind); this margin keeps every 20 ms packet whole.
const MIC_PRIME: usize = RATE as usize * 2 / 25;

/// The live track's source: reads the microphone buffer as raw f32. It is
/// read on songbird's mixing thread, so it never waits: a wait there
/// delays every packet of the call, heard as a robotic voice. It sends
/// silence until a margin is buffered, then the voice without gaps; if the
/// buffer runs dry it builds the margin again rather than cutting words.
struct MicSource(Arc<MicBuffer>);

impl Read for MicSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::sync::atomic::Ordering::Relaxed;
        let wanted = (buf.len() / 4).min(1920);
        if wanted == 0 {
            return Ok(0);
        }
        let mut samples = self.0.samples.lock().unwrap_or_else(|e| e.into_inner());
        let primed = self.0.primed.load(Relaxed) || samples.len() >= MIC_PRIME;
        if primed && samples.len() < wanted {
            // Ran dry: count it, and buffer the margin again.
            self.0.late.fetch_add((wanted - samples.len()) as u64, Relaxed);
            self.0.primed.store(false, Relaxed);
        } else {
            self.0.primed.store(primed, Relaxed);
        }
        let take = primed && samples.len() >= wanted;
        for i in 0..wanted {
            let sample = if take { samples.pop_front().unwrap_or(0.0) } else { 0.0 };
            buf[i * 4..i * 4 + 4].copy_from_slice(&sample.to_le_bytes());
        }
        Ok(wanted * 4)
    }
}

impl Seek for MicSource {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

impl symphonia_core::io::MediaSource for MicSource {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

// ---- audio devices ----------------------------------------------------------

/// What the audio thread and the voice task share.
struct Shared {
    mic: Arc<MicBuffer>,
    output: Mutex<VecDeque<f32>>,
    settings: Mutex<AudioSettings>,
    muted: std::sync::atomic::AtomicBool,
    deaf: std::sync::atomic::AtomicBool,
    /// Mic test: the microphone, as processed, straight to the speakers;
    /// the call hears nothing from us and we nothing from it meanwhile.
    loopback: std::sync::atomic::AtomicBool,
    /// What the speakers play from the call, mono at 48 kHz: the echo
    /// canceller's reference, drained by the microphone's thread.
    render: Mutex<VecDeque<f32>>,
    /// Diagnostics: times the speakers ran dry while playing, voice ticks
    /// received, and ticks whose packet was lost.
    underruns: std::sync::atomic::AtomicU64,
    ticks: std::sync::atomic::AtomicU64,
    lost: std::sync::atomic::AtomicU64,
    /// Diagnostics: a recording of what arrives and what the speakers play.
    recording: Mutex<Option<Recording>>,
}

/// "Record 20 s of what you hear": the decoded mix as it arrives and
/// what the device is fed, saved side by side to find which stage harms
/// the sound.
#[derive(Default)]
struct Recording {
    received: Vec<f32>,
    played: Vec<f32>,
    played_rate: u32,
    played_channels: usize,
    /// Decoded lengths per tick, and how many samples were past full scale.
    lengths: std::collections::BTreeMap<usize, u32>,
    clipped: u64,
    /// The Opus frames as received (after DAVE, trimmed) per sender, with
    /// their length in samples, for a reference decode by ffmpeg/libopus;
    /// and how many frames carried the DAVE trailer.
    opus: HashMap<u32, Vec<(Vec<u8>, u64)>>,
    trailers: u64,
    /// The packets whole, as songbird hands them over (after transport and
    /// DAVE decryption), with its payload offset and end padding: what
    /// `opus_frame` cuts can be checked against every other cut offline.
    raw: Vec<(u32, u16, u16, Vec<u8>)>,
}

const RECORD_SECONDS: usize = 20;

impl Recording {
    fn full(&self) -> bool {
        self.received.len() >= RATE as usize * 2 * RECORD_SECONDS
    }

    fn save(self) {
        let dir = crate::settings::config_dir().join("logs");
        write_wav(&dir.join("voice-diagnostic-received.wav"), &self.received, RATE, 2);
        write_wav(&dir.join("voice-diagnostic-played.wav"), &self.played, self.played_rate, self.played_channels as u16);
        // The sender with the most audio (the bot, in a music test).
        let frames = self.opus.values().max_by_key(|f| f.len()).map_or(0, Vec::len);
        let sender = self.opus.iter().max_by_key(|(_, f)| f.len()).map(|(ssrc, _)| *ssrc);
        let summary = sender.and_then(|s| self.opus.get(&s)).map(|p| opus_summary(p)).unwrap_or_default();
        if let Some(packets) = sender.and_then(|s| self.opus.get(&s)) {
            write_ogg_opus(&dir.join("voice-diagnostic-opus.ogg"), packets);
        }
        // Each packet: ssrc, the payload's start and end inside the RTP
        // payload (u32, u16, u16 little-endian, as songbird's voice tick
        // hands them over), its length (u16) and its bytes.
        let mut raw = b"CFPK1".to_vec();
        for (ssrc, offset, end, bytes) in &self.raw {
            raw.extend(ssrc.to_le_bytes());
            raw.extend(offset.to_le_bytes());
            raw.extend(end.to_le_bytes());
            raw.extend((bytes.len() as u16).to_le_bytes());
            raw.extend(bytes);
        }
        if let Err(error) = std::fs::write(dir.join("voice-diagnostic-packets.bin"), raw) {
            log::warn!("voice: diagnostic: {error}");
        }
        log::info!(
            "voice: diagnostic saved; decoded sizes {:?}, clipped samples {}, output {} Hz {} channels, {frames} Opus packets recorded, {} with the DAVE trailer (removed); {summary}",
            self.lengths,
            self.clipped,
            self.played_rate,
            self.played_channels,
            self.trailers
        );
    }
}

/// What a sender's Opus stream is: its bitrate and its modes, from each
/// frame's TOC byte (RFC 6716, 3.1), e.g. "128 kb/s; CELT fullband 20 ms
/// stereo: 1000". A music bot at a low bitrate sounds hollow on any client.
fn opus_summary(packets: &[(Vec<u8>, u64)]) -> String {
    let bytes: usize = packets.iter().map(|(f, _)| f.len()).sum();
    let samples: u64 = packets.iter().map(|(_, s)| s).sum();
    let seconds = samples as f64 / f64::from(RATE);
    let mut modes: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for (frame, _) in packets {
        let Some(&toc) = frame.first() else { continue };
        let config = toc >> 3;
        let (mode, bandwidth) = match config {
            0..=3 => ("SILK", "narrowband"),
            4..=7 => ("SILK", "mediumband"),
            8..=11 => ("SILK", "wideband"),
            12..=13 => ("hybrid", "super-wideband"),
            14..=15 => ("hybrid", "fullband"),
            16..=19 => ("CELT", "narrowband"),
            20..=23 => ("CELT", "wideband"),
            24..=27 => ("CELT", "super-wideband"),
            _ => ("CELT", "fullband"),
        };
        let ms = match config {
            0..=11 => [10.0, 20.0, 40.0, 60.0][usize::from(config % 4)],
            12..=15 => [10.0, 20.0][usize::from(config % 2)],
            _ => [2.5, 5.0, 10.0, 20.0][usize::from(config % 4)],
        };
        let channels = if toc & 0x04 != 0 { "stereo" } else { "mono" };
        *modes.entry(format!("{mode} {bandwidth} {ms} ms {channels}")).or_default() += 1;
    }
    let rate = if seconds > 0.0 { bytes as f64 * 8.0 / seconds / 1000.0 } else { 0.0 };
    let list: Vec<String> = modes.iter().map(|(m, n)| format!("{m}: {n}")).collect();
    format!("{rate:.0} kb/s; {}", list.join(", "))
}

/// The Opus frame of a received packet, without what surrounds it: the RTP
/// header, the transport's tag and nonce, RTP padding, the header extension
/// and the DAVE trailer. songbird 0.6.1 decodes from the transport's offsets
/// alone, so after DAVE decryption its frame kept the ciphertext's trailer
/// (tag, nonce, size, 0xFAFA). libopus takes those bytes for audio: CELT,
/// which sizes its bit allocation by the frame and reads raw bits from its
/// end, played music distorted and with a wrong stereo image. Returns the
/// frame and whether a trailer came off.
///
/// `end` is where the payload ends inside `rtp.payload()`, as a voice
/// tick's `RtpData` carries it: songbird 0.6.1 fills `payload_end_pad`
/// with `payload.len() - suffix` (`SsrcState::get_voice_tick`), an index,
/// although its documentation calls it the number of bytes to drop. Read as
/// a count, the frame came out as the payload's first ~20 bytes: some 8
/// bytes of Opus, which libopus decodes without complaint into a hiss that
/// follows the music's loudness, with no stereo left and a constant 68-
/// sample comb (the range decoder reading zeros past the end), for music
/// and voices alike.
fn opus_frame(packet: &[u8], offset: usize, end: usize) -> Option<(&[u8], bool)> {
    let first = *packet.first()?;
    let header = 12 + 4 * usize::from(first & 0x0f);
    let payload = packet.get(header..)?;
    let mut body = payload.get(offset..end)?;
    if first & 0x20 != 0 {
        let padding = usize::from(*body.last()?);
        body = body.get(..body.len().checked_sub(padding)?)?;
    }
    if first & 0x10 != 0 {
        let words = usize::from(u16::from_be_bytes([*body.get(2)?, *body.get(3)?]));
        body = body.get(4 + 4 * words..)?;
    }
    let frame = strip_dave(body);
    (!frame.is_empty()).then_some((frame, frame.len() != body.len()))
}

/// A DAVE frame's plaintext without the supplemental data at its end
/// (daveprotocol.com): the 8-byte truncated tag, the ULEB128 nonce (and
/// unencrypted ranges, none for audio), one byte with the size of all of
/// it, and the 0xFAFA marker. Anything else comes back whole.
fn strip_dave(frame: &[u8]) -> &[u8] {
    let n = frame.len();
    if n < 12 || frame[n - 2..] != [0xfa, 0xfa] {
        return frame;
    }
    let size = usize::from(frame[n - 3]);
    if size < 12 || size >= n {
        return frame;
    }
    // The nonce must end its ULEB128 right before the size byte.
    let nonce = &frame[n - size + 8..n - 3];
    if nonce.last().is_none_or(|b| b & 0x80 != 0) {
        return frame;
    }
    &frame[..n - size]
}

/// The CRC-32 of Ogg pages: polynomial 0x04c11db7, not reflected.
fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for byte in data {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04c1_1db7 } else { crc << 1 };
        }
    }
    crc
}

/// Opus frames as an Ogg Opus file (RFC 7845), one frame per page, so
/// `ffmpeg -i voice-diagnostic-opus.ogg reference.wav` decodes exactly what
/// arrived, independently of CordFast.
fn write_ogg_opus(path: &std::path::Path, packets: &[(Vec<u8>, u64)]) {
    let mut out = Vec::new();
    let page = |out: &mut Vec<u8>, flags: u8, granule: u64, sequence: u32, data: &[u8]| {
        let start = out.len();
        out.extend(b"OggS\0");
        out.push(flags);
        out.extend(granule.to_le_bytes());
        out.extend(0x0c0f_da57u32.to_le_bytes());
        out.extend(sequence.to_le_bytes());
        out.extend([0; 4]);
        let mut lacing = vec![255u8; data.len() / 255];
        lacing.push((data.len() % 255) as u8);
        out.push(lacing.len() as u8);
        out.extend(lacing);
        out.extend(data);
        let crc = ogg_crc(&out[start..]);
        out[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
    };
    let mut head = b"OpusHead".to_vec();
    head.extend([1, 2]);
    head.extend(0u16.to_le_bytes());
    head.extend(RATE.to_le_bytes());
    head.extend([0, 0, 0]);
    page(&mut out, 0x02, 0, 0, &head);
    let mut tags = b"OpusTags".to_vec();
    tags.extend(8u32.to_le_bytes());
    tags.extend(b"CordFast");
    tags.extend(0u32.to_le_bytes());
    page(&mut out, 0, 0, 1, &tags);
    let mut granule = 0;
    for (i, (frame, samples)) in packets.iter().enumerate() {
        granule += samples;
        let last = i + 1 == packets.len();
        page(&mut out, if last { 0x04 } else { 0 }, granule, i as u32 + 2, frame);
    }
    if let Err(error) = std::fs::write(path, out) {
        log::warn!("voice: diagnostic: {error}");
    }
}

/// A float WAV file.
fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32, channels: u16) {
    let data = samples.len() as u32 * 4;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(3u16.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * u32::from(channels) * 4).to_le_bytes());
    out.extend((channels * 4).to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for s in samples {
        out.extend(s.to_le_bytes());
    }
    if let Err(error) = std::fs::write(path, out) {
        log::warn!("voice: diagnostic: {error}");
    }
}

enum Gate {
    Open,
    Closed,
    Level(f32),
}

/// Input and output device names, for the settings page.
pub fn devices() -> (Vec<String>, Vec<String>) {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let names = |list: Result<cpal::InputDevices<cpal::Devices>, _>| -> Vec<String> {
        list.map(|l| l.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    };
    let inputs = names(host.input_devices());
    let outputs = host
        .output_devices()
        .map(|l| l.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    (inputs, outputs)
}

/// Runs the microphone and speaker streams until `stop` is dropped.
fn audio_thread(shared: Arc<Shared>, gate: mpsc::UnboundedSender<Gate>, stop: std::sync::mpsc::Receiver<()>) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let settings = shared.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let pick = |want: &Option<String>, input: bool| -> Option<cpal::Device> {
        if let Some(name) = want {
            let found = if input {
                host.input_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(name))
            } else {
                host.output_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(name))
            };
            if found.is_some() {
                return found;
            }
        }
        if input { host.default_input_device() } else { host.default_output_device() }
    };

    // Microphone.
    let input = pick(&settings.input_device, true).and_then(|device| {
        let config = device.default_input_config().ok()?;
        let channels = usize::from(config.channels());
        let rate = config.sample_rate().0;
        log::info!(
            "voice: microphone {:?}, {} Hz, {} channels, {:?}",
            device.name().unwrap_or_default(),
            rate,
            channels,
            config.sample_format()
        );
        let shared = shared.clone();
        let gate = gate.clone();
        let mut state = GateState::default();
        let mut resampler = Resampler::new(rate);
        let suppress_at_start = settings.noise_suppression;
        // The device's callback only hands the samples over; this thread
        // cleans and gates them, so the model never stalls the audio
        // system's real-time thread. It ends with the stream.
        let (raw, samples) = std::sync::mpsc::sync_channel::<Vec<f32>>(64);
        std::thread::Builder::new()
            .name("cordfast-mic".into())
            .spawn(move || {
        let mut denoiser = if suppress_at_start { Denoiser::with_model() } else { Denoiser::default() };
        let mut echo: Option<Processor> = None;
        let mut gain: Option<Processor> = None;
        let mut ptt_held = false;
        let mut process = move |data: &[f32]| {
            let settings = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
            let (volume, threshold, automatic, suppress, ptt) = (
                settings.input_volume,
                settings.sensitivity_db,
                settings.automatic,
                settings.noise_suppression,
                settings.push_to_talk,
            );
            let (cancel_echo, steady, neural) = (settings.echo_cancellation, settings.gain_control, settings.automatic && settings.advanced_activity);
            let (sound_device, sound_volume, ptt_sounds) = (settings.output_device.clone(), (settings.output_volume * 0.6).min(1.0), settings.ptt_sounds);
            drop(settings);
            let mono: Vec<f32> = data
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32 * volume)
                .collect();
            let mut mono = resampler.run(&mono);
            // WebRTC's order: the echo out first, then the noise, then the
            // level.
            if cancel_echo {
                mono = echo.get_or_insert_with(Processor::echo).run(&mono, Some(&shared.render));
            } else if echo.take().is_some() {
                shared.render.lock().unwrap_or_else(|e| e.into_inner()).clear();
            }
            let voice = if suppress {
                let (clean, probability) = denoiser.run(&mono);
                mono = clean;
                probability
            } else if neural {
                denoiser.detect(&mono)
            } else {
                None
            };
            let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len().max(1) as f32).sqrt();
            let db = 20.0 * rms.max(1e-6).log10();
            let mut open = state.update(db, threshold, automatic);
            // Advanced voice activity: the network's voice detection decides,
            // so a fan or keyboard loud enough to pass the level never opens it.
            if neural && let Some(probability) = voice {
                open = state.voice(probability > 0.55 && db > -70.0);
            }
            // Push-to-talk overrides both: the key, and nothing else.
            if let Some(key) = ptt {
                let held = key_down(key);
                // Discord's ptt_start and ptt_stop, unless muted (its
                // sound player's rule for push-to-talk).
                if held != ptt_held && ptt_sounds && !shared.muted.load(std::sync::atomic::Ordering::Relaxed) && !shared.deaf.load(std::sync::atomic::Ordering::Relaxed) {
                    crate::sounds::play(if held { crate::sounds::Sound::PttStart } else { crate::sounds::Sound::PttStop }, sound_device.clone(), sound_volume);
                }
                ptt_held = held;
                open = state.voice(held);
            }
            // The gain comes after the gate's measure, so the sensitivity
            // keeps meaning the microphone's own level.
            if steady {
                mono = gain.get_or_insert_with(Processor::gain).run(&mono, None);
            } else {
                gain = None;
            }
            // The mic test keeps us out of the call: nothing is sent.
            let testing = shared.loopback.load(std::sync::atomic::Ordering::Relaxed);
            let sending = open && !testing;
            let _ = gate.send(Gate::Level(db));
            if sending != state.was_open {
                state.was_open = sending;
                let _ = gate.send(if sending { Gate::Open } else { Gate::Closed });
            }
            if testing {
                // What others would hear: after suppression, only while the
                // gate is open.
                let mut output = shared.output.lock().unwrap_or_else(|e| e.into_inner());
                for s in &mono {
                    let s = if open { *s } else { 0.0 };
                    output.push_back(s);
                    output.push_back(s);
                }
                let excess = output.len().saturating_sub(MAX_OUTPUT);
                output.drain(..excess);
            }
            if sending && !shared.muted.load(std::sync::atomic::Ordering::Relaxed) {
                // A soft limiter last: the volume (up to 200%) and the
                // automatic gain may push peaks past full scale, which Opus
                // turns into harsh clipping.
                let mut limited = 0u64;
                for s in &mut mono {
                    let a = s.abs();
                    if a > 0.9 {
                        limited += 1;
                        *s = s.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh());
                    }
                }
                if limited > 0 {
                    shared.mic.limited.fetch_add(limited, std::sync::atomic::Ordering::Relaxed);
                }
                let mut samples = shared.mic.samples.lock().unwrap_or_else(|e| e.into_inner());
                for s in mono {
                    samples.push_back(s);
                    samples.push_back(s);
                }
                // Never more than 100 ms behind.
                let excess = samples.len().saturating_sub(RATE as usize / 5);
                samples.drain(..excess);
            }
        };
        while let Ok(data) = samples.recv() {
            process(&data);
        }
            })
            .ok()?;
        let error = |e| log::warn!("microphone: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                {
                    let raw = raw.clone();
                    move |data: &[f32], _: &_| {
                        let _ = raw.try_send(data.to_vec());
                    }
                },
                error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &_| {
                    let f: Vec<f32> = data.iter().map(|s| f32::from(*s) / 32768.0).collect();
                    let _ = raw.try_send(f);
                },
                error,
                None,
            ),
            other => {
                log::warn!("microphone: format {other:?} not supported");
                return None;
            }
        };
        let stream = stream.ok()?;
        stream.play().ok()?;
        Some(stream)
    });

    // Speakers.
    let output = pick(&settings.output_device, false).and_then(|device| {
        let config = device.default_output_config().ok()?;
        let channels = usize::from(config.channels());
        let rate = config.sample_rate().0;
        log::info!(
            "voice: output {:?}, {} Hz, {} channels, {:?}",
            device.name().unwrap_or_default(),
            rate,
            channels,
            config.sample_format()
        );
        if rate < 32_000 {
            log::warn!("voice: the output runs at {rate} Hz: a Bluetooth headset in call mode (HFP) sounds like this");
        }
        let shared = shared.clone();
        // Plays stereo 48 kHz at the device's rate, interpolating between
        // frames. Voice arrives in 20 ms bursts on the network's clock and
        // the device drains on its own, so a jitter buffer sits between
        // them: playback starts once PREBUFFER is queued, waits again after
        // running dry, and trims a queue that grew past MAX_QUEUE (clock
        // drift) back to the target, so neither crackles nor lag build up.
        const PREBUFFER: usize = RATE as usize * 2 * 80 / 1000;
        const MAX_QUEUE: usize = RATE as usize * 2 * 300 / 1000;
        let step = f64::from(RATE) / f64::from(rate);
        let mut phase = 0.0f64;
        let mut previous = [0.0f32; 2];
        let mut current = [0.0f32; 2];
        let mut playing = false;
        let mut played: Vec<f32> = Vec::new();
        let mut fill = move |data: &mut [f32]| {
            let deaf = shared.deaf.load(std::sync::atomic::Ordering::Relaxed);
            let (volume, cancel_echo) = {
                let settings = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                (settings.output_volume, settings.echo_cancellation)
            };
            // The echo canceller's reference: the call as played, never the
            // mic test's own voice (it would cancel the voice under test).
            let reference = cancel_echo && !shared.loopback.load(std::sync::atomic::Ordering::Relaxed);
            let mut queue = shared.output.lock().unwrap_or_else(|e| e.into_inner());
            if !playing && queue.len() >= PREBUFFER {
                playing = true;
            }
            if queue.len() > MAX_QUEUE {
                let excess = (queue.len() - PREBUFFER) & !1;
                queue.drain(..excess);
            }
            for frame in data.chunks_mut(channels) {
                phase += step;
                while phase >= 1.0 {
                    phase -= 1.0;
                    previous = current;
                    if playing && queue.len() >= 2 {
                        current = [queue.pop_front().unwrap_or(0.0), queue.pop_front().unwrap_or(0.0)];
                    } else {
                        // Dry: fade to silence and buffer again.
                        if playing {
                            shared.underruns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        playing = false;
                        current = [current[0] * 0.9, current[1] * 0.9];
                    }
                    if reference {
                        played.push(if deaf { 0.0 } else { (current[0] + current[1]) * 0.5 * volume });
                    }
                }
                let t = phase as f32;
                let left = previous[0] + (current[0] - previous[0]) * t;
                let right = previous[1] + (current[1] - previous[1]) * t;
                // Stereo on the first two channels: a mono device gets both
                // mixed, a surround one silence on the rest. Copying the
                // right side into every other channel put it six times into
                // a 7.1 headset's virtualiser, each copy a little late.
                for (i, sample) in frame.iter_mut().enumerate() {
                    let value = match (channels, i) {
                        (1, _) => (left + right) * 0.5,
                        (_, 0) => left,
                        (_, 1) => right,
                        _ => 0.0,
                    };
                    *sample = if deaf { 0.0 } else { value * volume };
                }
            }
            drop(queue);
            if !played.is_empty() {
                let mut render = shared.render.lock().unwrap_or_else(|e| e.into_inner());
                render.extend(played.drain(..));
                // A second at most, when no microphone drains it.
                let excess = render.len().saturating_sub(RATE as usize);
                render.drain(..excess);
            }
            if let Ok(mut recording) = shared.recording.try_lock()
                && let Some(recording) = recording.as_mut()
            {
                recording.played_rate = rate;
                recording.played_channels = channels;
                recording.played.extend_from_slice(data);
            }
        };
        let error = |e| log::warn!("speaker: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &_| fill(data),
                error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
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
            ),
            other => {
                log::warn!("speaker: format {other:?} not supported");
                return None;
            }
        };
        let stream = stream.ok()?;
        stream.play().ok()?;
        Some(stream)
    });

    if input.is_none() {
        log::warn!("voice: no microphone");
    }
    if output.is_none() {
        log::warn!("voice: no audio output");
    }
    let _ = stop.recv();
    drop(input);
    drop(output);
}

/// Voice-activity gate with an optional self-adjusting threshold: the noise
/// floor follows the quietest recent level, and voice is what rises well
/// above it.
#[derive(Default)]
struct GateState {
    floor: Option<f32>,
    open_until: Option<Instant>,
    was_open: bool,
}

impl GateState {
    fn update(&mut self, db: f32, threshold: f32, automatic: bool) -> bool {
        let floor = self.floor.get_or_insert(db);
        // Falls fast to quiet levels, climbs slowly with steady noise.
        *floor = if db < *floor { db } else { *floor + (db - *floor) * 0.002 };
        let limit = if automatic { (*floor + 14.0).max(-60.0) } else { threshold };
        let now = Instant::now();
        if db > limit {
            self.open_until = Some(now + HANGOVER);
        }
        self.open_until.is_some_and(|until| now < until)
    }

    /// The gate driven by voice detection instead of level, same hangover.
    fn voice(&mut self, speaking: bool) -> bool {
        let now = Instant::now();
        if speaking {
            self.open_until = Some(now + HANGOVER);
        }
        self.open_until.is_some_and(|until| now < until)
    }
}

/// RNNoise over 10 ms frames (480 samples at 48 kHz). Returns the cleaned
/// samples (one frame behind) and the highest voice probability seen.
///
/// The cleaning is DeepFilterNet3's, the open model closest to Discord's
/// Krisp: full band, so voices keep their air instead of RNNoise's dull
/// edge, and it removes noise RNNoise lets through (keyboards, dogs,
/// music in the room). RNNoise still runs beside it, cheaply, for its voice
/// probability, which drives the gate; and it cleans on its own if the
/// model could not load.
#[derive(Default)]
struct Denoiser {
    state: Option<Box<nnnoiseless::DenoiseState<'static>>>,
    model: Option<DeepFilter>,
    pending: Vec<f32>,
    /// Samples (scaled as RNNoise wants) waiting for detection alone.
    probe: Vec<f32>,
}

struct DeepFilter {
    tract: df::tract::DfTract,
    noisy: ndarray::Array2<f32>,
    clean: ndarray::Array2<f32>,
}

impl Denoiser {
    /// With DeepFilterNet3 loaded: done before the microphone starts, as
    /// the load takes longer than an audio callback may.
    fn with_model() -> Self {
        let started = std::time::Instant::now();
        // No post filter, which dulled the voice further, and the
        // attenuation held to 20 dB: at full strength the model took
        // 15-20 dB off 1-4 kHz and zeroed quiet syllables it judged noise
        // (frames under -10 dB of local SNR), which people heard as a
        // muffled voice with pops and gaps. The thresholds let quieter
        // speech through before that zero mask applies.
        let params = df::tract::RuntimeParams::default_with_ch(1).with_post_filter(0.0).with_atten_lim(20.0).with_thresholds(-15.0, 30.0, 20.0);
        let model = df::tract::DfTract::new(df::tract::DfParams::default(), &params)
            .map_err(|e| log::warn!("voice: noise suppression: {e:#}"))
            .ok()
            .filter(|m| m.sr == RATE as usize && m.hop_size == nnnoiseless::DenoiseState::FRAME_SIZE)
            .map(|tract| DeepFilter {
                noisy: ndarray::Array2::zeros((1, tract.hop_size)),
                clean: ndarray::Array2::zeros((1, tract.hop_size)),
                tract,
            });
        if model.is_some() {
            log::info!("voice: DeepFilterNet3 loaded in {:?}", started.elapsed());
        }
        Self { model, ..Self::default() }
    }

    /// RNNoise's voice probability alone, the samples left as they are:
    /// voice detection with the suppression off.
    fn detect(&mut self, input: &[f32]) -> Option<f32> {
        let state = self.state.get_or_insert_with(nnnoiseless::DenoiseState::new);
        self.probe.extend(input.iter().map(|x| x * 32768.0));
        let frame = nnnoiseless::DenoiseState::FRAME_SIZE;
        let mut buffer = [0.0f32; nnnoiseless::DenoiseState::FRAME_SIZE];
        let mut probability: Option<f32> = None;
        let whole = self.probe.len() / frame * frame;
        for chunk in self.probe[..whole].chunks_exact(frame) {
            let p = state.process_frame(&mut buffer, chunk);
            probability = Some(probability.map_or(p, |q| q.max(p)));
        }
        self.probe.drain(..whole);
        probability
    }

    fn run(&mut self, input: &[f32]) -> (Vec<f32>, Option<f32>) {
        let state = self.state.get_or_insert_with(nnnoiseless::DenoiseState::new);
        self.pending.extend_from_slice(input);
        let frame = nnnoiseless::DenoiseState::FRAME_SIZE;
        let mut out = Vec::with_capacity(self.pending.len());
        let mut probability: Option<f32> = None;
        let mut buffer = vec![0.0f32; frame];
        let mut scaled = vec![0.0f32; frame];
        while self.pending.len() >= frame {
            let chunk: Vec<f32> = self.pending.drain(..frame).collect();
            for (s, x) in scaled.iter_mut().zip(&chunk) {
                *s = x * 32768.0;
            }
            let p = state.process_frame(&mut buffer, &scaled);
            probability = Some(probability.map_or(p, |q| q.max(p)));
            let cleaned = self.model.as_mut().and_then(|m| {
                for (n, x) in m.noisy.iter_mut().zip(&chunk) {
                    *n = x.clamp(-0.999, 0.999);
                }
                m.tract.process(m.noisy.view(), m.clean.view_mut()).ok()?;
                Some(m.clean.iter().copied().collect::<Vec<f32>>())
            });
            match cleaned {
                Some(cleaned) => out.extend(cleaned),
                None => out.extend(buffer.iter().map(|s| s / 32768.0)),
            }
        }
        (out, probability)
    }
}

/// WebRTC's audio processing, as Discord's client runs it, from sonora (a
/// pure Rust port), over 10 ms frames at 48 kHz: the echo canceller (AEC3,
/// with its high-pass filter) before the noise suppression, fed what the
/// speakers play; or the adaptive gain (AGC2, with its limiter) after it.
struct Processor {
    apm: sonora::AudioProcessing,
    pending: Vec<f32>,
    frame: Vec<f32>,
    far: Vec<f32>,
    scratch: Vec<f32>,
}

/// The speakers' and the microphone's buffers together, as WebRTC wants
/// told; its own estimator finds the exact delay from there.
const ECHO_DELAY_MS: i32 = 40;

impl Processor {
    fn new(config: sonora::Config) -> Self {
        let stream = sonora::StreamConfig::new(RATE, 1);
        let apm = sonora::AudioProcessing::builder().config(config).capture_config(stream).render_config(stream).build();
        Self { apm, pending: Vec::new(), frame: vec![0.0; FRAME], far: Vec::new(), scratch: vec![0.0; FRAME] }
    }

    fn echo() -> Self {
        Self::new(sonora::Config { echo_canceller: Some(sonora::config::EchoCanceller::default()), ..Default::default() })
    }

    fn gain() -> Self {
        let gain = sonora::config::GainController2 { adaptive_digital: Some(sonora::config::AdaptiveDigital::default()), ..Default::default() };
        Self::new(sonora::Config { gain_controller2: Some(gain), ..Default::default() })
    }

    /// The input, processed, in whole frames (one frame behind at most);
    /// with `render`, what the speakers played since is fed first.
    fn run(&mut self, input: &[f32], render: Option<&Mutex<VecDeque<f32>>>) -> Vec<f32> {
        if let Some(render) = render {
            // Taken in one go, so the speakers' thread never waits on this.
            self.far.extend(render.lock().unwrap_or_else(|e| e.into_inner()).drain(..));
            let whole = self.far.len() / FRAME * FRAME;
            for frame in self.far[..whole].chunks_exact(FRAME) {
                let _ = self.apm.process_render_f32(&[frame], &mut [&mut self.scratch]);
            }
            self.far.drain(..whole);
        }
        self.pending.extend_from_slice(input);
        let whole = self.pending.len() / FRAME * FRAME;
        let mut out = Vec::with_capacity(whole);
        for frame in self.pending[..whole].chunks_exact(FRAME) {
            if render.is_some() {
                let _ = self.apm.set_stream_delay_ms(ECHO_DELAY_MS);
            }
            match self.apm.process_capture_f32(&[frame], &mut [&mut self.frame]) {
                Ok(()) => out.extend_from_slice(&self.frame),
                Err(_) => out.extend_from_slice(frame),
            }
        }
        self.pending.drain(..whole);
        out
    }
}

/// Whether a key or mouse button is held, wherever the focus is.
#[cfg(windows)]
fn key_down(key: u32) -> bool {
    // SAFETY: GetAsyncKeyState only reads the key's state.
    let state = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key as i32) };
    state as u16 & 0x8000 != 0
}

#[cfg(not(windows))]
fn key_down(_key: u32) -> bool {
    false
}

/// The key or mouse button that went down now, for recording a
/// push-to-talk bind: any key at all, as Discord lets you pick. The left and
/// right mouse buttons are left out, since they press the record button.
#[cfg(windows)]
pub fn pressed_key() -> Option<u32> {
    (0x03..=0xFE).find(|&vk| {
        // The generic Shift/Ctrl/Alt codes repeat their left/right ones.
        !matches!(vk, 0x10..=0x12) && key_down(vk)
    })
}

#[cfg(not(windows))]
pub fn pressed_key() -> Option<u32> {
    None
}

/// A key's name as the keyboard's layout calls it ("V", "Caps Lock", "Ç").
pub fn key_name(vk: u32) -> String {
    match vk {
        0x01 => return t!("Left Mouse Button").into(),
        0x02 => return t!("Right Mouse Button").into(),
        0x04 => return t!("Middle Mouse Button").into(),
        0x05 => return t!("Mouse Button 4").into(),
        0x06 => return t!("Mouse Button 5").into(),
        _ => {}
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyNameTextW, MAPVK_VK_TO_VSC, MapVirtualKeyW};
        // SAFETY: both calls only read the keyboard layout; the buffer's
        // length is passed along.
        unsafe {
            let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
            // Keys on the extended block share scan codes with the keypad.
            let extended = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x6F | 0x90 | 0xA3 | 0xA5);
            let param = ((scan << 16) | if extended { 1 << 24 } else { 0 }) as i32;
            let mut buffer = [0u16; 64];
            let len = GetKeyNameTextW(param, buffer.as_mut_ptr(), buffer.len() as i32);
            if len > 0 {
                return String::from_utf16_lossy(&buffer[..len as usize]);
            }
        }
    }
    t!("Key {code}", code = format!("{vk:#04X}"))
}

/// Linear resampling from a device's rate to 48 kHz.
struct Resampler {
    step: f64,
    phase: f64,
    last: f32,
}

impl Resampler {
    fn new(from: u32) -> Self {
        Self { step: f64::from(from) / f64::from(RATE), phase: 0.0, last: 0.0 }
    }

    fn run(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.step - 1.0).abs() < f64::EPSILON {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        let mut index = 0usize;
        while index < input.len() {
            let next = input[index];
            let t = self.phase as f32;
            out.push(self.last + (next - self.last) * t);
            self.phase += self.step;
            while self.phase >= 1.0 && index < input.len() {
                self.phase -= 1.0;
                self.last = input[index];
                index += 1;
            }
        }
        out
    }
}

// ---- received voice ---------------------------------------------------------

struct Receiver {
    /// Which connection this is: a voice server move or a channel switch
    /// leaves the old one closing after the new one started.
    generation: u64,
    shared: Arc<Shared>,
    ssrcs: Arc<Mutex<HashMap<u32, Id>>>,
    events: mpsc::UnboundedSender<Internal>,
    speaking: Mutex<HashSet<Id>>,
    /// An Opus decoder per sender: songbird only decrypts (see
    /// [`opus_frame`] for why it no longer decodes).
    decoders: Mutex<HashMap<u32, songbird::driver::opus::Decoder>>,
    /// Frames and bytes per sender, logged once after five seconds: a
    /// sender whose frames come out a dozen bytes long sounds like hiss,
    /// and this shows it in the log at once.
    sizes: Mutex<HashMap<u32, (u32, u64)>>,
}

impl Receiver {
    /// A tick's audio from one sender, 48 kHz stereo: the packet's frame,
    /// or a concealed 20 ms when it was lost.
    fn decode(&self, ssrc: u32, packet: Option<&songbird::events::context_data::RtpData>, recording: Option<&mut Recording>) -> Option<Vec<i16>> {
        use songbird::driver::opus;
        let mut decoders = self.decoders.lock().unwrap_or_else(|e| e.into_inner());
        let decoder = match decoders.entry(ssrc) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                // Nothing to conceal before a sender's first packet.
                packet?;
                entry.insert(opus::Decoder::new(RATE, opus::Channels::Stereo).ok()?)
            }
        };
        // Up to 120 ms, Opus's longest packet.
        let mut pcm = vec![0i16; 5760 * 2];
        let Some(data) = packet else {
            let samples = decoder.decode(&[], &mut pcm[..1920], false).ok()?;
            pcm.truncate(samples * 2);
            return Some(pcm);
        };
        let (frame, trailer) = opus_frame(&data.packet, data.payload_offset, data.payload_end_pad)?;
        {
            let mut sizes = self.sizes.lock().unwrap_or_else(|e| e.into_inner());
            let entry = sizes.entry(ssrc).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += frame.len() as u64;
            if entry.0 == 250 {
                let average = entry.1 / u64::from(entry.0);
                let rate = average * 8 * 50 / 1000;
                if average < 24 {
                    log::warn!("voice: {ssrc} sends Opus frames of {average} bytes ({rate} kb/s): too short, it will hiss");
                } else {
                    log::info!("voice: {ssrc} sends Opus frames of {average} bytes (~{rate} kb/s)");
                }
            }
        }
        let samples = match decoder.decode(frame, &mut pcm, false) {
            Ok(samples) => samples,
            Err(error) => {
                log::debug!("voice: invalid Opus packet from {ssrc}: {error:?}");
                return None;
            }
        };
        pcm.truncate(samples * 2);
        if let Some(recording) = recording {
            recording.trailers += u64::from(trailer);
            recording.opus.entry(ssrc).or_default().push((frame.to_vec(), samples as u64));
            recording.raw.push((ssrc, data.payload_offset as u16, data.payload_end_pad as u16, data.packet.to_vec()));
        }
        Some(pcm)
    }
}

#[async_trait::async_trait]
impl EventHandler for Receiver {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(update) => {
                if let Some(user) = update.user_id {
                    self.ssrcs
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(update.ssrc, user.0);
                }
            }
            EventContext::VoiceTick(tick) => {
                let ssrcs = self.ssrcs.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let volumes = self
                    .shared
                    .settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .user_volumes
                    .clone();
                let mut mix = vec![0.0f32; 1920];
                let mut now: HashSet<Id> = HashSet::new();
                self.shared.ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if tick.speaking.values().any(|d| d.packet.is_none()) {
                    self.shared.lost.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let mut recording = self.shared.recording.lock().unwrap_or_else(|e| e.into_inner());
                for (ssrc, data) in &tick.speaking {
                    let user = ssrcs.get(ssrc).copied();
                    if let Some(user) = user {
                        now.insert(user);
                    }
                    let pcm = self.decode(*ssrc, data.packet.as_ref(), recording.as_mut());
                    if let Some(active) = recording.as_mut() {
                        *active.lengths.entry(pcm.as_ref().map_or(0, Vec::len)).or_default() += 1;
                    }
                    let Some(pcm) = pcm else { continue };
                    let volume = user.and_then(|u| volumes.get(&u)).copied().unwrap_or(1.0);
                    // A sender on longer frames hands back more than 20 ms;
                    // all of it is played.
                    if pcm.len() > mix.len() {
                        mix.resize(pcm.len(), 0.0);
                    }
                    for (m, s) in mix.iter_mut().zip(&pcm) {
                        *m += f32::from(*s) / 32768.0 * volume;
                    }
                }
                // Senders who left take their decoder with them.
                self.decoders
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|ssrc, _| tick.speaking.contains_key(ssrc) || tick.silent.contains(ssrc));
                {
                    if let Some(active) = recording.as_mut() {
                        active.clipped += mix.iter().filter(|s| s.abs() > 1.0).count() as u64;
                        active.received.extend_from_slice(&mix);
                        if tick.speaking.is_empty() {
                            active.received.extend(std::iter::repeat_n(0.0, 1920));
                        }
                        if active.full() {
                            let done = recording.take();
                            drop(recording);
                            if let Some(done) = done {
                                std::thread::spawn(move || done.save());
                            }
                            let _ = self.events.send(Internal::Recorded);
                        }
                    }
                }
                if !tick.speaking.is_empty() && !self.shared.loopback.load(std::sync::atomic::Ordering::Relaxed) {
                    let mut output = self.shared.output.lock().unwrap_or_else(|e| e.into_inner());
                    output.extend(mix.iter().map(|s| s.clamp(-1.0, 1.0)));
                    let excess = output.len().saturating_sub(MAX_OUTPUT);
                    output.drain(..excess);
                }
                let mut speaking = self.speaking.lock().unwrap_or_else(|e| e.into_inner());
                if *speaking != now {
                    *speaking = now.clone();
                    let _ = self.events.send(Internal::Speaking(now));
                }
            }
            EventContext::DriverConnect(_) | EventContext::DriverReconnect(_) => {
                let _ = self.events.send(Internal::Connected);
            }
            EventContext::DriverDisconnect(data) => {
                // 4014 and 4022: Discord ended this voice session for good
                // (another device took it, the channel went away, a kick).
                use songbird::events::context_data::DisconnectReason;
                use songbird::model::CloseCode;
                let ended = matches!(data.reason, Some(DisconnectReason::WsClosed(Some(CloseCode::Disconnected | CloseCode::CallTerminated))));
                let reason = data.reason.map_or_else(|| "disconnected".to_owned(), |r| format!("{r:?}"));
                let _ = self.events.send(Internal::Disconnected { generation: self.generation, reason, ended });
            }
            _ => {}
        }
        None
    }
}

enum Internal {
    Speaking(HashSet<Id>),
    Recorded,
    Connected,
    Disconnected { generation: u64, reason: String, ended: bool },
}

// ---- the voice task -----------------------------------------------------------

pub enum VoiceCommand {
    Join { guild: Option<Id>, channel: Id },
    Leave,
    Mute(bool),
    Deaf(bool),
    Settings(AudioSettings),
    /// From the main gateway: our own VOICE_STATE_UPDATE.
    State(Value),
    /// From the main gateway: VOICE_SERVER_UPDATE.
    Server(Value),
    /// The account's voice state from another client. Discord lets each
    /// account be in voice once per server (once per call in DMs): only
    /// when that client joins voice where we are does the call become
    /// theirs, and we leave without telling the gateway, which would drop
    /// them too. Its mutes, its leaving, or a call elsewhere change nothing
    /// here.
    Elsewhere(Value),
    RankRegions,
    /// Mic test on or off: hear yourself as others would.
    MicTest(bool),
    /// Plays a short tone through the speakers.
    TestTone,
    /// Records 20 s of what we hear in the call, for diagnosis.
    Record,
}

/// What the voice task asks of the main gateway.
pub struct GatewayVoice {
    pub guild: Option<Id>,
    pub channel: Option<Id>,
    pub mute: bool,
    pub deaf: bool,
    pub regions: Vec<String>,
}

pub async fn run(
    me: Id,
    http: reqwest::Client,
    mut commands: mpsc::UnboundedReceiver<VoiceCommand>,
    gateway: impl Fn(GatewayVoice) + Send + 'static,
    events: impl Fn(VoiceEvent) + Send + 'static,
) {
    let shared = Arc::new(Shared {
        mic: Arc::new(MicBuffer::default()),
        output: Mutex::new(VecDeque::new()),
        settings: Mutex::new(AudioSettings {
            input_device: None,
            output_device: None,
            input_volume: 1.0,
            output_volume: 1.0,
            sensitivity_db: -50.0,
            automatic: true,
            noise_suppression: true,
            echo_cancellation: true,
            gain_control: true,
            advanced_activity: true,
            push_to_talk: None,
            ptt_sounds: true,
            user_volumes: HashMap::new(),
        }),
        render: Mutex::new(VecDeque::new()),
        recording: Mutex::new(None),
        muted: false.into(),
        deaf: false.into(),
        loopback: false.into(),
        underruns: 0.into(),
        ticks: 0.into(),
        lost: 0.into(),
    });
    let mut stats = tokio::time::interval(Duration::from_secs(15));
    let (gate_tx, mut gate_rx) = mpsc::unbounded_channel::<Gate>();
    let (internal_tx, mut internal_rx) = mpsc::unbounded_channel::<Internal>();

    let mut regions: Vec<(String, u32)> = Vec::new();
    let mut ranked_at: Option<Instant> = None;
    let mut target: Option<(Option<Id>, Id)> = None;
    let mut session: Option<String> = None;
    let mut server: Option<(String, String)> = None;
    let mut driver: Option<Driver> = None;
    let mut track: Option<TrackHandle> = None;
    let mut audio: Option<std::sync::mpsc::Sender<()>> = None;
    let mut speaking_others: HashSet<Id> = HashSet::new();
    let mut me_speaking = false;
    let mut last_level = Instant::now();
    // Diagnostics: how often the microphone opened since the last stats.
    let mut gate_opens = 0u32;
    let (mut mute, mut deaf) = (false, false);
    // The connection now in use; older ones' events are ignored.
    let mut generation: u64 = 0;

    let region_names = |regions: &[(String, u32)]| regions.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>();
    // Out of the call here: the connection, the audio and what was known
    // of it, as a leave does; the gateway hears of it only from a leave.
    macro_rules! drop_call {
        () => {
            if let Some(mut old) = driver.take() {
                old.leave();
            }
            track = None;
            audio = None;
            session = None;
            server = None;
            speaking_others.clear();
            events(VoiceEvent::Channel(None));
            events(VoiceEvent::Speaking(HashSet::new()));
        };
    }

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    VoiceCommand::MicTest(on) => {
                        shared.loopback.store(on, std::sync::atomic::Ordering::Relaxed);
                        if on && audio.is_none() {
                            audio = Some(start_audio(&shared, &gate_tx));
                        } else if !on && driver.is_none() {
                            audio = None;
                            shared.output.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        }
                    }
                    VoiceCommand::Record => {
                        *shared.recording.lock().unwrap_or_else(|e| e.into_inner()) = Some(Recording::default());
                        log::info!("voice: recording {RECORD_SECONDS} s for the diagnostic");
                    }
                    VoiceCommand::TestTone => {
                        if audio.is_none() {
                            audio = Some(start_audio(&shared, &gate_tx));
                        }
                        // A second of A4, faded in and out.
                        let mut output = shared.output.lock().unwrap_or_else(|e| e.into_inner());
                        for i in 0..RATE as usize {
                            let t = i as f32 / RATE as f32;
                            let fade = (t * 20.0).min(1.0).min((1.0 - t) * 20.0);
                            let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.25 * fade;
                            output.push_back(s);
                            output.push_back(s);
                        }
                    }
                    VoiceCommand::RankRegions => {
                        regions = rank_regions(&http).await;
                        ranked_at = Some(Instant::now());
                        events(VoiceEvent::Regions(regions.clone()));
                    }
                    VoiceCommand::Join { guild, channel } => {
                        // Re-rank when stale: the best route can change.
                        if ranked_at.is_none_or(|at| at.elapsed() > Duration::from_secs(15 * 60)) {
                            regions = rank_regions(&http).await;
                            ranked_at = Some(Instant::now());
                            events(VoiceEvent::Regions(regions.clone()));
                        }
                        if let Some(mut old) = driver.take() {
                            old.leave();
                        }
                        track = None;
                        server = None;
                        session = None;
                        target = Some((guild, channel));
                        events(VoiceEvent::Channel(target));
                        gateway(GatewayVoice {
                            guild,
                            channel: Some(channel),
                            mute,
                            deaf,
                            regions: region_names(&regions),
                        });
                    }
                    VoiceCommand::Leave => {
                        if let Some((guild, _)) = target.take() {
                            gateway(GatewayVoice { guild, channel: None, mute, deaf, regions: Vec::new() });
                        }
                        drop_call!();
                    }
                    VoiceCommand::Elsewhere(state) => {
                        let their_channel = state.get("channel_id").and_then(crate::discord::model::sf);
                        let their_guild = state.get("guild_id").and_then(crate::discord::model::sf);
                        let taken = match (target, their_channel) {
                            (Some((Some(guild), _)), Some(_)) => their_guild == Some(guild),
                            (Some((None, channel)), Some(theirs)) => theirs == channel,
                            _ => false,
                        };
                        if taken && let Some((guild, channel)) = target.take() {
                            events(VoiceEvent::Ended { elsewhere: true, guild, channel });
                            drop_call!();
                        }
                    }
                    VoiceCommand::Mute(on) => {
                        mute = on;
                        shared.muted.store(on, std::sync::atomic::Ordering::Relaxed);
                        if on && let Some(track) = &track {
                            let _ = track.pause();
                        }
                        if let Some((guild, channel)) = target {
                            gateway(GatewayVoice { guild, channel: Some(channel), mute, deaf, regions: region_names(&regions) });
                        }
                    }
                    VoiceCommand::Deaf(on) => {
                        deaf = on;
                        shared.deaf.store(on, std::sync::atomic::Ordering::Relaxed);
                        if let Some((guild, channel)) = target {
                            gateway(GatewayVoice { guild, channel: Some(channel), mute, deaf, regions: region_names(&regions) });
                        }
                    }
                    VoiceCommand::Settings(settings) => {
                        let devices_changed = {
                            let mut current = shared.settings.lock().unwrap_or_else(|e| e.into_inner());
                            let changed = current.input_device != settings.input_device
                                || current.output_device != settings.output_device;
                            *current = settings;
                            changed
                        };
                        if devices_changed && audio.is_some() {
                            audio = Some(start_audio(&shared, &gate_tx));
                        }
                    }
                    VoiceCommand::State(state) => {
                        if state.get("user_id").and_then(sf) == Some(me) {
                            if state.get("channel_id").is_some_and(Value::is_null) {
                                // Moved out or disconnected by someone else.
                                if driver.is_some() {
                                    if let Some(mut old) = driver.take() {
                                        old.leave();
                                    }
                                    target = None;
                                    audio = None;
                                    track = None;
                                    events(VoiceEvent::Channel(None));
                                }
                            } else {
                                if let (Some(channel), Some((guild, _))) = (state.get("channel_id").and_then(sf), target) {
                                    target = Some((guild, channel));
                                }
                                session = state.get("session_id").and_then(Value::as_str).map(str::to_owned);
                            }
                        }
                    }
                    VoiceCommand::Server(info) => {
                        let token = info.get("token").and_then(Value::as_str);
                        let endpoint = info.get("endpoint").and_then(Value::as_str);
                        // Every client of the account gets every voice server:
                        // one for another client's call elsewhere is not ours,
                        // and connecting to it hangs on "Connecting".
                        let for_us = match target {
                            Some((Some(guild), _)) => info.get("guild_id").and_then(sf) == Some(guild),
                            Some((None, channel)) => info.get("guild_id").and_then(sf).is_none_or(|g| g == channel) && info.get("channel_id").and_then(sf).is_none_or(|c| c == channel),
                            None => false,
                        };
                        if !for_us {
                            log::info!("voice: a voice server for another client's call, ignored");
                        } else if let (Some(token), Some(endpoint)) = (token, endpoint) {
                            server = Some((token.to_owned(), endpoint.to_owned()));
                        }
                    }
                }
                // Both halves present: connect (again, after a server move).
                if let (Some((guild, channel)), Some(session_id), Some((token, endpoint))) =
                    (target, session.clone(), server.take())
                {
                    let mut new = Driver::new(
                        // Decrypted only: the receiver decodes, from the right
                        // bytes (see `opus_frame`).
                        Config::default().decode_mode(DecodeMode::Decrypt),
                    );
                    let ssrcs = Arc::new(Mutex::new(HashMap::new()));
                    for event in [CoreEvent::SpeakingStateUpdate, CoreEvent::VoiceTick, CoreEvent::DriverConnect, CoreEvent::DriverReconnect, CoreEvent::DriverDisconnect] {
                        new.add_global_event(
                            event.into(),
                            Receiver {
                                generation: generation + 1,
                                shared: shared.clone(),
                                ssrcs: ssrcs.clone(),
                                events: internal_tx.clone(),
                                speaking: Mutex::new(HashSet::new()),
                                decoders: Mutex::new(HashMap::new()),
                                sizes: Mutex::new(HashMap::new()),
                            },
                        );
                    }
                    // A DM call is "guildless": its channel stands in as the server.
                    let server_id = guild.unwrap_or(channel);
                    let nz = |id: Id| std::num::NonZeroU64::new(id).unwrap_or(std::num::NonZeroU64::MIN);
                    let info = ConnectionInfo {
                        channel_id: ChannelId::from(nz(channel)),
                        endpoint,
                        guild_id: GuildId::from(nz(server_id)),
                        session_id,
                        token,
                        user_id: UserId::from(nz(me)),
                    };
                    let connect = new.connect(info);
                    if audio.is_none() {
                        audio = Some(start_audio(&shared, &gate_tx));
                    }
                    shared.mic.samples.lock().unwrap_or_else(|e| e.into_inner()).clear();
                    shared.mic.primed.store(false, std::sync::atomic::Ordering::Relaxed);
                    let handle = new.play_input(RawAdapter::new(MicSource(shared.mic.clone()), RATE, 2).into());
                    // The gate only reports changes: one already open (a mic
                    // test, the call before) would leave the new track paused
                    // until the next silence, and nobody would hear us.
                    if !(me_speaking && !mute) {
                        let _ = handle.pause();
                    }
                    track = Some(handle);
                    driver = Some(new);
                    generation += 1;
                    tokio::spawn(async move {
                        if let Err(error) = connect.await {
                            log::warn!("voice: {error}");
                        }
                    });
                }
            }
            _ = stats.tick() => {
                if driver.is_some() {
                    use std::sync::atomic::Ordering::Relaxed;
                    let queued = shared.output.lock().unwrap_or_else(|e| e.into_inner()).len();
                    log::info!(
                        "voice: queue {} ms, playback underruns {}, ticks {}, ticks with loss {}; mic opened {} times, speaking {}, muted {}, mic late {} ms, mic limited {} samples",
                        queued * 1000 / (RATE as usize * 2),
                        shared.underruns.swap(0, Relaxed),
                        shared.ticks.swap(0, Relaxed),
                        shared.lost.swap(0, Relaxed),
                        std::mem::take(&mut gate_opens),
                        me_speaking,
                        mute,
                        shared.mic.late.swap(0, Relaxed) / 96,
                        shared.mic.limited.swap(0, Relaxed),
                    );
                }
            }
            Some(gate) = gate_rx.recv() => match gate {
                Gate::Open => {
                    me_speaking = true;
                    gate_opens += 1;
                    if !mute && let Some(track) = &track {
                        shared.mic.samples.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        shared.mic.primed.store(false, std::sync::atomic::Ordering::Relaxed);
                        let _ = track.play();
                    }
                    let mut all = speaking_others.clone();
                    all.insert(me);
                    events(VoiceEvent::Speaking(all));
                }
                Gate::Closed => {
                    me_speaking = false;
                    if let Some(track) = &track {
                        let _ = track.pause();
                    }
                    events(VoiceEvent::Speaking(speaking_others.clone()));
                }
                Gate::Level(db) => {
                    if last_level.elapsed() > Duration::from_millis(100) {
                        last_level = Instant::now();
                        events(VoiceEvent::MicLevel(db));
                    }
                }
            },
            Some(internal) = internal_rx.recv() => match internal {
                Internal::Recorded => {}
                Internal::Speaking(users) => {
                    speaking_others = users;
                    let mut all = speaking_others.clone();
                    if me_speaking && !mute {
                        all.insert(me);
                    }
                    events(VoiceEvent::Speaking(all));
                }
                Internal::Connected => events(VoiceEvent::Connected),
                // An older connection's end, after a move: not ours to act on.
                Internal::Disconnected { generation: from, .. } if from != generation => {}
                Internal::Disconnected { reason, ended, .. } => {
                    log::warn!("voice: disconnected: {reason}");
                    if ended {
                        if let Some((guild, channel)) = target.take() {
                            events(VoiceEvent::Ended { elsewhere: false, guild, channel });
                        }
                        drop_call!();
                    } else {
                        events(VoiceEvent::Disconnected);
                    }
                }
            },
        }
    }
}

fn start_audio(shared: &Arc<Shared>, gate: &mpsc::UnboundedSender<Gate>) -> std::sync::mpsc::Sender<()> {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let (shared, gate) = (shared.clone(), gate.clone());
    std::thread::Builder::new()
        .name("cordfast-audio".into())
        .spawn(move || {
            // Windows' timer ticks every 15.6 ms by default, which delivers
            // the 20 ms voice ticks in bursts; 1 ms while audio runs, as
            // voice apps do.
            #[cfg(windows)]
            // SAFETY: plain calls into winmm, paired below.
            unsafe {
                windows_sys::Win32::Media::timeBeginPeriod(1);
            }
            audio_thread(shared, gate, stop_rx);
            #[cfg(windows)]
            // SAFETY: ends the period begun above.
            unsafe {
                windows_sys::Win32::Media::timeEndPeriod(1);
            }
        })
        .expect("unable to start the audio thread");
    stop_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_to_48k() {
        let mut r = Resampler::new(44_100);
        let input = vec![0.5f32; 441];
        let out = r.run(&input);
        // 10 ms at 44.1 kHz is 480 samples at 48 kHz, give or take one.
        assert!((out.len() as i32 - 480).abs() <= 1, "{}", out.len());
        assert!(out.iter().skip(2).all(|s| (s - 0.5).abs() < 1e-3));
        let mut same = Resampler::new(48_000);
        assert_eq!(same.run(&input).len(), 441);
    }

    #[test]
    fn denoiser_keeps_pace_and_detects_silence() {
        let mut d = Denoiser::default();
        // 100 ms of silence in 10 ms blocks: every block comes back whole.
        let mut total = 0;
        let mut last = None;
        for _ in 0..10 {
            let (out, probability) = d.run(&[0.0; 480]);
            total += out.len();
            last = probability;
        }
        assert_eq!(total, 4800);
        assert!(last.is_some_and(|p| p < 0.5), "{last:?}");
    }

    /// Music through libopus as a bot sends it (stereo, 20 ms, 128 kb/s),
    /// in RTP packets as songbird stores them after DAVE decryption (the
    /// trailer still after the frame, the transport's 4-byte nonce and
    /// 16-byte tag after that), decoded the way songbird 0.6.1 did and the
    /// way [`opus_frame`] does, against a clean libopus decode:
    /// `cargo test dave_trailer -- --nocapture`.
    #[test]
    fn dave_trailer_harms_music_and_opus_frame_removes_it() {
        use songbird::driver::opus;
        let mut seed = 7u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        // Chords on both sides, a little apart, over shared noise: L/R
        // correlation near music's 0.6-0.9.
        let source: Vec<i16> = (0..RATE as usize * 5)
            .flat_map(|i| {
                let t = i as f32 / RATE as f32;
                let tone = |f: f32| (t * f * std::f32::consts::TAU).sin();
                let common = (tone(220.0) + tone(277.2) + tone(329.6)) * 0.12 + noise() * 0.08;
                let left = common + tone(880.0) * 0.05;
                let right = common + tone(1318.5) * 0.05;
                [(left * 32767.0) as i16, (right * 32767.0) as i16]
            })
            .collect();
        let mut encoder = opus::Encoder::new(RATE, opus::Channels::Stereo, opus::Application::Audio).unwrap();
        encoder.set_bitrate(opus::Bitrate::Bits(128_000)).unwrap();
        let mut frames = Vec::new();
        let mut packets = Vec::new();
        for (n, pcm) in source.chunks(1920).enumerate() {
            let mut frame = vec![0u8; 1500];
            let len = encoder.encode(pcm, &mut frame).unwrap();
            frame.truncate(len);
            // RTP header with a one-word extension, as Discord sends.
            let mut packet = vec![0x90, 0x78, 0, n as u8, 0, 0, 0, 0, 0, 0, 0, 1];
            packet.extend([0xbe, 0xde, 0, 1, 0x10, 0xff, 0, 0]);
            packet.extend(&frame);
            // 8-byte tag, a 2-byte nonce, the size byte, 0xFAFA.
            packet.extend((0..8).map(|i| (n * 31 + i * 17) as u8));
            packet.extend([0x81, 0x02, 13, 0xfa, 0xfa]);
            packet.extend([0x55; 20]);
            frames.push(frame);
            packets.push(packet);
        }
        let decode = |frames: &mut dyn Iterator<Item = &[u8]>| -> Vec<i16> {
            let mut decoder = opus::Decoder::new(RATE, opus::Channels::Stereo).unwrap();
            let mut out = Vec::new();
            for frame in frames {
                let mut pcm = vec![0i16; 5760 * 2];
                let samples = decoder.decode(frame, &mut pcm, false).unwrap();
                out.extend_from_slice(&pcm[..samples * 2]);
            }
            out
        };
        let clean = decode(&mut frames.iter().map(Vec::as_slice));
        // songbird: the payload minus the transport's suffix, minus the
        // extension, and nothing else.
        let songbird = decode(&mut packets.iter().map(|p| &p[12 + 8..p.len() - 20]));
        // What a voice tick hands over: the payload's end as an index.
        let end = |p: &[u8]| p.len() - 12 - 20;
        let ours = decode(&mut packets.iter().map(|p| {
            let (frame, trailer) = opus_frame(p, 0, end(p)).unwrap();
            assert!(trailer);
            frame
        }));
        // Read as a count of bytes to drop, as songbird's documentation
        // says, the same field left the frame's first few bytes: the hiss
        // the owner heard from bots and people alike.
        let as_count = decode(&mut packets.iter().map(|p| {
            let payload = &p[12..];
            let body = &payload[..payload.len() - end(p)];
            &body[8.min(body.len())..]
        }));
        let correlation = |pcm: &[i16]| {
            let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
            for f in pcm.chunks(2).skip(4800) {
                let (l, r) = (f64::from(f[0]), f64::from(f[1]));
                lr += l * r;
                ll += l * l;
                rr += r * r;
            }
            lr / (ll * rr).sqrt()
        };
        // The damage, in dB below the signal.
        let (mut signal, mut error) = (0.0f64, 0.0f64);
        for (a, b) in clean.iter().zip(&songbird).skip(9600) {
            signal += f64::from(*a).powi(2);
            error += (f64::from(*a) - f64::from(*b)).powi(2);
        }
        let snr = 10.0 * (signal / error.max(1.0)).log10();
        println!(
            "L/R correlation: source {:.2}, clean libopus {:.2}, as songbird {:.2}, CordFast {:.2}; songbird vs clean: {snr:.1} dB",
            correlation(&source),
            correlation(&clean),
            correlation(&songbird),
            correlation(&ours)
        );
        let (mut signal, mut error) = (0.0f64, 0.0f64);
        for (a, b) in clean.iter().zip(&as_count).skip(9600) {
            signal += f64::from(*a).powi(2);
            error += (f64::from(*a) - f64::from(*b)).powi(2);
        }
        let snr_count = 10.0 * (signal / error.max(1.0)).log10();
        println!("read as a count: correlation {:.2}, {snr_count:.1} dB", correlation(&as_count));
        assert_eq!(ours, clean, "CordFast must decode as the reference does");
        assert!(snr < 20.0, "the trailer hardly changed the sound ({snr:.1} dB)");
        assert!(snr_count < 3.0, "read as a count the frame should turn into noise ({snr_count:.1} dB)");
    }

    #[test]
    fn opus_frame_leaves_plain_frames_alone() {
        // No DAVE, no extension: the payload minus the suffix.
        let mut packet = vec![0x80, 0x78, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1];
        packet.extend([0xfc, 1, 2, 3, 0xfa, 0xfa]);
        packet.extend([0; 20]);
        let (frame, trailer) = opus_frame(&packet, 0, packet.len() - 12 - 20).unwrap();
        assert_eq!(frame, [0xfc, 1, 2, 3, 0xfa, 0xfa]);
        assert!(!trailer);
        // RTP padding comes off before the trailer is looked for.
        let mut padded = vec![0xa0, 0x78, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0xfc, 9];
        padded.extend([0, 0, 3]);
        padded.extend([0; 20]);
        assert_eq!(opus_frame(&padded, 0, padded.len() - 12 - 20).unwrap().0, [0xfc, 9]);
        assert!(opus_frame(&[0x80, 0x78], 0, 0).is_none());
    }

    /// The diagnostic's Ogg file decodes in ffmpeg to the same audio:
    /// `cargo test ogg_opus_in_ffmpeg -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn ogg_opus_in_ffmpeg() {
        use songbird::driver::opus;
        let mut encoder = opus::Encoder::new(RATE, opus::Channels::Stereo, opus::Application::Audio).unwrap();
        let mut decoder = opus::Decoder::new(RATE, opus::Channels::Stereo).unwrap();
        let mut packets = Vec::new();
        let mut ours = Vec::new();
        for n in 0..100 {
            let pcm: Vec<i16> = (0..960).flat_map(|i| {
                let t = (n * 960 + i) as f32 / RATE as f32;
                let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
                [s, s / 2]
            }).collect();
            let mut frame = vec![0u8; 1500];
            let len = encoder.encode(&pcm, &mut frame).unwrap();
            frame.truncate(len);
            let mut out = vec![0i16; 5760 * 2];
            let samples = decoder.decode(&frame, &mut out, false).unwrap();
            ours.extend_from_slice(&out[..samples * 2]);
            packets.push((frame, samples as u64));
        }
        let dir = std::env::temp_dir();
        let ogg = dir.join("cordfast-test.ogg");
        let wav = dir.join("cordfast-test.raw");
        write_ogg_opus(&ogg, &packets);
        let status = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-c:a", "libopus", "-i"])
            .arg(&ogg)
            .args(["-f", "s16le", "-ar", "48000", "-ac", "2"])
            .arg(&wav)
            .status()
            .expect("ffmpeg");
        assert!(status.success());
        let theirs: Vec<i16> = std::fs::read(&wav).unwrap().chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        assert_eq!(theirs.len(), ours.len());
        let worst = ours.iter().zip(&theirs).map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs()).max().unwrap();
        println!("ffmpeg/libopus vs CordFast: {} samples, largest difference {worst}", ours.len());
        assert!(worst <= 1);
    }

    #[test]
    fn opus_summary_reads_bitrate_and_modes() {
        // A second of CELT full band, 20 ms, stereo, 160 bytes a frame.
        let mut frame = vec![0u8; 160];
        frame[0] = 0xfc;
        let packets: Vec<(Vec<u8>, u64)> = (0..50).map(|_| (frame.clone(), 960)).collect();
        assert_eq!(opus_summary(&packets), "64 kb/s; CELT fullband 20 ms stereo: 50");
    }

    #[test]
    fn ogg_crc_matches_the_specification() {
        // The check value of CRC-32/MPEG-2 without its final inversion and
        // initial ones: Ogg's variant.
        assert_eq!(ogg_crc(b"123456789"), 0x89a1_897f);
    }

    #[test]
    fn gate_holds_open_through_short_pauses() {
        let mut gate = GateState::default();
        assert!(!gate.update(-80.0, -50.0, false));
        assert!(gate.update(-20.0, -50.0, false));
        // A word's gap is shorter than the hangover.
        assert!(gate.update(-80.0, -50.0, false));
        assert!(gate.voice(false));
    }
}

#[cfg(test)]
mod devices_test {
    use super::{Denoiser, FRAME, Mutex, Processor, RATE, VecDeque};

    /// DeepFilterNet3's cost on 10 s of noisy, voice-like sound:
    /// `cargo test --release deepfilter_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn deepfilter_cost() {
        let mut denoiser = Denoiser::with_model();
        assert!(denoiser.model.is_some());
        let mut seed = 1u32;
        let audio: Vec<f32> = (0..RATE as usize * 10)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                let t = i as f32 / RATE as f32;
                let voice: f32 = (1..8).map(|h| (t * 180.0 * h as f32 * std::f32::consts::TAU).sin() / h as f32).sum();
                voice * 0.1 * (t * 3.0).sin().abs() + noise * 0.05
            })
            .collect();
        let started = std::time::Instant::now();
        for chunk in audio.chunks(480) {
            denoiser.run(chunk);
        }
        let elapsed = started.elapsed();
        println!("10 s in {elapsed:?}: {:.1}% of one core", elapsed.as_secs_f64() / 10.0 * 100.0);
    }

    /// A voice-like test signal: harmonics with a syllable rhythm.
    fn speech(seconds: f32, pitch: f32) -> Vec<f32> {
        (0..(RATE as f32 * seconds) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let voice: f32 = (1..8).map(|h| (t * pitch * h as f32 * std::f32::consts::TAU).sin() / h as f32).sum();
                voice * 0.1 * (t * 3.0).sin().abs()
            })
            .collect()
    }

    /// The canceller takes the speakers' sound out of the microphone: what
    /// the far end says, played and heard back 30 ms later at half level,
    /// is mostly gone once it has adapted.
    #[test]
    fn echo_canceller_takes_out_what_the_speakers_play() {
        let far = speech(4.0, 180.0);
        let lag = RATE as usize * 30 / 1000;
        let mic: Vec<f32> = (0..far.len()).map(|i| if i >= lag { far[i - lag] * 0.5 } else { 0.0 }).collect();
        let render = Mutex::new(VecDeque::new());
        let mut echo = Processor::echo();
        let mut out = Vec::new();
        for (played, heard) in far.chunks(FRAME).zip(mic.chunks(FRAME)) {
            render.lock().unwrap().extend(played);
            out.extend(echo.run(heard, Some(&render)));
        }
        let energy = |x: &[f32]| x.iter().map(|s| s * s).sum::<f32>();
        let tail = RATE as usize;
        let reduction = 10.0 * (energy(&mic[mic.len() - tail..]) / energy(&out[out.len() - tail..]).max(1e-9)).log10();
        assert!(reduction > 10.0, "only {reduction:.1} dB less echo");
        // Our own voice, with the speakers silent, goes through whole.
        let near = speech(4.0, 240.0);
        let mut echo = Processor::echo();
        let mut out = Vec::new();
        for heard in near.chunks(FRAME) {
            render.lock().unwrap().extend(std::iter::repeat_n(0.0, FRAME));
            out.extend(echo.run(heard, Some(&render)));
        }
        let kept = 10.0 * (energy(&out[out.len() - tail..]) / energy(&near[near.len() - tail..])).log10();
        println!("echo {reduction:.1} dB down, own voice {kept:.1} dB");
        assert!(kept > -3.0, "own voice {kept:.1} dB");
    }

    /// The gain brings a quiet voice up toward a steady level.
    #[test]
    fn gain_control_raises_a_quiet_voice() {
        let quiet: Vec<f32> = speech(4.0, 220.0).iter().map(|s| s * 0.05).collect();
        let mut gain = Processor::gain();
        let mut out = Vec::new();
        for chunk in quiet.chunks(FRAME) {
            out.extend(gain.run(chunk, None));
        }
        let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
        let tail = RATE as usize;
        assert!(rms(&out[out.len() - tail..]) > rms(&quiet[quiet.len() - tail..]) * 2.0);
    }

    /// How each step of the microphone's chain shapes a real voice, band by
    /// band, against the input: `CORDFAST_SPEECH=voice.wav cargo test
    /// --release mic_chain_tone -- --ignored --nocapture` (16-bit mono
    /// 48 kHz). Writes each step's output beside the input to listen to.
    #[test]
    #[ignore]
    fn mic_chain_tone() {
        let Ok(path) = std::env::var("CORDFAST_SPEECH") else { return };
        let bytes = std::fs::read(&path).unwrap();
        let mut at = 12;
        let mut voice = Vec::new();
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if &bytes[at..at + 4] == b"data" {
                voice = bytes[at + 8..(at + 8 + size).min(bytes.len())].chunks_exact(2).map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0).collect();
                break;
            }
            at += 8 + size + size % 2;
        }
        let write = |name: &str, x: &[f32]| {
            let mut out = Vec::new();
            let data = (x.len() * 2) as u32;
            out.extend(b"RIFF");
            out.extend((36 + data).to_le_bytes());
            out.extend(b"WAVEfmt ");
            out.extend(16u32.to_le_bytes());
            out.extend(1u16.to_le_bytes());
            out.extend(1u16.to_le_bytes());
            out.extend(RATE.to_le_bytes());
            out.extend((RATE * 2).to_le_bytes());
            out.extend(2u16.to_le_bytes());
            out.extend(16u16.to_le_bytes());
            out.extend(b"data");
            out.extend(data.to_le_bytes());
            for s in x {
                out.extend(((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
            std::fs::write(std::path::Path::new(&path).with_file_name(format!("{name}.wav")), out).unwrap();
        };
        // Band energies by a plain DFT over 4096-sample windows.
        let bands = |x: &[f32]| -> Vec<f32> {
            const N: usize = 4096;
            let edges = [0.0, 300.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
            let mut sum = vec![0.0f64; edges.len() - 1];
            for window in x.chunks_exact(N).step_by(2) {
                for k in 1..N / 2 {
                    let f = k as f32 * RATE as f32 / N as f32;
                    let Some(b) = edges.windows(2).position(|e| f >= e[0] && f < e[1]) else { continue };
                    let (mut re, mut im) = (0.0f64, 0.0f64);
                    for (n, s) in window.iter().enumerate() {
                        let w = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / N as f64).cos();
                        let a = std::f64::consts::TAU * k as f64 * n as f64 / N as f64;
                        re += f64::from(*s) * w * a.cos();
                        im -= f64::from(*s) * w * a.sin();
                    }
                    sum[b] += re * re + im * im;
                }
            }
            sum.iter().map(|e| (10.0 * (e + 1e-12).log10()) as f32).collect()
        };
        let reference = bands(&voice);
        let report = |name: &str, x: &[f32]| {
            let b = bands(x);
            let line: Vec<String> = b.iter().zip(&reference).map(|(o, i)| format!("{:6.1}", o - i)).collect();
            println!("{name:<28} 0-300 300-1k 1-2k 2-4k 4-8k 8-16k Hz: {}", line.join(" "));
        };
        // Someone else talking in the headphones while we talk: the far end
        // the echo canceller hears, which never reaches the microphone.
        let far: Vec<f32> = voice.iter().rev().map(|s| s * 0.5).collect();
        let silent = vec![0.0f32; voice.len()];
        let run_echo = |far: &[f32]| {
            let render = Mutex::new(VecDeque::new());
            let mut echo = Processor::echo();
            let mut out = Vec::new();
            for (played, heard) in far.chunks(FRAME).zip(voice.chunks(FRAME)) {
                render.lock().unwrap().extend(played);
                out.extend(echo.run(heard, Some(&render)));
            }
            out
        };
        let denoise = |x: &[f32]| {
            let mut d = Denoiser::with_model();
            let mut out = Vec::new();
            for chunk in x.chunks(FRAME) {
                out.extend(d.run(chunk).0);
            }
            out
        };
        report("input", &voice);
        let echo_quiet = run_echo(&silent);
        report("echo, speakers silent", &echo_quiet);
        write("step-echo-quiet", &echo_quiet);
        let echo_talk = run_echo(&far);
        report("echo, other person talking", &echo_talk);
        write("step-echo-talk", &echo_talk);
        let clean = denoise(&voice);
        report("deepfilternet", &clean);
        write("step-deepfilter", &clean);
        let both = denoise(&echo_talk);
        report("echo (talking) + deepfilter", &both);
        write("step-echo-deepfilter", &both);
        let mut gain = Processor::gain();
        let gained: Vec<f32> = both.chunks(FRAME).flat_map(|c| gain.run(c, None)).collect();
        report("+ gain", &gained);
    }

    /// What the echo canceller and the gain cost on this machine:
    /// `cargo test --release echo_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn echo_cost() {
        let far = speech(10.0, 180.0);
        let near = speech(10.0, 240.0);
        let render = Mutex::new(VecDeque::new());
        let (mut echo, mut gain) = (Processor::echo(), Processor::gain());
        let started = std::time::Instant::now();
        for (played, heard) in far.chunks(FRAME).zip(near.chunks(FRAME)) {
            render.lock().unwrap().extend(played);
            let clean = echo.run(heard, Some(&render));
            gain.run(&clean, None);
        }
        let elapsed = started.elapsed();
        println!("10 s in {elapsed:?}: {:.1}% of one core", elapsed.as_secs_f64() / 10.0 * 100.0);
    }

    /// Prints the default devices and formats: `cargo test print_devices -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_devices() {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        if let Some(d) = host.default_output_device() {
            println!("output: {:?} {:?}", d.name(), d.default_output_config());
        }
        if let Some(d) = host.default_input_device() {
            println!("input: {:?} {:?}", d.name(), d.default_input_config());
        }
        for d in host.output_devices().into_iter().flatten() {
            println!("  available output: {:?}", d.name());
        }
    }
}

