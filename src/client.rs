use crate::clipboard::clipboard_listener::{self, ClipboardEvent};
use async_trait::async_trait;
use bytes::Bytes;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Host, StreamConfig,
};
use crossbeam_queue::ArrayQueue;
use magnum_opus::{Channels::*, Decoder as AudioDecoder};
use ringbuf::{ring_buffer::RbBase, Rb};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::c_void,
    ops::Deref,
    str::FromStr,
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex, RwLock,
    },
};
use uuid::Uuid;

use crate::{
    check_port,
    common::input::{MOUSE_BUTTON_LEFT, MOUSE_BUTTON_RIGHT, MOUSE_TYPE_DOWN, MOUSE_TYPE_UP},
    create_symmetric_key_msg, decode_id_pk, decode_id_pk_kx, get_rs_pk,
    is_keyboard_mode_supported,
    ui_interface::{get_builtin_option, use_texture_render},
    ui_session_interface::{InvokeUiSession, Session},
};
pub use file_trait::FileManager;
use hbb_common::{
    allow_err,
    anyhow::{anyhow, Context},
    bail,
    config::{
        self, Config, PeerConfig, PeerInfoSerde, Resolution, CONNECT_TIMEOUT,
        READ_TIMEOUT, RELAY_PORT,
    },
    get_version_number, log,
    protobuf::{Message as _, MessageField},
    rand,
    rendezvous_proto::*,
    sha2::{Digest, Sha256},
    socket_client::connect_tcp_local,
    sodiumoxide::crypto::sign,
    timeout,
    tokio::{
        self,
        sync::mpsc::{unbounded_channel, UnboundedReceiver},
        time::{Duration, Instant},
    },
    ResultType, Stream,
};
use base::{
    config::keys,
    fs::JobType,
    message_proto::{option_message::BoolOption, *},
};
pub use helper::*;
use scrap::{codec::Decoder, CodecFormat, ImageFormat, ImageRgb, ImageTexture};

use crate::clipboard::CLIPBOARD_INTERVAL;
use crate::clipboard::{check_clipboard, ClipboardSide};

pub use super::lang::*;

mod audio_playback;
mod audio_playback_recovery;
#[cfg(test)]
#[path = "client/tests/audio_state_tests.rs"]
mod audio_state_tests;
pub mod file_trait;
pub mod helper;
pub mod io_loop;
pub mod screenshot;

pub const MILLI1: Duration = Duration::from_millis(1);
pub const SEC30: Duration = Duration::from_secs(30);
// Empirical restart reconnect grace window.
const RESTART_REMOTE_DEVICE_GRACE: Duration = Duration::from_secs(5 * 60);
pub const VIDEO_QUEUE_SIZE: usize = 120;
const MAX_DECODE_FAIL_COUNTER: usize = 3;

pub const LOGIN_MSG_PASSWORD_EMPTY: &str = "Empty Password";
pub const LOGIN_MSG_PASSWORD_WRONG: &str = "Wrong Password";
pub const LOGIN_MSG_2FA_WRONG: &str = "Wrong 2FA Code";
pub const REQUIRE_2FA: &'static str = "2FA Required";
pub const LOGIN_MSG_NO_PASSWORD_ACCESS: &str = "No Password Access";
pub const LOGIN_MSG_OFFLINE: &str = "Offline";
pub const LOGIN_MSG_NOT_FOUND: &str = "Computer not found on the network";
pub const LOGIN_SCREEN_WAYLAND: &str = "Wayland login screen is not supported";
pub const SCRAP_X11_REQUIRED: &str = "x11 expected";
pub const SCRAP_X11_REF_URL: &str = "https://rustdesk.com/docs/en/manual/linux/#x11-required";

pub const AUDIO_BUFFER_MS: usize = 3000;
/// On a LAN a reachable peer answers at once; this only bounds the wait for one that is off.
const LAN_CONNECT_TIMEOUT: u64 = 3_000;

/// Client of the remote desktop.
pub struct Client;

struct ClipboardState {
    is_text_required: bool,
    running: bool,
}

lazy_static::lazy_static! {
    static ref AUDIO_HOST: Host = cpal::default_host();
}

lazy_static::lazy_static! {
    static ref ENIGO: Arc<Mutex<enigo::Enigo>> = Arc::new(Mutex::new(enigo::Enigo::new()));
}

lazy_static::lazy_static! {
    static ref CLIPBOARD_STATE: Arc<Mutex<ClipboardState>> = Arc::new(Mutex::new(ClipboardState::new()));
}

pub fn get_key_state(key: enigo::Key) -> bool {
    use enigo::KeyboardControllable;
    ENIGO.lock().unwrap().get_key_state(key)
}

impl Client {
    const CLIENT_CLIPBOARD_NAME: &'static str = "client-clipboard";

    /// Connects straight to the peer: `peer` is an IP, `host:port`, or the ID of a computer
    /// found by LAN discovery.
    pub async fn start(
        peer: &str,
        conn_type: ConnType,
        interface: impl Interface,
    ) -> ResultType<Stream> {
        debug_assert!(peer == interface.get_id());
        if config::is_incoming_only() && !is_switch_sides_back(conn_type, &interface).await {
            bail!("Incoming only mode");
        }
        for addr in peer_addrs(peer) {
            match connect_tcp_local(&*addr, None, LAN_CONNECT_TIMEOUT).await {
                Ok(stream) => return Ok(stream),
                Err(err) => log::info!("Failed to connect to {addr}: {err}"),
            }
        }
        bail!(LOGIN_MSG_NOT_FOUND)
    }

    // Client half of the session handshake; not wired in until the key exchange works without
    // a rendezvous server.
    #[allow(dead_code)]
    async fn secure_connection(
        peer_id: &str,
        signed_id_pk: Vec<u8>,
        key: &str,
        conn: &mut Stream,
    ) -> ResultType<Option<Vec<u8>>> {
        let rs_pk = get_rs_pk(if key.is_empty() {
            config::RS_PUB_KEY
        } else {
            key
        });
        let mut sign_pk = None;
        let mut option_pk = None;
        if !signed_id_pk.is_empty() {
            if let Some(rs_pk) = rs_pk {
                if let Ok((id, pk)) = decode_id_pk(&signed_id_pk, &rs_pk) {
                    if id == peer_id {
                        sign_pk = Some(sign::PublicKey(pk));
                        option_pk = Some(pk.to_vec());
                    }
                }
            }
            if sign_pk.is_none() {
                log::error!("Handshake failed: invalid public key from rendezvous server");
            }
        }
        let sign_pk = match sign_pk {
            Some(v) => v,
            None => {
                // No trusted peer identity (key-less deployment, or a blob that does not verify
                // under our root): fall through on the non-secure path with is_secured() false.
                // send an empty message out in case server is setting up secure and waiting for first message
                conn.send(&Message::new()).await?;
                return Ok(option_pk);
            }
        };
        match timeout(READ_TIMEOUT, conn.next()).await? {
            Some(res) => {
                let bytes = res?;
                if let Ok(msg_in) = Message::parse_from_bytes(&bytes) {
                    if let Some(message::Union::SignedId(si)) = msg_in.union {
                        if let Ok((id, their_pk_b, kx_version)) =
                            decode_id_pk_kx(&si.id, &sign_pk)
                        {
                            if id == peer_id {
                                let (asymmetric_value, symmetric_value, key) =
                                    create_symmetric_key_msg(their_pk_b);
                                let picked = hbb_common::tcp::kx_version_for(kx_version);
                                let mut msg_out = Message::new();
                                msg_out.set_public_key(PublicKey {
                                    asymmetric_value: asymmetric_value.clone(),
                                    symmetric_value,
                                    kx_version: picked,
                                    ..Default::default()
                                });
                                timeout(CONNECT_TIMEOUT, conn.send(&msg_out)).await??;
                                conn.set_negotiated_key(
                                    key,
                                    true,
                                    &hbb_common::tcp::KxTranscript {
                                        initiator_pk: &asymmetric_value,
                                        responder_pk: &their_pk_b,
                                        advertised: kx_version,
                                        picked,
                                    },
                                )?;
                            } else {
                                log::error!("Handshake failed: sign failure");
                                conn.send(&Message::new()).await?;
                            }
                        } else {
                            // fall back to non-secure connection in case pk mismatch
                            log::info!("pk mismatch, fall back to non-secure");
                            let mut msg_out = Message::new();
                            msg_out.set_public_key(PublicKey::new());
                            conn.send(&msg_out).await?;
                        }
                    } else {
                        log::error!("Handshake failed: invalid message type");
                        conn.send(&Message::new()).await?;
                    }
                } else {
                    log::error!("Handshake failed: invalid message format");
                    conn.send(&Message::new()).await?;
                }
            }
            None => {
                bail!("Reset by the peer");
            }
        }
        Ok(option_pk)
    }

    #[inline]
    pub fn set_is_text_clipboard_required(b: bool) {
        CLIPBOARD_STATE.lock().unwrap().is_text_required = b;
    }


    fn try_stop_clipboard() {
        // Disconnected Flutter sessions may keep UI handlers alive, so only connected sessions
        // should block clipboard cleanup.
        if crate::flutter::sessions::has_connected_sessions_running(ConnType::DEFAULT_CONN) {
            return;
        }
        clipboard_listener::unsubscribe(Self::CLIENT_CLIPBOARD_NAME);
        CLIPBOARD_STATE.lock().unwrap().running = false;
    }

    // `try_start_clipboard` is called by all session when connection is established. (When handling peer info).
    // This function only create one thread with a loop, the loop is shared by all sessions.
    // After all sessions are end, the loop exists.
    //
    // If clipboard update is detected, the text will be sent to all sessions by `send_clipboard_msg`.
    fn try_start_clipboard() -> Option<UnboundedReceiver<()>> {
        let mut clipboard_lock = CLIPBOARD_STATE.lock().unwrap();
        if clipboard_lock.running {
            return None;
        }

        let (tx_cb_result, rx_cb_result) = mpsc::channel();
        if let Err(e) =
            clipboard_listener::subscribe(Self::CLIENT_CLIPBOARD_NAME.to_owned(), tx_cb_result)
        {
            log::error!("Failed to subscribe clipboard listener: {}", e);
            return None;
        }

        clipboard_lock.running = true;
        let (tx_started, rx_started) = unbounded_channel();

        log::info!("Start client clipboard loop");
        std::thread::spawn(move || {
            let mut handler = ClientClipboardHandler {
                ctx: None,
            };

            tx_started.send(()).ok();
            loop {
                if !CLIPBOARD_STATE.lock().unwrap().running {
                    break;
                }
                match rx_cb_result.recv_timeout(Duration::from_millis(CLIPBOARD_INTERVAL)) {
                    Ok(ClipboardEvent::Changed) => {
                        handler.check_clipboard(true);
                    }
                    Ok(ClipboardEvent::Stop) => {
                        log::debug!("Clipboard listener stopped");
                        break;
                    }
                    Ok(ClipboardEvent::StopWithError(err)) => {
                        log::error!("Clipboard listener stopped with error: {}", err);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        log::error!("Clipboard listener disconnected");
                        break;
                    }
                }
            }
            log::info!("Stop client clipboard loop");
            CLIPBOARD_STATE.lock().unwrap().running = false;
        });

        Some(rx_started)
    }

}

impl ClipboardState {
    fn new() -> Self {
        Self {
            is_text_required: true,
            running: false,
        }
    }
}

struct ClientClipboardHandler {
    ctx: Option<crate::clipboard::ClipboardContext>,
}

impl ClientClipboardHandler {
    fn is_text_required(&self) -> bool {
        {
            CLIPBOARD_STATE.lock().unwrap().is_text_required
        }
    }


    fn check_clipboard(&mut self, check_text: bool) {
        if CLIPBOARD_STATE.lock().unwrap().running {

            if check_text && self.is_text_required() {
                if let Some(msg) = check_clipboard(&mut self.ctx, ClipboardSide::Client, false) {
                    self.send_msg(msg, false);
                }
            }
        }
    }

    #[inline]
    fn send_msg(&self, msg: Message, _is_file: bool) {
        crate::flutter::send_clipboard_msg(msg, _is_file);
    }
}

/// Audio handler for the [`Client`].
#[derive(Default)]
pub struct AudioHandler {
    audio_decoder: Option<(AudioDecoder, Vec<f32>)>,
    audio_buffer: AudioBuffer,
    audio_resampler: Option<crate::audio_resampler::AudioResampler>,
    sample_rate: (u32, u32),
    audio_stream: Option<Box<dyn StreamTrait>>,
    channels: u16,
    device_channel: u16,
    playback_status: Arc<audio_playback::AudioPlaybackStatus>,
    playback_recovery: audio_playback_recovery::PlaybackRecovery,
}

#[derive(Clone, Copy)]
struct DecodedAudioConfig {
    sample_rate: u32,
    input_channels: u16,
    output_channels: u16,
}

fn create_audio_resampler(
    input_rate: u32,
    output_rate: u32,
    channels: u16,
) -> ResultType<Option<crate::audio_resampler::AudioResampler>> {
    if input_rate == output_rate {
        return Ok(None);
    }
    Ok(Some(crate::audio_resampler::AudioResampler::new(
        crate::audio_resampler::AudioResamplerConfig {
            input_rate,
            output_rate,
            channels,
        },
    )?))
}

fn prepare_decoded_audio(
    input: &[f32],
    resampler: Option<&mut crate::audio_resampler::AudioResampler>,
    config: DecodedAudioConfig,
) -> Result<Vec<f32>, crate::audio_resampler::AudioResamplerError> {
    let mut output = match resampler {
        Some(resampler) => resampler.process(input)?,
        None => input.to_owned(),
    };
    if config.input_channels != config.output_channels {
        output = crate::audio_rechannel(
            output,
            config.sample_rate,
            config.sample_rate,
            config.input_channels,
            config.output_channels,
        );
    }
    Ok(output)
}

struct AudioBuffer(
    pub Arc<std::sync::Mutex<ringbuf::HeapRb<f32>>>,
    usize,
    [usize; 30],
    Arc<std::sync::atomic::AtomicUsize>,
);

impl Default for AudioBuffer {
    fn default() -> Self {
        Self(
            Arc::new(std::sync::Mutex::new(
                ringbuf::HeapRb::<f32>::new(48000 * 2 * AUDIO_BUFFER_MS / 1000), // 48000hz, 2 channel
            )),
            48000 * 2,
            [0; 30],
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        )
    }
}

impl AudioBuffer {
    pub fn resize(&mut self, sample_rate: usize, channels: usize) {
        let capacity = sample_rate * channels * AUDIO_BUFFER_MS / 1000;
        let old_capacity = self.0.lock().unwrap().capacity();
        if capacity != old_capacity {
            *self.0.lock().unwrap() = ringbuf::HeapRb::<f32>::new(capacity);
            self.1 = sample_rate * channels;
            log::info!("Audio buffer resized from {old_capacity} to {capacity}");
        }
    }

    fn try_shrink(&mut self, having: usize) {
        extern crate chrono;
        use chrono::prelude::*;

        let mut i = (having * 10) / self.1;
        if i > 29 {
            i = 29;
        }
        self.2[i] += 1;

        #[allow(non_upper_case_globals)]
        static mut tms: i64 = 0;
        let dt = Local::now().timestamp_millis();
        unsafe {
            if tms == 0 {
                tms = dt;
                return;
            } else if dt < tms + 12000 {
                return;
            }
            tms = dt;
        }

        // the safer water mark to drop
        let mut zero = 0;
        // the water mark taking most of time
        let mut max = 0;
        for i in 0..30 {
            if self.2[i] == 0 && zero == i {
                zero += 1;
            }

            if self.2[i] > self.2[max] {
                self.2[max] = 0;
                max = i;
            } else {
                self.2[i] = 0;
            }
        }
        zero = zero * 2 / 3;

        // how many data can be dropped:
        // 1. will not drop if buffered data is less than 600ms
        // 2. choose based on min(zero, max)
        const N: usize = 4;
        self.2[max] = 0;
        if max < 6 {
            return;
        } else if max > zero * N {
            max = zero * N;
        }

        let mut lock = self.0.lock().unwrap();
        let cap = lock.capacity();
        let having = lock.occupied_len();
        let skip = (cap * max / (30 * N) + 1) & (!1);
        if (having > skip * 3) && (skip > 0) {
            lock.skip(skip);
            let generation = self.signal_discontinuity();
            drop(lock);
            log::info!("skip {skip}, based {max} {zero}, generation={generation}");
        }
    }

    /// The caller must hold the PCM buffer lock while signaling the discard.
    fn signal_discontinuity(&self) -> usize {
        self.3
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1)
    }

    /// append pcm to audio buffer, if buffered data
    /// exceeds AUDIO_BUFFER_MS,  only AUDIO_BUFFER_MS
    /// will be kept.
    fn append_pcm2(&self, buffer: &[f32]) -> usize {
        let mut lock = self.0.lock().unwrap();
        let cap = lock.capacity();
        let having = lock.occupied_len() + buffer.len();
        lock.push_slice_overwrite(buffer);
        let discard = (having > cap).then(|| (having - cap, self.signal_discontinuity()));
        let occupied = lock.occupied_len();
        drop(lock);
        if let Some((discarded, generation)) = discard {
            hbb_common::throttled_log!(
                audio_playback::AUDIO_PLAYBACK_LOG_INTERVAL,
                debug,
                "Audio buffer capacity discard: samples={discarded}, generation={generation}"
            );
        }
        occupied
    }

    /// append pcm to audio buffer, trying to drop data
    /// when data is too much (per 12 seconds) based
    /// statistics.
    pub fn append_pcm(&mut self, buffer: &[f32]) {
        let having = self.append_pcm2(buffer);
        self.try_shrink(having);
    }
}

impl AudioHandler {

    /// Start the audio playback.
    fn start_audio(&mut self, format0: AudioFormat) -> ResultType<()> {
        let device = AUDIO_HOST
            .default_output_device()
            .with_context(|| "Failed to get default output device")?;
        log::info!(
            "Using default output device: \"{}\"",
            device.name().unwrap_or("".to_owned())
        );
        let config = device.default_output_config().map_err(|e| anyhow!(e))?;
        let sample_format = config.sample_format();
        log::info!("Default output format: {:?}", config);
        log::info!("Remote input format: {:?}", format0);
        #[allow(unused_mut)]
        let mut config: StreamConfig = config.into();
        {
            // this makes ios and android audio output not work
            config.buffer_size = cpal::BufferSize::Fixed(64);
        }

        self.sample_rate = (format0.sample_rate, config.sample_rate.0);
        let audio_resampler = create_audio_resampler(
            format0.sample_rate, config.sample_rate.0, format0.channels as _,
        )?;
        let mut build_output_stream = |config: StreamConfig| match sample_format {
            cpal::SampleFormat::I8 => self.build_output_stream::<i8>(&config, &device),
            cpal::SampleFormat::I16 => self.build_output_stream::<i16>(&config, &device),
            cpal::SampleFormat::I32 => self.build_output_stream::<i32>(&config, &device),
            cpal::SampleFormat::I64 => self.build_output_stream::<i64>(&config, &device),
            cpal::SampleFormat::U8 => self.build_output_stream::<u8>(&config, &device),
            cpal::SampleFormat::U16 => self.build_output_stream::<u16>(&config, &device),
            cpal::SampleFormat::U32 => self.build_output_stream::<u32>(&config, &device),
            cpal::SampleFormat::U64 => self.build_output_stream::<u64>(&config, &device),
            cpal::SampleFormat::F32 => self.build_output_stream::<f32>(&config, &device),
            cpal::SampleFormat::F64 => self.build_output_stream::<f64>(&config, &device),
            f => bail!("unsupported audio format: {:?}", f),
        };
        if config.channels > format0.channels as _ {
            let no_rechannel_config = StreamConfig {
                channels: format0.channels as _,
                ..config.clone()
            };
            if let Err(_) = build_output_stream(no_rechannel_config) {
                build_output_stream(config)?;
            }
        } else {
            build_output_stream(config)?;
        }
        self.audio_resampler = audio_resampler;

        Ok(())
    }

    /// Handle audio format and create an audio decoder.
    pub fn handle_format(&mut self, f: AudioFormat) {
        self.handle_format_with_start(f, Self::start_audio);
    }

    fn handle_format_with_start(
        &mut self,
        f: AudioFormat,
        start: impl FnOnce(&mut Self, AudioFormat) -> ResultType<()>,
    ) {
        if !is_supported_audio_channel_count(f.channels) {
            log::error!("Unsupported audio channel count: {}", f.channels);
            return;
        }
        match AudioDecoder::new(f.sample_rate, if f.channels > 1 { Stereo } else { Mono }) {
            Ok(d) => {
                let playback_failed = self.cancel_pending_playback();
                let keep_existing_stream = self.audio_stream.is_some()
                    && self.sample_rate.0 == f.sample_rate
                    && u32::from(self.channels) == f.channels;
                let buffer = vec![0.; f.sample_rate as usize * f.channels as usize];
                let mut previous = std::mem::take(self);
                self.prepare_playback(&f);
                self.audio_decoder = Some((d, buffer));
                self.channels = f.channels as _;
                let result = start(self, f);
                let keep_existing_stream = keep_existing_stream
                    && !playback_failed
                    && !previous.playback_recovery.report_pending();
                if result.is_err() && keep_existing_stream {
                    // The restarted capture has new Opus history even when output startup fails.
                    previous.audio_decoder = self.audio_decoder.take();
                    *self = previous;
                    self.handle_audio_start_result(result, true);
                    return;
                }
                self.finish_playback_replacement(result, keep_existing_stream.then_some(previous));
            }
            Err(err) => {
                log::error!("Failed to create audio decoder: {}", err);
            }
        }
    }

    fn handle_audio_start_result(&mut self, result: ResultType<()>, keep_existing_stream: bool) {
        if let Err(error) = result {
            if keep_existing_stream {
                log::error!(
                    "Failed to replace audio playback stream; keeping the existing compatible stream: {error:#}"
                );
            } else {
                *self = Self::default();
                log::error!("Failed to start audio playback: {error:#}");
            }
        }
    }

    /// Handle audio frame and play it.
    #[inline]
    pub fn handle_frame(&mut self, frame: AudioFrame) {
        self.playback_status.report_errors();
        if self.audio_stream.is_none()
            || !self
                .playback_status
                .ready
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        self.audio_decoder.as_mut().map(|(d, buffer)| {
            let decoded_frames = match d.decode_float(&frame.data, buffer, false) {
                Ok(decoded_frames) => decoded_frames,
                Err(error) => {
                    log::warn!("Failed to decode audio frame: {error:?}");
                    return;
                }
            };
            let channels = self.channels;
            let n = decoded_frames * channels as usize;
            {
                let config = DecodedAudioConfig {
                    sample_rate: self.sample_rate.1,
                    input_channels: self.channels,
                    output_channels: self.device_channel,
                };
                let buffer = match prepare_decoded_audio(
                    &buffer[0..n],
                    self.audio_resampler.as_mut(),
                    config,
                ) {
                    Ok(output) => output,
                    Err(error) => {
                        log::error!("Failed to resample decoded audio: {error:#}");
                        return;
                    }
                };
                self.audio_buffer.append_pcm(&buffer);
            }
        });
    }

    /// Build audio output stream for current device.
    fn build_output_stream<T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        config: &StreamConfig,
        device: &Device,
    ) -> ResultType<()> {
        self.device_channel = config.channels;
        let err_fn = self.playback_recovery.new_error_callback();
        self.audio_buffer
            .resize(config.sample_rate.0 as _, config.channels as _);
        let audio_buffer = self.audio_buffer.0.clone();
        let discontinuity_generation = self.audio_buffer.3.clone();
        let mut playback_writer = audio_playback::AudioPlaybackWriter::new(
            audio_playback::AudioPlaybackConfig {
                sample_rate: config.sample_rate.0,
                channels: config.channels as usize,
            },
            audio_buffer,
            discontinuity_generation,
        )?;
        let playback_status = playback_writer.status.clone();
        let timeout = None;
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                playback_writer.write_output(data);
            },
            err_fn,
            timeout,
        )?;
        stream.play()?;
        self.audio_stream = Some(Box::new(stream));
        self.playback_status = playback_status;
        Ok(())
    }
}

fn is_supported_audio_channel_count(channels: u32) -> bool {
    (1..=2).contains(&channels)
}

#[cfg(test)]
mod audio_format_tests {
    use super::is_supported_audio_channel_count;

    #[test]
    fn only_mono_and_stereo_are_supported() {
        assert!(is_supported_audio_channel_count(1));
        assert!(is_supported_audio_channel_count(2));
        assert!(!is_supported_audio_channel_count(0));
        assert!(!is_supported_audio_channel_count(u32::MAX));
    }

    #[test]
    fn failed_audio_start_discards_format_state() {
        use super::{anyhow, AudioDecoder, AudioHandler, Stereo};

        const SAMPLE_RATE: u32 = 48_000;
        const CHANNELS: u16 = 2;
        let decoder = AudioDecoder::new(SAMPLE_RATE, Stereo).unwrap();
        let mut handler = AudioHandler {
            audio_decoder: Some((decoder, Vec::new())),
            sample_rate: (SAMPLE_RATE, SAMPLE_RATE),
            channels: CHANNELS,
            ..Default::default()
        };

        handler.handle_audio_start_result(Err(anyhow!("Injected playback startup failure")), false);

        assert!(handler.audio_decoder.is_none());
        assert_eq!(handler.channels, 0);
        assert_eq!(handler.sample_rate, (0, 0));
    }
}

/// Video handler for the [`Client`].
pub struct VideoHandler {
    decoder: Decoder,
    pub rgb: ImageRgb,
    pub texture: ImageTexture,
    _display: usize, // useful for debug
    fail_counter: usize,
    first_frame: bool,
}

impl VideoHandler {
    pub fn get_adapter_luid() -> Option<i64> {
        crate::flutter::get_adapter_luid()
    }

    /// Create a new video handler.
    pub fn new(format: CodecFormat, _display: usize) -> Self {
        let luid = Self::get_adapter_luid();
        log::info!("new video handler for display #{_display}, format: {format:?}, luid: {luid:?}");
        let rgba_format =
            if cfg!(feature = "flutter") && (cfg!(windows) || cfg!(target_os = "linux")) {
                ImageFormat::ABGR
            } else {
                ImageFormat::ARGB
            };
        VideoHandler {
            decoder: Decoder::new(format, luid),
            rgb: ImageRgb::new(rgba_format, crate::get_dst_align_rgba()),
            texture: Default::default(),
            _display,
            fail_counter: 0,
            first_frame: true,
        }
    }

    /// Handle a new video frame.
    #[inline]
    pub fn handle_frame(
        &mut self,
        vf: VideoFrame,
        pixelbuffer: &mut bool,
        chroma: &mut Option<Chroma>,
    ) -> ResultType<bool> {
        let format = CodecFormat::from(&vf);
        if format != self.decoder.format() {
            self.reset(Some(format));
        }
        match &vf.union {
            Some(frame) => {
                let res = self.decoder.handle_video_frame(
                    frame,
                    &mut self.rgb,
                    &mut self.texture,
                    pixelbuffer,
                    chroma,
                );
                if res.as_ref().is_ok_and(|x| *x) {
                    self.fail_counter = 0;
                } else {
                    if self.fail_counter < usize::MAX {
                        if self.first_frame && self.fail_counter < MAX_DECODE_FAIL_COUNTER {
                            log::error!("decode first frame failed");
                            self.fail_counter = MAX_DECODE_FAIL_COUNTER;
                        } else {
                            self.fail_counter += 1;
                        }
                        log::error!(
                            "Failed to handle video frame, fail counter: {}",
                            self.fail_counter
                        );
                    }
                }
                self.first_frame = false;
                res
            }
            _ => Ok(false),
        }
    }

    /// Reset the decoder, change format if it is Some
    pub fn reset(&mut self, format: Option<CodecFormat>) {
        log::info!(
            "reset video handler for display #{}, format: {format:?}",
            self._display
        );
        let luid = Self::get_adapter_luid();
        let format = format.unwrap_or(self.decoder.format());
        self.decoder = Decoder::new(format, luid);
        self.fail_counter = 0;
        self.first_frame = true;
    }
}

// The source of sent password
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum PasswordSource {
    SharedAb(String),
    Undefined,
}

impl Default for PasswordSource {
    fn default() -> Self {
        PasswordSource::Undefined
    }
}

impl PasswordSource {
    // Whether the password is shared ab password
    pub fn is_shared_ab(&self, password: &[u8], hash: &Hash) -> bool {
        if password.is_empty() {
            return false;
        }
        match self {
            PasswordSource::SharedAb(p) => Self::equal(p, password, hash),
            _ => false,
        }
    }

    //  Whether the password equals to the connected password
    fn equal(password: &str, connected_password: &[u8], hash: &Hash) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(password);
        hasher.update(&hash.salt);
        let res = hasher.finalize();
        connected_password[..] == res[..]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ConnToken {
    password: Vec<u8>,
    password_source: PasswordSource,
    session_id: u64,
}

/// Login config handler for [`Client`].
#[derive(Default)]
pub struct LoginConfigHandler {
    id: String,
    pub conn_type: ConnType,
    hash: Hash,
    password: Vec<u8>, // remember password for reconnect
    pub remember: bool,
    config: PeerConfig,
    pub version: i64,
    pub session_id: u64, // used for local <-> server communication
    pub supported_encoding: SupportedEncoding,
    restarting_remote_device: bool,
    // Start time of the restart grace window. On Windows the peer may briefly
    // reconnect before the real reboot disconnect.
    restart_remote_device_at: Option<Instant>,
    switch_uuid: Option<String>,
    switch_back_allowed: bool,
    pub save_ab_password_to_recent: bool, // true: connected with ab password
    pub custom_fps: Arc<Mutex<Option<usize>>>,
    pub last_auto_fps: Option<usize>,
    pub adapter_luid: Option<i64>,
    pub mark_unsupported: Vec<CodecFormat>,
    pub selected_windows_session_id: Option<u32>,
    pub peer_info: Option<PeerInfo>,
    password_source: PasswordSource, // where the sent password comes from
    pub enable_trusted_devices: bool,
}

impl Deref for LoginConfigHandler {
    type Target = PeerConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl LoginConfigHandler {
    /// Initialize the login config handler.
    ///
    /// # Arguments
    ///
    /// * `id` - id of peer
    /// * `conn_type` - Connection type enum.
    pub fn initialize(
        &mut self,
        id: String,
        conn_type: ConnType,
        switch_uuid: Option<String>,
        adapter_luid: Option<i64>,
        conn_token: Option<String>,
    ) {
        self.id = id;
        self.conn_type = conn_type;
        let config = self.load_config();
        self.remember = !config.password.is_empty();
        self.config = config;

        let conn_token = conn_token
            .map(|x| serde_json::from_str::<ConnToken>(&x).ok())
            .flatten();
        let mut sid = 0;
        if let Some(token) = conn_token {
            sid = token.session_id;
            self.password = token.password; // use as last password
            self.password_source = token.password_source;
        }
        if sid == 0 {
            sid = rand::random();
            if sid == 0 {
                // you won the lottery
                sid = 1;
            }
        }
        self.session_id = sid;
        self.supported_encoding = Default::default();
        self.clear_restarting_remote_device();
        {
            self.switch_back_allowed = false;
        }
        self.switch_uuid = switch_uuid;
        self.adapter_luid = adapter_luid;
        self.selected_windows_session_id = None;
    }

    pub fn allow_switch_back_once(&mut self) {
        self.switch_back_allowed = true;
    }

    pub fn consume_switch_back_permission(&mut self) -> bool {
        if self.switch_back_allowed {
            self.switch_back_allowed = false;
            true
        } else {
            false
        }
    }

    /// Check if the client should auto login.
    /// Return password if the client should auto login, otherwise return empty string.
    pub fn should_auto_login(&self) -> String {
        let l = self.lock_after_session_end.v;
        let a = !self.get_option("auto-login").is_empty();
        let p = self.get_option("os-password");
        if !p.is_empty() && l && a {
            p
        } else {
            "".to_owned()
        }
    }

    /// Load [`PeerConfig`].
    pub fn load_config(&self) -> PeerConfig {
        debug_assert!(self.id.len() > 0);
        PeerConfig::load(&self.id)
    }

    /// Save a [`PeerConfig`] into the handler.
    ///
    /// # Arguments
    ///
    /// * `config` - [`PeerConfig`] to save.
    pub fn save_config(&mut self, config: PeerConfig) {
        config.store(&self.id);
        self.config = config;
    }

    /// Set an option for handler's [`PeerConfig`].
    ///
    /// # Arguments
    ///
    /// * `k` - key of option
    /// * `v` - value of option
    pub fn set_option(&mut self, k: String, v: String) {
        let mut config = self.load_config();
        if v == self.get_option(&k) {
            return;
        }
        config.options.insert(k, v);
        self.save_config(config);
    }

    //to-do: too many dup code below.

    /// Save view style to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The view style to be saved.
    pub fn save_view_style(&mut self, value: String) {
        let mut config = self.load_config();
        config.view_style = value;
        self.save_config(config);
    }

    /// Save keyboard mode to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The view style to be saved.
    pub fn save_keyboard_mode(&mut self, value: String) {
        let mut config = self.load_config();
        config.keyboard_mode = value;
        self.save_config(config);
    }

    /// Save reverse mouse wheel ("", "Y") to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The reverse mouse wheel ("", "Y").
    pub fn save_reverse_mouse_wheel(&mut self, value: String) {
        let mut config = self.load_config();
        config.reverse_mouse_wheel = value;
        self.save_config(config);
    }

    /// Save "displays_as_individual_windows" ("", "Y") to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The "displays_as_individual_windows" value ("", "Y").
    pub fn save_displays_as_individual_windows(&mut self, value: String) {
        let mut config = self.load_config();
        config.displays_as_individual_windows = value;
        self.save_config(config);
    }

    /// Save "use_all_my_displays_for_the_remote_session" ("", "Y") to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The "use_all_my_displays_for_the_remote_session" value ("", "Y").
    pub fn save_use_all_my_displays_for_the_remote_session(&mut self, value: String) {
        let mut config = self.load_config();
        config.use_all_my_displays_for_the_remote_session = value;
        self.save_config(config);
    }

    /// Save scroll style to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The scroll style to be saved.
    pub fn save_scroll_style(&mut self, value: String) {
        let mut config = self.load_config();
        config.scroll_style = value;
        self.save_config(config);
    }

    /// Save edge scroll edge thickness to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The edge thickness to be saved.
    pub fn save_edge_scroll_edge_thickness(&mut self, value: i32) {
        let mut config = self.load_config();
        config.edge_scroll_edge_thickness = value;
        self.save_config(config);
    }

    /// Set a ui config of flutter for handler's [`PeerConfig`].
    ///
    /// # Arguments
    ///
    /// * `k` - key of option
    /// * `v` - value of option
    pub fn save_ui_flutter(&mut self, k: String, v: String) {
        let mut config = self.load_config();
        if v.is_empty() {
            config.ui_flutter.remove(&k);
        } else {
            config.ui_flutter.insert(k, v);
        }
        self.save_config(config);
    }

    /// Get a ui config of flutter for handler's [`PeerConfig`].
    /// Return String if the option is found, otherwise return "".
    ///
    /// # Arguments
    ///
    /// * `k` - key of option
    pub fn get_ui_flutter(&self, k: &str) -> String {
        if let Some(v) = self.config.ui_flutter.get(k) {
            v.clone()
        } else {
            "".to_owned()
        }
    }

    /// Toggle an option in the handler.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the option to toggle.
    ///
    // It's Ok to check the option empty in this function.
    // `toggle_option()` is only called in a session.
    // Custom client advanced settings will not effect this function.
    pub fn toggle_option(&mut self, name: String) -> Option<Message> {
        let mut option = OptionMessage::default();
        let mut config = self.load_config();
        if name == "show-remote-cursor" {
            config.show_remote_cursor.v = !config.show_remote_cursor.v;
            option.show_remote_cursor = (if config.show_remote_cursor.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "follow-remote-cursor" {
            config.follow_remote_cursor.v = !config.follow_remote_cursor.v;
            option.follow_remote_cursor = (if config.follow_remote_cursor.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "follow-remote-window" {
            config.follow_remote_window.v = !config.follow_remote_window.v;
            option.follow_remote_window = (if config.follow_remote_window.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "disable-audio" {
            config.disable_audio.v = !config.disable_audio.v;
            option.disable_audio = (if config.disable_audio.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "disable-clipboard" {
            config.disable_clipboard.v = !config.disable_clipboard.v;
            option.disable_clipboard = (if config.disable_clipboard.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "lock-after-session-end" {
            config.lock_after_session_end.v = !config.lock_after_session_end.v;
            option.lock_after_session_end = (if config.lock_after_session_end.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "enable-file-copy-paste" {
            config.enable_file_copy_paste.v = !config.enable_file_copy_paste.v;
            option.enable_file_transfer = (if config.enable_file_copy_paste.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "show-quality-monitor" {
            config.show_quality_monitor.v = !config.show_quality_monitor.v;
        } else if name == "allow_swap_key" {
            config.allow_swap_key.v = !config.allow_swap_key.v;
        } else if name == "view-only" {
            config.view_only.v = !config.view_only.v;
            let f = |b: bool| {
                if b {
                    BoolOption::Yes.into()
                } else {
                    BoolOption::No.into()
                }
            };
            if config.view_only.v {
                option.disable_keyboard = f(true);
                option.disable_clipboard = f(true);
                option.show_remote_cursor = f(true);
                option.enable_file_transfer = f(false);
                option.lock_after_session_end = f(false);
            } else {
                option.disable_keyboard = f(false);
                option.disable_clipboard = f(self.get_toggle_option("disable-clipboard"));
                option.show_remote_cursor = f(self.get_toggle_option("show-remote-cursor"));
                option.enable_file_transfer = f(self.config.enable_file_copy_paste.v);
                option.lock_after_session_end = f(self.config.lock_after_session_end.v);
                if config.show_my_cursor.v {
                    config.show_my_cursor.v = false;
                    option.show_my_cursor = BoolOption::No.into();
                }
            }
        } else if name == "show-my-cursor" {
            config.show_my_cursor.v = !config.show_my_cursor.v;
            option.show_my_cursor = if config.show_my_cursor.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            }
            .into();
        } else {
            let is_set = self
                .options
                .get(&name)
                .map(|o| !o.is_empty())
                .unwrap_or(false);
            if is_set {
                self.config.options.remove(&name);
            } else {
                self.config.options.insert(name, "Y".to_owned());
            }
            self.config.store(&self.id);
            return None;
        }


        self.save_config(config);
        let mut misc = Misc::new();
        misc.set_option(option);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        Some(msg_out)
    }

    /// Get [`PeerConfig`] of the current [`LoginConfigHandler`].
    ///
    /// # Arguments
    pub fn get_config(&mut self) -> &mut PeerConfig {
        &mut self.config
    }

    /// Get [`OptionMessage`] of the current [`LoginConfigHandler`].
    /// Return `None` if there's no option, for example, when the session is only for file transfer.
    ///
    /// # Arguments
    ///
    /// * `ignore_default` - If `true`, ignore the default value of the option.
    fn get_option_message(&self, ignore_default: bool) -> Option<OptionMessage> {
        if self.conn_type.eq(&ConnType::FILE_TRANSFER) {
            return None;
        }
        let mut msg = OptionMessage::new();
        let q = self.image_quality.clone();
        if let Some(q) = self.get_image_quality_enum(&q, ignore_default) {
            msg.image_quality = q.into();
        } else if q == "custom" {
            let config = self.load_config();
            let quality = if config.custom_image_quality.is_empty() {
                50
            } else {
                config.custom_image_quality[0]
            };
            msg.custom_image_quality = quality << 8;
            if let Some(custom_fps) = self.options.get("custom-fps") {
                let custom_fps = custom_fps.parse().unwrap_or(30);
                msg.custom_fps = custom_fps;
                *self.custom_fps.lock().unwrap() = Some(custom_fps as _);
            }
        }
        let view_only = self.get_toggle_option("view-only");
        if view_only {
            msg.disable_keyboard = BoolOption::Yes.into();
        }
        if view_only || self.get_toggle_option("show-remote-cursor") {
            msg.show_remote_cursor = BoolOption::Yes.into();
        }
        if view_only && self.get_toggle_option("show-my-cursor") {
            msg.show_my_cursor = BoolOption::Yes.into();
        }
        if self.get_toggle_option("follow-remote-cursor") {
            msg.follow_remote_cursor = BoolOption::Yes.into();
        }
        if self.get_toggle_option("follow-remote-window") {
            msg.follow_remote_window = BoolOption::Yes.into();
        }
        if !view_only && self.get_toggle_option("lock-after-session-end") {
            msg.lock_after_session_end = BoolOption::Yes.into();
        }
        if self.get_toggle_option("disable-audio") {
            msg.disable_audio = BoolOption::Yes.into();
        }
        if !view_only && self.get_toggle_option(keys::OPTION_ENABLE_FILE_COPY_PASTE) {
            msg.enable_file_transfer = BoolOption::Yes.into();
        }
        if view_only || self.get_toggle_option("disable-clipboard") {
            msg.disable_clipboard = BoolOption::Yes.into();
        }
        msg.supported_decoding = MessageField::some(self.get_supported_decoding());
        Some(msg)
    }

    pub fn get_supported_decoding(&self) -> SupportedDecoding {
        Decoder::supported_decodings(
            Some(&self.id),
            use_texture_render(),
            self.adapter_luid,
            &self.mark_unsupported,
        )
    }

    /// Parse the image quality option.
    /// Return [`ImageQuality`] if the option is valid, otherwise return `None`.
    ///
    /// # Arguments
    ///
    /// * `q` - The image quality option.
    /// * `ignore_default` - Ignore the default value.
    fn get_image_quality_enum(&self, q: &str, ignore_default: bool) -> Option<ImageQuality> {
        if q == "low" {
            Some(ImageQuality::Low)
        } else if q == "best" {
            Some(ImageQuality::Best)
        } else if q == "balanced" {
            if ignore_default {
                None
            } else {
                Some(ImageQuality::Balanced)
            }
        } else {
            None
        }
    }

    /// Get the status of a toggle option.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the toggle option.
    ///
    // It's Ok to check the option empty in this function.
    // `get_toggle_option()` is only called in a session.
    // Custom client advanced settings will not effect this function.
    pub fn get_toggle_option(&self, name: &str) -> bool {
        if name == "show-remote-cursor" {
            self.config.show_remote_cursor.v
        } else if name == "lock-after-session-end" {
            self.config.lock_after_session_end.v
        } else if name == keys::OPTION_ENABLE_FILE_COPY_PASTE {
            self.config.enable_file_copy_paste.v
        } else if name == "disable-audio" {
            self.config.disable_audio.v
        } else if name == "disable-clipboard" {
            self.config.disable_clipboard.v
        } else if name == "show-quality-monitor" {
            self.config.show_quality_monitor.v
        } else if name == "allow_swap_key" {
            self.config.allow_swap_key.v
        } else if name == "view-only" {
            self.config.view_only.v
        } else if name == "show-my-cursor" {
            self.config.show_my_cursor.v
        } else if name == "follow-remote-cursor" {
            self.config.follow_remote_cursor.v
        } else if name == "follow-remote-window" {
            self.config.follow_remote_window.v
        } else {
            !self.get_option(name).is_empty()
        }
    }

    /// Create a [`Message`] for refreshing video.
    pub fn refresh() -> Message {
        let mut misc = Misc::new();
        misc.set_refresh_video(true);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    /// Create a [`Message`] for refreshing video.
    pub fn refresh_display(display: usize) -> Message {
        let mut misc = Misc::new();
        misc.set_refresh_video_display(display as _);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    /// Create a [`Message`] for saving custom image quality.
    ///
    /// # Arguments
    ///
    /// * `bitrate` - The given bitrate.
    /// * `quantizer` - The given quantizer.
    pub fn save_custom_image_quality(&mut self, image_quality: i32) -> Message {
        let mut misc = Misc::new();
        misc.set_option(OptionMessage {
            custom_image_quality: image_quality << 8,
            ..Default::default()
        });
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        let mut config = self.load_config();
        config.image_quality = "custom".to_owned();
        config.custom_image_quality = vec![image_quality as _];
        self.save_config(config);
        msg_out
    }

    /// Save the given image quality to the config.
    /// Return a [`Message`] that contains image quality, or `None` if the image quality is not valid.
    /// # Arguments
    ///
    /// * `value` - The image quality.
    pub fn save_image_quality(&mut self, value: String) -> Option<Message> {
        let mut res = None;
        if let Some(q) = self.get_image_quality_enum(&value, false) {
            let mut misc = Misc::new();
            misc.set_option(OptionMessage {
                image_quality: q.into(),
                ..Default::default()
            });
            let mut msg_out = Message::new();
            msg_out.set_misc(misc);
            res = Some(msg_out);
        }
        let mut config = self.load_config();
        config.image_quality = value;
        self.save_config(config);
        res
    }

    pub fn save_trackpad_speed(&mut self, speed: i32) {
        let mut config = self.load_config();
        config.trackpad_speed = speed;
        self.save_config(config);
    }

    /// Create a [`Message`] for saving custom fps.
    ///
    /// # Arguments
    ///
    /// * `fps` - The given fps.
    /// * `save_config` - Save the config.
    pub fn set_custom_fps(&mut self, fps: i32, save_config: bool) -> Message {
        let mut misc = Misc::new();
        misc.set_option(OptionMessage {
            custom_fps: fps,
            ..Default::default()
        });
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        if save_config {
            let mut config = self.load_config();
            config
                .options
                .insert("custom-fps".to_owned(), fps.to_string());
            self.save_config(config);
        }
        *self.custom_fps.lock().unwrap() = Some(fps as _);
        msg_out
    }

    pub fn get_option(&self, k: &str) -> String {
        if let Some(v) = self.config.options.get(k) {
            v.clone()
        } else {
            "".to_owned()
        }
    }

    #[inline]
    pub fn get_custom_resolution(&self, display: i32) -> Option<(i32, i32)> {
        self.config
            .custom_resolutions
            .get(&display.to_string())
            .map(|r| (r.w, r.h))
    }

    #[inline]
    pub fn set_custom_resolution(&mut self, display: i32, wh: Option<(i32, i32)>) {
        let display = display.to_string();
        let mut config = self.load_config();
        match wh {
            Some((w, h)) => {
                config
                    .custom_resolutions
                    .insert(display, Resolution { w, h });
            }
            None => {
                config.custom_resolutions.remove(&display);
            }
        }
        self.save_config(config);
    }

    /// Get user name.
    /// Return the name of the given peer. If the peer has no name, return the name in the config.
    ///
    /// # Arguments
    ///
    /// * `pi` - peer info.
    pub fn get_username(&self, pi: &PeerInfo) -> String {
        return if pi.username.is_empty() {
            self.info.username.clone()
        } else {
            pi.username.clone()
        };
    }

    /// Handle peer info.
    ///
    /// # Arguments
    ///
    /// * `username` - The name of the peer.
    /// * `pi` - The peer info.
    pub fn handle_peer_info(&mut self, pi: &PeerInfo) {
        if !pi.version.is_empty() {
            self.version = hbb_common::get_version_number(&pi.version);
        }
        let serde = PeerInfoSerde {
            username: pi.username.clone(),
            hostname: pi.hostname.clone(),
            platform: pi.platform.clone(),
        };
        let mut config = self.load_config();
        config.info = serde;
        let password = self.password.clone();
        let password0 = config.password.clone();
        let remember = self.remember;
        let hash = self.hash.clone();
        if remember {
            // remember is true: use PeerConfig password or ui login
            // not sync shared password to recent
            if !password.is_empty()
                && password != password0
                && !self.password_source.is_shared_ab(&password, &hash)
            {
                config.password = password.clone();
                log::debug!("remember password of {}", self.id);
            }
        } else if !password0.is_empty() {
            config.password = Default::default();
            log::debug!("remove password of {}", self.id);
        }
        if config.keyboard_mode.is_empty() {
            if is_keyboard_mode_supported(
                &KeyboardMode::Map,
                get_version_number(&pi.version),
                &pi.platform,
            ) {
                config.keyboard_mode = KeyboardMode::Map.to_string();
            } else {
                config.keyboard_mode = KeyboardMode::Legacy.to_string();
            }
        } else {
            let keyboard_modes =
                crate::get_supported_keyboard_modes(get_version_number(&pi.version), &pi.platform);
            let current_mode = &KeyboardMode::from_str(&config.keyboard_mode).unwrap_or_default();
            if !keyboard_modes.contains(current_mode) {
                config.keyboard_mode = KeyboardMode::Legacy.to_string();
            }
        }
        // no matter if change, for update file time
        self.save_config(config);
        self.supported_encoding = pi.encoding.clone().unwrap_or_default();
        log::info!("peer info supported_encoding:{:?}", self.supported_encoding);
    }

    pub fn get_remote_dir(&self) -> String {
        serde_json::from_str::<HashMap<String, String>>(&self.get_option("remote_dir"))
            .unwrap_or_default()
            .remove(&self.info.username)
            .unwrap_or_default()
    }

    pub fn get_all_remote_dir(&self, path: String) -> String {
        let d = self.get_option("remote_dir");
        let user = self.info.username.clone();
        let mut x = serde_json::from_str::<HashMap<String, String>>(&d).unwrap_or_default();
        if path.is_empty() {
            x.remove(&user);
        } else {
            x.insert(user, path);
        }
        serde_json::to_string::<HashMap<String, String>>(&x).unwrap_or_default()
    }

    /// Create a [`Message`] for login.
    fn create_login_msg(&self, password: Vec<u8>) -> Message {
        let avatar = get_builtin_option(keys::OPTION_AVATAR);
        let mut display_name = get_builtin_option(keys::OPTION_DISPLAY_NAME);
        if display_name.is_empty() {
            display_name = crate::username();
        }
        let display_name = display_name
            .split_whitespace()
            .map(|word| {
                word.chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if i == 0 {
                            c.to_uppercase().to_string()
                        } else {
                            c.to_string()
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ");
        let my_platform = hbb_common::whoami::platform().to_string();
        let hwid = if self.get_option("trust-this-device") == "Y" {
            crate::get_hwid()
        } else {
            Bytes::new()
        };
        let mut lr = LoginRequest {
            username: self.id.clone(),
            password: password.into(),
            my_id: Config::get_id(),
            my_name: display_name,
            my_platform,
            option: self.get_option_message(true).into(),
            session_id: self.session_id,
            version: crate::VERSION.to_string(),
            hwid,
            avatar,
            ..Default::default()
        };
        match self.conn_type {
            ConnType::FILE_TRANSFER => lr.set_file_transfer(FileTransfer {
                dir: self.get_remote_dir(),
                show_hidden: !self.get_option("remote_show_hidden").is_empty(),
                ..Default::default()
            }),
            _ => {}
        }

        let mut msg_out = Message::new();
        msg_out.set_login_request(lr);
        msg_out
    }

    pub fn update_supported_decodings(&self) -> Message {
        let decoding = scrap::codec::Decoder::supported_decodings(
            Some(&self.id),
            use_texture_render(),
            self.adapter_luid,
            &self.mark_unsupported,
        );
        let mut misc = Misc::new();
        misc.set_option(OptionMessage {
            supported_decoding: hbb_common::protobuf::MessageField::some(decoding),
            ..Default::default()
        });
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    pub fn restart_remote_device(&self) -> Message {
        let mut misc = Misc::new();
        misc.set_restart_remote_device(true);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    pub fn mark_restarting_remote_device(&mut self) {
        self.restarting_remote_device = true;
        self.restart_remote_device_at = Some(Instant::now());
    }

    pub fn clear_restarting_remote_device(&mut self) {
        self.restarting_remote_device = false;
        self.restart_remote_device_at = None;
    }

    pub fn is_restarting_remote_device(&self) -> bool {
        if !self.restarting_remote_device {
            return false;
        }
        // Keep this flag alive for a short grace window instead of clearing it on
        // connection_ready or the first peer bytes. During OS restart the peer can
        // briefly reconnect before the real reboot disconnect, and clearing it too
        // early would let the next disconnect escape the restart flow and fall back
        // to the normal error dialog / manual reconnect path.
        self.restart_remote_device_at
            .map(|started_at| started_at.elapsed() < RESTART_REMOTE_DEVICE_GRACE)
            .unwrap_or(false)
    }

    pub fn get_conn_token(&self) -> Option<String> {
        if self.password.is_empty() {
            return None;
        }
        serde_json::to_string(&ConnToken {
            password: self.password.clone(),
            password_source: self.password_source.clone(),
            session_id: self.session_id,
        })
        .ok()
    }

    pub fn get_id(&self) -> &str {
        &self.id
    }
}

/// Media data.
pub enum MediaData {
    VideoQueue,
    VideoFrame(Box<VideoFrame>),
    AudioFrame(Box<AudioFrame>),
    AudioFormat(AudioFormat),
    Reset,
}

pub type MediaSender = mpsc::Sender<MediaData>;

/// Start video thread.
///
/// # Arguments
///
/// * `video_callback` - The callback for video frame. Being called when a video frame is ready.
pub fn start_video_thread<F, T>(
    session: Session<T>,
    video_receiver: mpsc::Receiver<MediaData>,
    video_queue: Arc<RwLock<ArrayQueue<VideoFrame>>>,
    fps: Arc<RwLock<Option<usize>>>,
    chroma: Arc<RwLock<Option<Chroma>>>,
    discard_queue: Arc<RwLock<bool>>,
    video_callback: F,
) where
    F: 'static + FnMut(usize, &mut scrap::ImageRgb, *mut c_void, bool) + Send,
    T: InvokeUiSession,
{
    let mut video_callback = video_callback;
    let mut last_chroma = None;

    std::thread::spawn(move || {
        sync_cpu_usage();
        get_hwcodec_config();
        let mut video_handler = None;
        let mut count = 0;
        let mut duration = std::time::Duration::ZERO;
        let mut skip_beginning = 0;
        loop {
            if let Ok(data) = video_receiver.recv() {
                match data {
                    MediaData::VideoFrame(_) | MediaData::VideoQueue => {
                        let vf = match data {
                            MediaData::VideoFrame(vf) => {
                                *discard_queue.write().unwrap() = false;
                                *vf
                            }
                            MediaData::VideoQueue => {
                                if let Some(vf) = video_queue.read().unwrap().pop() {
                                    if discard_queue.read().unwrap().clone() {
                                        continue;
                                    }
                                    vf
                                } else {
                                    continue;
                                }
                            }
                            _ => {
                                // unreachable!();
                                continue;
                            }
                        };
                        let display = vf.display as usize;
                        let start = std::time::Instant::now();
                        let format = CodecFormat::from(&vf);
                        if video_handler.is_none() {
                            video_handler = Some(VideoHandler::new(format, display));
                        }
                        if let Some(handler) = video_handler.as_mut() {
                            let mut pixelbuffer = true;
                            let mut tmp_chroma = None;
                            let format_changed = handler.decoder.format() != format;
                            match handler.handle_frame(vf, &mut pixelbuffer, &mut tmp_chroma) {
                                Ok(true) => {
                                    video_callback(
                                        display,
                                        &mut handler.rgb,
                                        handler.texture.texture,
                                        pixelbuffer,
                                    );

                                    // chroma
                                    if tmp_chroma.is_some() && last_chroma != tmp_chroma {
                                        last_chroma = tmp_chroma;
                                        *chroma.write().unwrap() = tmp_chroma;
                                    }

                                    // fps calculation
                                    fps_calculate(
                                        &mut skip_beginning,
                                        &fps,
                                        format_changed,
                                        start.elapsed(),
                                        &mut count,
                                        &mut duration,
                                    );
                                }
                                Err(e) => {
                                    // This is a simple workaround.
                                    //
                                    // I only see the following error:
                                    // FailedCall("errcode=1 scrap::common::vpxcodec:libs\\scrap\\src\\common\\vpxcodec.rs:433:9")
                                    // When switching from all displays to one display, the error occurs.
                                    // eg:
                                    // 1. Connect to a device with two displays (A and B).
                                    // 2. Switch to display A. The error occurs.
                                    // 3. If the error does not occur. Switch from A to display B. The error occurs.
                                    //
                                    // to-do: fix the error
                                    log::error!("handle video frame error, {}", e);
                                    session.refresh_video(display as _);
                                }
                                _ => {}
                            }
                        }

                        // check invalid decoders
                        let mut should_update_supported = false;
                        if let Some(handler) = video_handler.as_mut() {
                            if !handler.decoder.valid()
                                || handler.fail_counter >= MAX_DECODE_FAIL_COUNTER
                            {
                                let mut lc = session.lc.write().unwrap();
                                let format = handler.decoder.format();
                                if !lc.mark_unsupported.contains(&format) {
                                    lc.mark_unsupported.push(format);
                                    should_update_supported = true;
                                    log::info!("mark {format:?} decoder as unsupported, valid:{}, fail_counter:{}, all unsupported:{:?}", handler.decoder.valid(), handler.fail_counter, lc.mark_unsupported);
                                }
                            }
                        }
                        if should_update_supported {
                            session.send(Data::Message(
                                session.lc.read().unwrap().update_supported_decodings(),
                            ));
                        }
                    }
                    MediaData::Reset => {
                        if let Some(handler) = video_handler.as_mut() {
                            handler.reset(None);
                        }
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
        log::info!("Video decoder loop exits");
    });
}

/// Start an audio thread
/// Return a audio [`MediaSender`]
pub fn start_audio_thread() -> MediaSender {
    let (audio_sender, audio_receiver) = mpsc::channel::<MediaData>();
    std::thread::spawn(move || {
        let mut audio_handler = AudioHandler::default();
        loop {
            let received = audio_handler.receive_audio(&audio_receiver);
            if let Ok(data) = received {
                match data {
                    MediaData::AudioFrame(af) => {
                        audio_handler.handle_frame(*af);
                    }
                    MediaData::AudioFormat(f) => {
                        log::debug!("recved audio format, sample rate={}", f.sample_rate);
                        audio_handler.handle_format(f);
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
        log::info!("Audio decoder loop exits");
    });
    audio_sender
}

#[inline]
fn fps_calculate(
    skip_beginning: &mut usize,
    fps: &Arc<RwLock<Option<usize>>>,
    format_changed: bool,
    elapsed: std::time::Duration,
    count: &mut usize,
    duration: &mut std::time::Duration,
) {
    if format_changed {
        *count = 0;
        *duration = std::time::Duration::ZERO;
        *skip_beginning = 0;
    }
    // // The first frame will be very slow
    if *skip_beginning < 3 {
        *skip_beginning += 1;
        return;
    }
    *duration += elapsed;
    *count += 1;
    let ms = duration.as_millis();
    if *count % 10 == 0 && ms > 0 {
        *fps.write().unwrap() = Some((*count as usize) * 1000 / (ms as usize));
    }
    // Clear to get real-time fps
    if *count >= 30 {
        *count = 0;
        *duration = Duration::ZERO;
    }
}

fn get_hwcodec_config() {
    // for unilink
    #[cfg(feature = "hwcodec")]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let start = std::time::Instant::now();
            if let Err(e) = crate::ipc::get_hwcodec_config_from_server() {
                log::error!(
                    "Failed to get hwcodec config: {e:?}, elapsed: {:?}",
                    start.elapsed()
                );
            } else {
                log::info!("{:?} used to get hwcodec config", start.elapsed());
            }
        });
    }
}

fn sync_cpu_usage() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let t = std::thread::spawn(do_sync_cpu_usage);
        t.join().ok();
    });
}

#[tokio::main(flavor = "current_thread")]
async fn do_sync_cpu_usage() {
    use crate::ipc::{connect, Data};
    let start = std::time::Instant::now();
    match connect(50, "").await {
        Ok(mut conn) => {
            if conn.send(&&Data::SyncWinCpuUsage(None)).await.is_ok() {
                if let Ok(Some(data)) = conn.next_timeout(50).await {
                    match data {
                        Data::SyncWinCpuUsage(cpu_usage) => {
                            base::platform::windows::sync_cpu_usage(cpu_usage);
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    log::info!("{:?} used to sync cpu usage", start.elapsed());
}

/// Handle latency test.
///
/// # Arguments
///
/// * `t` - The latency test message.
/// * `peer` - The peer.
pub async fn handle_test_delay(t: TestDelay, peer: &mut Stream) {
    if !t.from_client {
        let mut msg_out = Message::new();
        msg_out.set_test_delay(t);
        allow_err!(peer.send(&msg_out).await);
    }
}

/// Send mouse data.
///
/// # Arguments
///
/// * `mask` - Mouse event.
///     * mask = buttons << 3 | type
///     * type, 1: down, 2: up, 3: wheel, 4: trackpad
///     * buttons, 1: left, 2: right, 4: middle
/// * `x` - X coordinate.
/// * `y` - Y coordinate.
/// * `alt` - Whether the alt key is pressed.
/// * `ctrl` - Whether the ctrl key is pressed.
/// * `shift` - Whether the shift key is pressed.
/// * `command` - Whether the command key is pressed.
/// * `interface` - The interface for sending data.
#[inline]
pub fn send_mouse(
    mask: i32,
    x: i32,
    y: i32,
    alt: bool,
    ctrl: bool,
    shift: bool,
    command: bool,
    interface: &impl Interface,
) {
    let mut msg_out = Message::new();
    let mut mouse_event = MouseEvent {
        mask,
        x,
        y,
        ..Default::default()
    };
    if alt {
        mouse_event.modifiers.push(ControlKey::Alt.into());
    }
    if shift {
        mouse_event.modifiers.push(ControlKey::Shift.into());
    }
    if ctrl {
        mouse_event.modifiers.push(ControlKey::Control.into());
    }
    if command {
        mouse_event.modifiers.push(ControlKey::Meta.into());
    }
    interface.swap_modifier_mouse(&mut mouse_event);
    msg_out.set_mouse_event(mouse_event);
    interface.send(Data::Message(msg_out));
}

#[inline]
pub fn send_pointer_device_event(
    mut evt: PointerDeviceEvent,
    alt: bool,
    ctrl: bool,
    shift: bool,
    command: bool,
    interface: &impl Interface,
) {
    let mut msg_out = Message::new();
    if alt {
        evt.modifiers.push(ControlKey::Alt.into());
    }
    if shift {
        evt.modifiers.push(ControlKey::Shift.into());
    }
    if ctrl {
        evt.modifiers.push(ControlKey::Control.into());
    }
    if command {
        evt.modifiers.push(ControlKey::Meta.into());
    }
    msg_out.set_pointer_device_event(evt);
    interface.send(Data::Message(msg_out));
}

/// Activate OS by sending mouse movement.
///
/// # Arguments
///
/// * `interface` - The interface for sending data.
/// * `send_left_click` - Whether to send a click event.
fn activate_os(interface: &impl Interface, send_left_click: bool) {
    let left_down = MOUSE_BUTTON_LEFT << 3 | MOUSE_TYPE_DOWN;
    let left_up = MOUSE_BUTTON_LEFT << 3 | MOUSE_TYPE_UP;
    let right_down = MOUSE_BUTTON_RIGHT << 3 | MOUSE_TYPE_DOWN;
    let right_up = MOUSE_BUTTON_RIGHT << 3 | MOUSE_TYPE_UP;
    send_mouse(left_up, 0, 0, false, false, false, false, interface);
    std::thread::sleep(Duration::from_millis(50));
    send_mouse(0, 0, 0, false, false, false, false, interface);
    std::thread::sleep(Duration::from_millis(50));
    send_mouse(0, 3, 3, false, false, false, false, interface);
    let (click_down, click_up) = if send_left_click {
        (left_down, left_up)
    } else {
        (right_down, right_up)
    };
    std::thread::sleep(Duration::from_millis(50));
    send_mouse(click_down, 0, 0, false, false, false, false, interface);
    send_mouse(click_up, 0, 0, false, false, false, false, interface);
    /*
    let mut key_event = KeyEvent::new();
    // do not use Esc, which has problem with Linux
    key_event.set_control_key(ControlKey::RightArrow);
    key_event.press = true;
    let mut msg_out = Message::new();
    msg_out.set_key_event(key_event.clone());
    interface.send(Data::Message(msg_out.clone()));
    */
}

/// Input the OS's password.
///
/// # Arguments
///
/// * `p` - The password.
/// * `activate` - Whether to activate OS.
/// * `interface` - The interface for sending data.
pub fn input_os_password(p: String, activate: bool, interface: impl Interface) {
    std::thread::spawn(move || {
        _input_os_password(p, activate, interface);
    });
}

/// Input the OS's password.
///
/// # Arguments
///
/// * `p` - The password.
/// * `activate` - Whether to activate OS.
/// * `interface` - The interface for sending data.
fn _input_os_password(p: String, activate: bool, interface: impl Interface) {
    let input_password = !p.is_empty();
    if activate {
        // Click event is used to bring up the password input box.
        activate_os(&interface, input_password);
        std::thread::sleep(Duration::from_millis(1200));
    }
    if !input_password {
        return;
    }
    let mut key_event = KeyEvent::new();
    key_event.mode = KeyboardMode::Legacy.into();
    key_event.press = true;
    let mut msg_out = Message::new();
    key_event.set_seq(p);
    msg_out.set_key_event(key_event.clone());
    interface.send(Data::Message(msg_out.clone()));
    key_event.set_control_key(ControlKey::Return);
    msg_out.set_key_event(key_event);
    interface.send(Data::Message(msg_out));
}

/// Auto Login. A peer that can check for its logon or lock screen gets the password and types it
/// itself, only if it is there right then. Keys typed from here land in whatever has focus if the
/// screen is not locked by the time they arrive.
pub fn auto_login(p: String, pi: &PeerInfo, interface: impl Interface) {
    if !is_peer_handling_os_login(pi) {
        input_os_password(p, true, interface);
        return;
    }
    std::thread::spawn(move || {
        activate_os(&interface, true);
        std::thread::sleep(Duration::from_millis(1200));
        let mut msg_out = Message::new();
        msg_out.set_os_login(OSLogin {
            password: p,
            ..Default::default()
        });
        interface.send(Data::Message(msg_out));
    });
}

/// Windows and macOS peers from the 1.5.1 release handle `os_login`.
fn is_peer_handling_os_login(pi: &PeerInfo) -> bool {
    (pi.platform == crate::common::PLATFORM_WINDOWS || pi.platform == crate::common::PLATFORM_MACOS)
        && hbb_common::get_version_number(&pi.version) >= hbb_common::get_version_number("1.5.1")
}

#[derive(Copy, Clone)]
struct LoginErrorMsgBox {
    msgtype: &'static str,
    title: &'static str,
    text: &'static str,
    link: &'static str,
    try_again: bool,
}

lazy_static::lazy_static! {
    static ref LOGIN_ERROR_MAP: Arc<HashMap<&'static str, LoginErrorMsgBox>> = {
        let map = HashMap::from([(LOGIN_SCREEN_WAYLAND, LoginErrorMsgBox{
            msgtype: "error",
            title: "Login Error",
            text: "Login screen using Wayland is not supported",
            link: "https://rustdesk.com/docs/en/manual/linux/#login-screen",
            try_again: true,
        }), (LOGIN_MSG_NO_PASSWORD_ACCESS, LoginErrorMsgBox{
            msgtype: "wait-remote-accept-nook",
            title: "Prompt",
            text: "Please wait for the remote side to accept your session request...",
            link: "",
            try_again: true,
        })]);
        Arc::new(map)
    };
}

/// Handle login error.
/// Return true if the password is wrong, return false if there's an actual error.
pub fn handle_login_error(
    lc: Arc<RwLock<LoginConfigHandler>>,
    err: &str,
    interface: &impl Interface,
) -> bool {
    if err == LOGIN_MSG_PASSWORD_EMPTY {
        lc.write().unwrap().password = Default::default();
        interface.msgbox("input-password", "Password Required", "", "");
        true
    } else if err == LOGIN_MSG_PASSWORD_WRONG {
        lc.write().unwrap().password = Default::default();
        interface.msgbox("re-input-password", err, "Do you want to enter again?", "");
        true
    } else if err == LOGIN_MSG_2FA_WRONG || err == REQUIRE_2FA {
        let enabled = lc.read().unwrap().get_option("trust-this-device") == "Y";
        if enabled {
            lc.write()
                .unwrap()
                .set_option("trust-this-device".to_string(), "".to_string());
        }
        interface.msgbox("input-2fa", err, "", "");
        true
    } else if LOGIN_ERROR_MAP.contains_key(err) {
        if let Some(msgbox_info) = LOGIN_ERROR_MAP.get(err) {
            interface.msgbox(
                msgbox_info.msgtype,
                msgbox_info.title,
                msgbox_info.text,
                msgbox_info.link,
            );
            msgbox_info.try_again
        } else {
            // unreachable!
            false
        }
    } else {
        if err.contains(SCRAP_X11_REQUIRED) {
            interface.msgbox("error", "Login Error", err, SCRAP_X11_REF_URL);
        } else {
            interface.msgbox("error", "Login Error", err, "");
        }
        false
    }
}

// "Switch sides" requires the incoming-only client to connect back to its
// controlling peer; verify the local pending uuid before opening the connection.
/// Addresses to try for `peer`: the address itself, or the IPs LAN discovery saw for that ID.
fn peer_addrs(peer: &str) -> Vec<String> {
    if hbb_common::is_ip_str(peer) || hbb_common::is_domain_port_str(peer) {
        return vec![check_port(peer, RELAY_PORT + 1)];
    }
    config::LanPeers::load()
        .peers
        .into_iter()
        .filter(|p| p.id == peer)
        .flat_map(|p| p.ip_mac.into_keys())
        .map(|ip| check_port(ip, RELAY_PORT + 1))
        .collect()
}

async fn is_switch_sides_back(conn_type: ConnType, interface: &impl Interface) -> bool {
    if conn_type != ConnType::DEFAULT_CONN {
        return false;
    }
    let (id, uuid) = {
        let lch = interface.get_lch();
        let lc = lch.read().unwrap();
        let Some(uuid) = lc.switch_uuid.as_deref() else {
            return false;
        };
        let Ok(uuid) = Uuid::parse_str(uuid) else {
            return false;
        };
        (lc.id.clone(), uuid)
    };
    if !request_local_switch_sides_uuid(
        &id,
        &uuid,
        crate::ipc::SwitchSidesUuidAction::Check,
    )
    .await
    {
        return false;
    }
    let lch = interface.get_lch();
    let lc = lch.read().unwrap();
    let current_uuid = lc
        .switch_uuid
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok());
    lc.id == id && current_uuid.as_ref() == Some(&uuid)
}


async fn request_local_switch_sides_uuid(
    id: &str,
    uuid: &Uuid,
    action: crate::ipc::SwitchSidesUuidAction,
) -> bool {
    let Ok(mut conn) = crate::ipc::connect(1000, "").await else {
        return false;
    };
    let uuid = uuid.to_string();
    if conn
        .send(&crate::ipc::Data::SwitchSidesUuid(
            uuid.clone(),
            id.to_owned(),
            action,
            None,
        ))
        .await
        .is_err()
    {
        return false;
    }
    match conn.next_timeout(1000).await {
        Ok(Some(crate::ipc::Data::SwitchSidesUuid(
            returned_uuid,
            returned_id,
            returned_action,
            Some(true),
        ))) => {
            returned_uuid == uuid && returned_id == id && returned_action == action
        }
        _ => false,
    }
}

/// Handle hash message sent by peer.
/// Hash will be used for login.
///
/// # Arguments
///
/// * `lc` - Login config.
/// * `hash` - Hash sent by peer.
/// * `interface` - [`Interface`] for sending data.
/// * `peer` - [`Stream`] for communicating with peer.
pub async fn handle_hash(
    lc: Arc<RwLock<LoginConfigHandler>>,
    password_preset: &str,
    hash: Hash,
    interface: &impl Interface,
    peer: &mut Stream,
) -> bool {
    lc.write().unwrap().hash = hash.clone();
    // Take care of password application order

    // switch_uuid
    {
        let uuid = lc.write().unwrap().switch_uuid.take();
        if let Some(uuid) = uuid {
            if let Ok(uuid) = uuid::Uuid::from_str(&uuid) {
                let id = lc.read().unwrap().id.clone();
                if !request_local_switch_sides_uuid(
                    &id,
                    &uuid,
                    crate::ipc::SwitchSidesUuidAction::Consume,
                )
                .await
                {
                    log::warn!("Ignored untrusted switch_uuid");
                } else {
                    lc.write().unwrap().allow_switch_back_once();
                    send_switch_login_request(lc.clone(), peer, uuid).await;
                    lc.write().unwrap().password_source = Default::default();
                    return true;
                }
            }
        }
        // Incoming-only may connect out solely for a verified switch-back;
        // never fall through to password login, including on repeated hashes.
        if config::is_incoming_only() {
            interface.msgbox("error", "Connection Error", "Incoming only mode", "");
            let mut misc = Misc::new();
            misc.set_close_reason(
                "Connection not allowed in incoming-only mode".to_owned(),
            );
            let mut msg = Message::new();
            msg.set_misc(misc);
            allow_err!(peer.send(&msg).await);
            return false;
        }
    }
    // last password
    let mut password = lc.read().unwrap().password.clone();
    // preset password
    if password.is_empty() {
        if !password_preset.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(password_preset);
            hasher.update(&hash.salt);
            let res = hasher.finalize();
            password = res[..].into();
            lc.write().unwrap().password_source = Default::default();
        }
    }
    // peer config password
    if password.is_empty() {
        password = lc.read().unwrap().config.password.clone();
        if !password.is_empty() {
            lc.write().unwrap().password_source = Default::default();
        }
    }
    if password.is_empty() {
        let p = crate::ui_interface::get_builtin_option(keys::OPTION_DEFAULT_CONNECT_PASSWORD);
        if !p.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(p.clone());
            hasher.update(&hash.salt);
            let res = hasher.finalize();
            password = res[..].into();
            lc.write().unwrap().password_source = PasswordSource::SharedAb(p); // reuse SharedAb here
        }
    }

    lc.write().unwrap().password = password.clone();

    let password = if password.is_empty() {
        // login without password, the remote side can click accept
        interface.msgbox("input-password", "Password Required", "", "");
        Vec::new()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(&password);
        hasher.update(&hash.challenge);
        hasher.finalize()[..].into()
    };

    send_login(lc.clone(), password, peer).await;
    lc.write().unwrap().hash = hash;
    true
}

/// Send login message to peer.
///
/// # Arguments
///
/// * `lc` - Login config.
/// * `password` - Password.
/// * `peer` - [`Stream`] for communicating with peer.
async fn send_login(lc: Arc<RwLock<LoginConfigHandler>>, password: Vec<u8>, peer: &mut Stream) {
    let msg_out = lc.read().unwrap().create_login_msg(password);
    allow_err!(peer.send(&msg_out).await);
}

/// Handle login request made from ui.
///
/// # Arguments
///
/// * `lc` - Login config.
/// * `password` - Password.
/// * `remember` - Whether to remember password.
/// * `peer` - [`Stream`] for communicating with peer.
pub async fn handle_login_from_ui(
    lc: Arc<RwLock<LoginConfigHandler>>,
    password: String,
    remember: bool,
    peer: &mut Stream,
) {
    let mut hash_password = if password.is_empty() {
        let mut password2 = lc.read().unwrap().password.clone();
        if password2.is_empty() {
            password2 = lc.read().unwrap().config.password.clone();
            if !password2.is_empty() {
                lc.write().unwrap().password_source = Default::default();
            }
        }
        password2
    } else {
        lc.write().unwrap().password_source = Default::default();
        let mut hasher = Sha256::new();
        hasher.update(password);
        hasher.update(&lc.read().unwrap().hash.salt);
        let res = hasher.finalize();
        lc.write().unwrap().remember = remember;
        res[..].into()
    };
    lc.write().unwrap().password = hash_password.clone();
    let mut hasher2 = Sha256::new();
    hasher2.update(&hash_password[..]);
    hasher2.update(&lc.read().unwrap().hash.challenge);
    hash_password = hasher2.finalize()[..].to_vec();

    send_login(lc.clone(), hash_password, peer).await;
}

async fn send_switch_login_request(
    lc: Arc<RwLock<LoginConfigHandler>>,
    peer: &mut Stream,
    uuid: Uuid,
) {
    let mut msg_out = Message::new();
    msg_out.set_switch_sides_response(SwitchSidesResponse {
        uuid: Bytes::from(uuid.as_bytes().to_vec()),
        lr: hbb_common::protobuf::MessageField::some(
            lc.read()
                .unwrap()
                .create_login_msg(vec![])
                .login_request()
                .to_owned(),
        ),
        ..Default::default()
    });
    allow_err!(peer.send(&msg_out).await);
}

/// Interface for client to send data and commands.
#[async_trait]
pub trait Interface: Send + Clone + 'static + Sized {
    /// Send message data to remote peer.
    fn send(&self, data: Data);
    fn msgbox(&self, msgtype: &str, title: &str, text: &str, link: &str);
    fn handle_login_error(&self, err: &str) -> bool;
    fn handle_peer_info(&self, pi: PeerInfo);
    fn set_multiple_windows_session(&self, sessions: Vec<WindowsSession>);
    fn on_error(&self, err: &str) {
        self.msgbox("error", "Error", err, "");
    }
    async fn handle_hash(&self, pass: &str, hash: Hash, peer: &mut Stream) -> bool;
    async fn handle_login_from_ui(
        &self,
        password: String,
        remember: bool,
        peer: &mut Stream,
    );
    async fn handle_test_delay(&self, t: TestDelay, peer: &mut Stream);

    fn get_lch(&self) -> Arc<RwLock<LoginConfigHandler>>;

    fn get_id(&self) -> String {
        self.get_lch().read().unwrap().id.clone()
    }

    fn get_switch_code(&self) -> String {
        match self.get_lch().read().unwrap().switch_uuid.clone() {
            Some(u) if !u.is_empty() => {
                use hbb_common::sodiumoxide::crypto::hash::sha256;
                crate::encode64(sha256::hash(u.as_bytes()).0)
            }
            _ => String::new(),
        }
    }

    fn swap_modifier_mouse(&self, _msg: &mut base::protos::message::MouseEvent) {}

    fn on_establish_connection_error(&self, err: String) {
        let title = "Connection Error";
        let text = err.to_string();
        let lch = self.get_lch();
        let is_restarting = lch.read().unwrap().is_restarting_remote_device();
        if is_restarting {
            log::info!("Restart remote device, suppress connection error: {err}");
            // Flutter treats this as a reconnect control event. The text is kept
            // for legacy UI and existing translation reuse.
            self.msgbox("restarting", "Restarting remote device", "Connection in progress. Please wait.", "");
            return;
        }

        log::error!("Connection closed: {err}({})", errno::errno().0);
        self.msgbox("error", title, &text, "");
    }
}

/// Data used by the client interface.
#[derive(Clone)]
pub enum Data {
    Close,
    RejectInsecureConnection,
    Login((String, bool)),
    Message(Message),
    InitialClipboard(usize, Option<Message>),
    SendFiles((i32, JobType, String, String, i32, bool, bool)),
    RemoveDirAll((i32, String, bool, bool)),
    RemoveDir((i32, String)),
    RemoveFile((i32, String, i32, bool)),
    CreateDir((i32, String, bool)),
    CancelJob(i32),
    SetConfirmOverrideFile((i32, i32, bool, bool, bool)),
    AddJob((i32, JobType, String, String, i32, bool, bool)),
    ResumeJob((i32, bool)),
    ElevateDirect,
    ElevateWithLogon(String, String),
    ContinueInsecureConnection,
    ResetDecoder(Option<usize>),
    RenameFile((i32, String, String, bool)),
    TakeScreenshot((i32, String)),
}

pub async fn confirm_insecure_connection(
    interface: &impl Interface,
    receiver: &mut UnboundedReceiver<Data>,
) -> bool {
    interface.msgbox(
        "insecure-connection-nocancel-hasclose",
        "Insecure Connection",
        "conn-e2ee-unavailable-tip",
        "",
    );
    while let Some(data) = receiver.recv().await {
        match data {
            Data::ContinueInsecureConnection => return true,
            Data::RejectInsecureConnection => return false,
            Data::Close => return false,
            _ => {}
        }
    }
    false
}

/// Keycode for key events.
#[derive(Clone, Debug)]
pub enum Key {
    ControlKey(ControlKey),
    Chr(u32),
    _Raw(u32),
}

lazy_static::lazy_static! {
    pub static ref KEY_MAP: HashMap<&'static str, Key> =
    [
        ("VK_A", Key::Chr('a' as _)),
        ("VK_B", Key::Chr('b' as _)),
        ("VK_C", Key::Chr('c' as _)),
        ("VK_D", Key::Chr('d' as _)),
        ("VK_E", Key::Chr('e' as _)),
        ("VK_F", Key::Chr('f' as _)),
        ("VK_G", Key::Chr('g' as _)),
        ("VK_H", Key::Chr('h' as _)),
        ("VK_I", Key::Chr('i' as _)),
        ("VK_J", Key::Chr('j' as _)),
        ("VK_K", Key::Chr('k' as _)),
        ("VK_L", Key::Chr('l' as _)),
        ("VK_M", Key::Chr('m' as _)),
        ("VK_N", Key::Chr('n' as _)),
        ("VK_O", Key::Chr('o' as _)),
        ("VK_P", Key::Chr('p' as _)),
        ("VK_Q", Key::Chr('q' as _)),
        ("VK_R", Key::Chr('r' as _)),
        ("VK_S", Key::Chr('s' as _)),
        ("VK_T", Key::Chr('t' as _)),
        ("VK_U", Key::Chr('u' as _)),
        ("VK_V", Key::Chr('v' as _)),
        ("VK_W", Key::Chr('w' as _)),
        ("VK_X", Key::Chr('x' as _)),
        ("VK_Y", Key::Chr('y' as _)),
        ("VK_Z", Key::Chr('z' as _)),
        ("VK_0", Key::Chr('0' as _)),
        ("VK_1", Key::Chr('1' as _)),
        ("VK_2", Key::Chr('2' as _)),
        ("VK_3", Key::Chr('3' as _)),
        ("VK_4", Key::Chr('4' as _)),
        ("VK_5", Key::Chr('5' as _)),
        ("VK_6", Key::Chr('6' as _)),
        ("VK_7", Key::Chr('7' as _)),
        ("VK_8", Key::Chr('8' as _)),
        ("VK_9", Key::Chr('9' as _)),
        ("VK_COMMA", Key::Chr(',' as _)),
        ("VK_SLASH", Key::Chr('/' as _)),
        ("VK_SEMICOLON", Key::Chr(';' as _)),
        ("VK_QUOTE", Key::Chr('\'' as _)),
        ("VK_LBRACKET", Key::Chr('[' as _)),
        ("VK_RBRACKET", Key::Chr(']' as _)),
        ("VK_BACKSLASH", Key::Chr('\\' as _)),
        ("VK_MINUS", Key::Chr('-' as _)),
        ("VK_PLUS", Key::Chr('=' as _)), // it is =, but sciter return VK_PLUS
        ("VK_DIVIDE", Key::ControlKey(ControlKey::Divide)), // numpad
        ("VK_MULTIPLY", Key::ControlKey(ControlKey::Multiply)), // numpad
        ("VK_SUBTRACT", Key::ControlKey(ControlKey::Subtract)), // numpad
        ("VK_ADD", Key::ControlKey(ControlKey::Add)), // numpad
        ("VK_DECIMAL", Key::ControlKey(ControlKey::Decimal)), // numpad
        ("VK_F1", Key::ControlKey(ControlKey::F1)),
        ("VK_F2", Key::ControlKey(ControlKey::F2)),
        ("VK_F3", Key::ControlKey(ControlKey::F3)),
        ("VK_F4", Key::ControlKey(ControlKey::F4)),
        ("VK_F5", Key::ControlKey(ControlKey::F5)),
        ("VK_F6", Key::ControlKey(ControlKey::F6)),
        ("VK_F7", Key::ControlKey(ControlKey::F7)),
        ("VK_F8", Key::ControlKey(ControlKey::F8)),
        ("VK_F9", Key::ControlKey(ControlKey::F9)),
        ("VK_F10", Key::ControlKey(ControlKey::F10)),
        ("VK_F11", Key::ControlKey(ControlKey::F11)),
        ("VK_F12", Key::ControlKey(ControlKey::F12)),
        ("VK_ENTER", Key::ControlKey(ControlKey::Return)),
        ("VK_CANCEL", Key::ControlKey(ControlKey::Cancel)),
        ("VK_BACK", Key::ControlKey(ControlKey::Backspace)),
        ("VK_TAB", Key::ControlKey(ControlKey::Tab)),
        ("VK_CLEAR", Key::ControlKey(ControlKey::Clear)),
        ("VK_RETURN", Key::ControlKey(ControlKey::Return)),
        ("VK_SHIFT", Key::ControlKey(ControlKey::Shift)),
        ("VK_CONTROL", Key::ControlKey(ControlKey::Control)),
        ("VK_MENU", Key::ControlKey(ControlKey::Alt)),
        ("VK_PAUSE", Key::ControlKey(ControlKey::Pause)),
        ("VK_CAPITAL", Key::ControlKey(ControlKey::CapsLock)),
        ("VK_KANA", Key::ControlKey(ControlKey::Kana)),
        ("VK_HANGUL", Key::ControlKey(ControlKey::Hangul)),
        ("VK_JUNJA", Key::ControlKey(ControlKey::Junja)),
        ("VK_FINAL", Key::ControlKey(ControlKey::Final)),
        ("VK_HANJA", Key::ControlKey(ControlKey::Hanja)),
        ("VK_KANJI", Key::ControlKey(ControlKey::Kanji)),
        ("VK_ESCAPE", Key::ControlKey(ControlKey::Escape)),
        ("VK_CONVERT", Key::ControlKey(ControlKey::Convert)),
        ("VK_SPACE", Key::ControlKey(ControlKey::Space)),
        ("VK_PRIOR", Key::ControlKey(ControlKey::PageUp)),
        ("VK_NEXT", Key::ControlKey(ControlKey::PageDown)),
        ("VK_END", Key::ControlKey(ControlKey::End)),
        ("VK_HOME", Key::ControlKey(ControlKey::Home)),
        ("VK_LEFT", Key::ControlKey(ControlKey::LeftArrow)),
        ("VK_UP", Key::ControlKey(ControlKey::UpArrow)),
        ("VK_RIGHT", Key::ControlKey(ControlKey::RightArrow)),
        ("VK_DOWN", Key::ControlKey(ControlKey::DownArrow)),
        ("VK_SELECT", Key::ControlKey(ControlKey::Select)),
        ("VK_PRINT", Key::ControlKey(ControlKey::Print)),
        ("VK_EXECUTE", Key::ControlKey(ControlKey::Execute)),
        ("VK_SNAPSHOT", Key::ControlKey(ControlKey::Snapshot)),
        ("VK_SCROLL", Key::ControlKey(ControlKey::Scroll)),
        ("VK_INSERT", Key::ControlKey(ControlKey::Insert)),
        ("VK_DELETE", Key::ControlKey(ControlKey::Delete)),
        ("VK_HELP", Key::ControlKey(ControlKey::Help)),
        ("VK_SLEEP", Key::ControlKey(ControlKey::Sleep)),
        ("VK_SEPARATOR", Key::ControlKey(ControlKey::Separator)),
        ("VK_NUMPAD0", Key::ControlKey(ControlKey::Numpad0)),
        ("VK_NUMPAD1", Key::ControlKey(ControlKey::Numpad1)),
        ("VK_NUMPAD2", Key::ControlKey(ControlKey::Numpad2)),
        ("VK_NUMPAD3", Key::ControlKey(ControlKey::Numpad3)),
        ("VK_NUMPAD4", Key::ControlKey(ControlKey::Numpad4)),
        ("VK_NUMPAD5", Key::ControlKey(ControlKey::Numpad5)),
        ("VK_NUMPAD6", Key::ControlKey(ControlKey::Numpad6)),
        ("VK_NUMPAD7", Key::ControlKey(ControlKey::Numpad7)),
        ("VK_NUMPAD8", Key::ControlKey(ControlKey::Numpad8)),
        ("VK_NUMPAD9", Key::ControlKey(ControlKey::Numpad9)),
        ("Apps", Key::ControlKey(ControlKey::Apps)),
        ("Meta", Key::ControlKey(ControlKey::Meta)),
        ("RAlt", Key::ControlKey(ControlKey::RAlt)),
        ("RWin", Key::ControlKey(ControlKey::RWin)),
        ("RControl", Key::ControlKey(ControlKey::RControl)),
        ("RShift", Key::ControlKey(ControlKey::RShift)),
        ("CTRL_ALT_DEL", Key::ControlKey(ControlKey::CtrlAltDel)),
        ("LOCK_SCREEN", Key::ControlKey(ControlKey::LockScreen)),
    ].iter().cloned().collect();
}

/// Check if the given message is an error and can be retried.
///
/// # Arguments
///
/// * `msgtype` - The message type.
/// * `title` - The title of the message.
/// * `text` - The text of the message.
#[inline]
pub fn check_if_retry(msgtype: &str, title: &str, text: &str) -> bool {
    msgtype == "error"
        && title == "Connection Error"
        && !text.to_lowercase().contains("offline")
        && !text.to_lowercase().contains("not exist")
        && !text.to_lowercase().contains("handshake")
        && !text.to_lowercase().contains("failed")
        && !text.to_lowercase().contains("resolve")
        && !text.to_lowercase().contains("mismatch")
        && !text.to_lowercase().contains("manually")
        && !text.to_lowercase().contains("restricted")
        && !text.to_lowercase().contains("incoming only")
        && !text.to_lowercase().contains("not allowed")
}

#[cfg(test)]
mod peer_addrs_tests {
    use super::peer_addrs;

    #[test]
    fn an_ip_is_dialed_on_the_direct_port_unless_one_is_given() {
        assert_eq!(peer_addrs("192.168.0.10"), vec!["192.168.0.10:21118"]);
        assert_eq!(peer_addrs("192.168.0.10:5000"), vec!["192.168.0.10:5000"]);
    }

    #[test]
    fn an_id_no_lan_peer_answered_for_has_nothing_to_dial() {
        assert!(peer_addrs("intradesk-test-unknown-peer").is_empty());
    }
}

#[cfg(test)]
mod retry_tests {
    use super::check_if_retry;

    #[test]
    fn incoming_only_error_is_not_retryable() {
        assert!(!check_if_retry(
            "error",
            "Connection Error",
            "Incoming only mode",
        ));
    }
}

#[cfg(test)]
mod kx_tests {
    use super::*;
    use hbb_common::{
        socket_client::connect_tcp,
        sodiumoxide::crypto::box_,
        tcp::{Encrypt, FramedStream, KxTranscript, KX_VERSION_LATEST},
        tokio::sync::oneshot,
    };

    const PEER_ID: &str = "123456789";

    /// What the stand-in controlled peer saw: the version the controller picked, and whether the
    /// controller's first application message decrypted.
    struct Seen {
        picked: u32,
        decrypted: bool,
    }

    fn test_delay(time: i64) -> Message {
        let mut msg = Message::new();
        msg.set_test_delay(TestDelay {
            time,
            ..Default::default()
        });
        msg
    }

    fn delay_in(bytes: &[u8]) -> Option<i64> {
        match Message::parse_from_bytes(bytes).ok()?.union? {
            message::Union::TestDelay(t) => Some(t.time),
            _ => None,
        }
    }

    /// A stand-in controlled peer on loopback. It advertises `advertised` in its signed identity
    /// and runs the key under `run`, or under the version the controller picked when `run` is
    /// `None`, then sends one message and reads one. Returns the host, its identity as the
    /// rendezvous server would sign it, and what it saw.
    async fn controlled_stub(
        advertised: u32,
        run: Option<u32>,
        rs_sk: sign::SecretKey,
    ) -> (String, Vec<u8>, oneshot::Receiver<Seen>) {
        let (sign_pk, sign_sk) = sign::gen_keypair();
        let id_pk = |pk: &[u8], kx_version| {
            IdPk {
                id: PEER_ID.to_owned(),
                pk: pk.to_vec().into(),
                kx_version,
                ..Default::default()
            }
            .write_to_bytes()
            .unwrap()
        };
        let signed_id_pk = sign::sign(&id_pk(&sign_pk.0, 0), &rs_sk);
        let listener = hbb_common::tcp::new_listener("127.0.0.1:0", false)
            .await
            .unwrap();
        let host = listener.local_addr().unwrap().to_string();
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let (stream, addr) = listener.accept().await.unwrap();
            let mut s = FramedStream::from(stream, addr);
            let (our_pk_b, our_sk_b) = box_::gen_keypair();
            let mut msg = Message::new();
            msg.set_signed_id(SignedId {
                id: sign::sign(&id_pk(&our_pk_b.0, advertised), &sign_sk).into(),
                ..Default::default()
            });
            s.send(&msg).await.unwrap();
            let bytes = s.next().await.unwrap().unwrap();
            let Some(message::Union::PublicKey(pk)) =
                Message::parse_from_bytes(&bytes).unwrap().union
            else {
                panic!("expected the controller's public key");
            };
            let key =
                Encrypt::decode(&pk.symmetric_value, &pk.asymmetric_value, &our_sk_b).unwrap();
            let t = KxTranscript {
                initiator_pk: &pk.asymmetric_value,
                responder_pk: &our_pk_b.0,
                advertised,
                picked: run.unwrap_or(pk.kx_version),
            };
            if t.picked == 0 {
                s.set_key(key);
            } else {
                s.set_key_split(key, false, &t).unwrap();
            }
            s.send(&test_delay(2)).await.unwrap();
            let decrypted = matches!(s.next().await, Some(Ok(b)) if delay_in(&b) == Some(1));
            tx.send(Seen {
                picked: pk.kx_version,
                decrypted,
            })
            .ok();
        });
        (host, signed_id_pk, rx)
    }

    /// The controller's handshake against the stub, then one application message each way.
    /// Returns what the stub saw and whether the stub's message decrypted on this side.
    async fn handshake(advertised: u32, run: Option<u32>) -> (Seen, bool) {
        let (rs_pk, rs_sk) = sign::gen_keypair();
        let (host, signed_id_pk, seen) = controlled_stub(advertised, run, rs_sk).await;
        let mut conn = connect_tcp(host, 3000).await.unwrap();
        let pk =
            Client::secure_connection(PEER_ID, signed_id_pk, &crate::encode64(rs_pk.0), &mut conn)
                .await
                .unwrap();
        assert!(pk.is_some() && conn.is_secured());
        conn.send(&test_delay(1)).await.unwrap();
        let decrypted = matches!(conn.next().await, Some(Ok(b)) if delay_in(&b) == Some(2));
        (seen.await.unwrap(), decrypted)
    }

    #[tokio::test]
    async fn test_new_peers_pick_version_1_and_exchange_application_data() {
        let (seen, decrypted) = handshake(KX_VERSION_LATEST, None).await;
        assert_eq!(seen.picked, 1);
        assert!(seen.decrypted && decrypted);
    }

    #[tokio::test]
    async fn test_a_peer_without_versions_gets_version_0() {
        let (seen, decrypted) = handshake(0, Some(0)).await;
        assert_eq!(seen.picked, 0);
        assert!(seen.decrypted && decrypted);
    }

    #[tokio::test]
    async fn test_a_pick_lowered_in_transit_decrypts_nothing() {
        // The controlled side takes 0 from a controller without versions, so a lowered pick is
        // caught by the keys disagreeing on the first application message, not before.
        let (seen, decrypted) = handshake(KX_VERSION_LATEST, Some(0)).await;
        assert_eq!(seen.picked, 1);
        assert!(!seen.decrypted && !decrypted);
    }
}
