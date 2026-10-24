// 24FPS (actually 23.976FPS) is what video professionals ages ago determined to be the
// slowest playback rate that still looks smooth enough to feel real.
// Our eyes can see a slight difference and even though 30FPS actually shows
// more information and is more realistic.
// 60FPS is commonly used in game, teamviewer 12 support this for video editing user.

// how to capture with mouse cursor:
// https://docs.microsoft.com/zh-cn/windows/win32/direct3ddxgi/desktop-dup-api?redirectedfrom=MSDN

// RECORD: The following Project has implemented audio capture, hardware codec and mouse cursor drawn.
// https://github.com/PHZ76/DesktopSharing

// dxgi memory leak issue
// https://stackoverflow.com/questions/47801238/memory-leak-in-creating-direct2d-device
// but per my test, it is more related to AcquireNextFrame,
// https://forums.developer.nvidia.com/t/dxgi-outputduplication-memory-leak-when-using-nv-but-not-amd-drivers/108582

// to-do:
// https://slhck.info/video/2017/03/01/rate-control.html

use super::{display_service::check_display_changed, service::ServiceTmpl, video_qos::VideoQoS, *};
use crate::platform::windows::is_process_consent_running;
use hbb_common::{
    tokio::sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        Mutex as TokioMutex,
    },
};
#[cfg(feature = "hwcodec")]
use scrap::hwcodec::{HwRamEncoder, HwRamEncoderConfig};
#[cfg(feature = "vram")]
use scrap::vram::{VRamEncoder, VRamEncoderConfig};
use scrap::{
    aom::AomEncoderConfig,
    codec::{Encoder, EncoderCfg},
    vpxcodec::{VpxEncoderConfig, VpxVideoCodecId},
    CodecFormat, Display, EncodeInput, TraitCapturer,
};
use std::io::ErrorKind::ConnectionReset;
use std::sync::Once;
use std::{
    collections::HashSet,
    io::ErrorKind::WouldBlock,
    ops::{Deref, DerefMut},
    time::{self, Duration, Instant},
};

pub const OPTION_REFRESH: &'static str = "refresh";

mod static_refresh;

const DXGI_RECOVERY_LIMIT: usize = 3;
const DXGI_RECOVERY_WINDOW: Duration = Duration::from_secs(10);
const DXGI_RECOVERY_FRAME_GRACE: Duration = Duration::from_secs(2);

struct DxgiRecoveryState {
    attempts: usize,
    window_started: Option<Instant>,
    restart_pending: bool,
    fallback_pending: bool,
}

impl DxgiRecoveryState {
    fn new() -> Self {
        Self {
            attempts: 0,
            window_started: None,
            restart_pending: false,
            fallback_pending: false,
        }
    }

    fn next_attempt(&mut self) -> Option<usize> {
        if self
            .window_started
            .map(|started| started.elapsed() > DXGI_RECOVERY_WINDOW)
            .unwrap_or(true)
        {
            self.attempts = 0;
            self.window_started = Some(Instant::now());
        }
        if self.attempts >= DXGI_RECOVERY_LIMIT {
            self.fallback_pending = true;
            return None;
        }
        self.attempts += 1;
        self.restart_pending = true;
        Some(self.attempts)
    }

    fn take_restart_pending(&mut self) -> bool {
        std::mem::take(&mut self.restart_pending)
    }

    fn take_fallback_pending(&mut self) -> bool {
        std::mem::take(&mut self.fallback_pending)
    }
}

type FrameFetchedNotifierSender = UnboundedSender<(i32, Option<Instant>)>;
type FrameFetchedNotifierReceiver = Arc<TokioMutex<UnboundedReceiver<(i32, Option<Instant>)>>>;

lazy_static::lazy_static! {
    static ref FRAME_FETCHED_NOTIFIERS: Mutex<HashMap<usize, (FrameFetchedNotifierSender, FrameFetchedNotifierReceiver)>> = Mutex::new(HashMap::default());

    // display_idx -> set of conn id.
    // Used to record which connections need to be notified when
    // 1. A new frame is received from a web client.
    //   Because web client does not send the display index in message `VideoReceived`.
    // 2. The client is closing.
    static ref DISPLAY_CONN_IDS: Arc<Mutex<HashMap<usize, HashSet<i32>>>> = Default::default();
    pub static ref VIDEO_QOS: Arc<Mutex<VideoQoS>> = Default::default();
    pub static ref IS_UAC_RUNNING: Arc<Mutex<bool>> = Default::default();
    pub static ref IS_FOREGROUND_WINDOW_ELEVATED: Arc<Mutex<bool>> = Default::default();
}

#[inline]
pub fn notify_video_frame_fetched(display_idx: usize, conn_id: i32, frame_tm: Option<Instant>) {
    if let Some(notifier) = FRAME_FETCHED_NOTIFIERS.lock().unwrap().get(&display_idx) {
        notifier.0.send((conn_id, frame_tm)).ok();
    }
}

#[inline]
pub fn notify_video_frame_fetched_by_conn_id(conn_id: i32, frame_tm: Option<Instant>) {
    let vec_display_idx: Vec<usize> = {
        let display_conn_ids = DISPLAY_CONN_IDS.lock().unwrap();
        display_conn_ids
            .iter()
            .filter_map(|(display_idx, conn_ids)| {
                if conn_ids.contains(&conn_id) {
                    Some(*display_idx)
                } else {
                    None
                }
            })
            .collect()
    };
    let notifiers = FRAME_FETCHED_NOTIFIERS.lock().unwrap();
    for display_idx in vec_display_idx {
        if let Some(notifier) = notifiers.get(&display_idx) {
            notifier.0.send((conn_id, frame_tm)).ok();
        }
    }
}

struct VideoFrameController {
    display_idx: usize,
    cur: Instant,
    send_conn_ids: HashSet<i32>,
}

impl VideoFrameController {
    fn new(display_idx: usize) -> Self {
        Self {
            display_idx,
            cur: Instant::now(),
            send_conn_ids: HashSet::new(),
        }
    }

    fn reset(&mut self) {
        self.send_conn_ids.clear();
    }

    fn set_send(&mut self, tm: Instant, conn_ids: HashSet<i32>) {
        if !conn_ids.is_empty() {
            self.cur = tm;
            self.send_conn_ids = conn_ids;
            DISPLAY_CONN_IDS
                .lock()
                .unwrap()
                .insert(self.display_idx, self.send_conn_ids.clone());
        }
    }

    #[tokio::main(flavor = "current_thread")]
    async fn try_wait_next(&mut self, fetched_conn_ids: &mut HashSet<i32>, timeout_millis: u64) {
        if self.send_conn_ids.is_empty() {
            return;
        }

        let timeout_dur = Duration::from_millis(timeout_millis as u64);
        let receiver = {
            match FRAME_FETCHED_NOTIFIERS
                .lock()
                .unwrap()
                .get(&self.display_idx)
            {
                Some(notifier) => notifier.1.clone(),
                None => {
                    return;
                }
            }
        };
        let mut receiver_guard = receiver.lock().await;
        match tokio::time::timeout(timeout_dur, receiver_guard.recv()).await {
            Err(_) => {
                // break if timeout
                // log::error!("blocking wait frame receiving timeout {}", timeout_millis);
            }
            Ok(Some((id, instant))) => {
                if let Some(tm) = instant {
                    log::trace!("Channel recv latency: {}", tm.elapsed().as_secs_f32());
                }
                fetched_conn_ids.insert(id);
            }
            Ok(None) => {
                // this branch would never be reached
            }
        }
        while !receiver_guard.is_empty() {
            if let Some((id, instant)) = receiver_guard.recv().await {
                if let Some(tm) = instant {
                    log::trace!("Channel recv latency: {}", tm.elapsed().as_secs_f32());
                }
                fetched_conn_ids.insert(id);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum VideoSource {
    Monitor,
}

impl VideoSource {
    pub fn service_name_prefix(&self) -> &'static str {
        match self {
            VideoSource::Monitor => "monitor",
        }
    }
}

#[derive(Clone)]
pub struct VideoService {
    sp: GenericService,
    idx: usize,
    source: VideoSource,
    dxgi_recovery_state: Arc<Mutex<DxgiRecoveryState>>,
}

impl Deref for VideoService {
    type Target = ServiceTmpl<ConnInner>;

    fn deref(&self) -> &Self::Target {
        &self.sp
    }
}

impl DerefMut for VideoService {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sp
    }
}

pub fn get_service_name(source: VideoSource, idx: usize) -> String {
    format!("{}{}", source.service_name_prefix(), idx)
}

pub fn new(source: VideoSource, idx: usize) -> GenericService {
    let _ = FRAME_FETCHED_NOTIFIERS
        .lock()
        .unwrap()
        .entry(idx)
        .or_insert_with(|| {
            let (tx, rx) = unbounded_channel();
            (tx, Arc::new(TokioMutex::new(rx)))
        });
    let vs = VideoService {
        sp: GenericService::new(get_service_name(source, idx), true),
        idx,
        source,
        dxgi_recovery_state: Arc::new(Mutex::new(DxgiRecoveryState::new())),
    };
    GenericService::run(&vs, run);
    vs.sp
}

// Capturer object is expensive, avoiding to create it frequently.
fn create_capturer(
    display: Display,
    _current: usize,
    _portable_service_running: bool,
) -> ResultType<Box<dyn TraitCapturer>> {
    {
        log::debug!("Create capturer dxgi|gdi");
        return crate::portable_service::client::create_capturer(
            _current,
            display,
            _portable_service_running,
        );
    }
}

pub(super) struct CapturerInfo {
    pub origin: (i32, i32),
    pub width: usize,
    pub height: usize,
    pub ndisplay: usize,
    pub current: usize,
    pub capturer: Box<dyn TraitCapturer>,
}

impl Deref for CapturerInfo {
    type Target = Box<dyn TraitCapturer>;

    fn deref(&self) -> &Self::Target {
        &self.capturer
    }
}

impl DerefMut for CapturerInfo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.capturer
    }
}

fn get_capturer_monitor(
    current: usize,
    portable_service_running: bool,
) -> ResultType<CapturerInfo> {

    let mut displays = Display::all()?;
    let ndisplay = displays.len();
    if ndisplay <= current {
        bail!(
            "Failed to get display {}, displays len: {}",
            current,
            ndisplay
        );
    }

    let display = displays.remove(current);


    let (origin, width, height) = (display.origin(), display.width(), display.height());
    let name = display.name();
    log::debug!(
        "#displays={}, current={}, origin: {:?}, width={}, height={}, cpus={}/{}, name:{}",
        ndisplay,
        current,
        &origin,
        width,
        height,
        num_cpus::get_physical(),
        num_cpus::get(),
        &name,
    );

    let capturer = create_capturer(display, current, portable_service_running)?;
    Ok(CapturerInfo {
        origin,
        width,
        height,
        ndisplay,
        current,
        capturer,
    })
}

fn get_capturer(
    source: VideoSource,
    current: usize,
    portable_service_running: bool,
) -> ResultType<CapturerInfo> {
    match source {
        VideoSource::Monitor => get_capturer_monitor(current, portable_service_running),
    }
}

fn run(vs: VideoService) -> ResultType<()> {
    let mut _raii = Raii::new(vs.idx, vs.sp.name());
    // Wayland only support one video capturer for now. It is ok to call ensure_inited() here.
    //
    // ensure_inited() is needed because clear() may be called.
    // to-do: wayland ensure_inited should pass current display index.
    // But for now, we do not support multi-screen capture on wayland.

    let last_portable_service_running = crate::portable_service::client::running();

    let display_idx = vs.idx;
    let dxgi_recovery_state = vs.dxgi_recovery_state.clone();
    let sp = vs.sp;
    let mut c = get_capturer(vs.source, display_idx, last_portable_service_running)?;
    // ACCESS_LOST marks the next successful capturer creation as a recovery. This timestamp is
    // consumed once and temporarily holds off the normal WouldBlock-to-GDI fallback, giving the
    // replacement DXGI capturer time to produce its first frame. Normal startup is unaffected.
    let dxgi_recovery_started = dxgi_recovery_state
        .lock()
        .unwrap()
        .take_restart_pending()
        .then(Instant::now);
    if dxgi_recovery_state.lock().unwrap().take_fallback_pending() {
        c.set_gdi();
        log::info!("dxgi recovery exhausted, fall back to gdi");
    }
    if !scrap::codec::enable_directx_capture() && !c.is_gdi() {
        log::info!("disable dxgi with option, fall back to gdi");
        c.set_gdi();
    }
    let mut video_qos = VIDEO_QOS.lock().unwrap();
    let mut spf = video_qos.spf();
    let mut quality = video_qos.ratio();
    drop(video_qos);
    let (mut encoder, encoder_cfg, codec_format, use_i444) = match setup_encoder(
        &c,
        sp.name(),
        quality,
        last_portable_service_running,
        vs.source,
    ) {
        Ok(result) => result,
        Err(err) => {
            log::error!("Failed to create encoder: {err:?}, fallback to VP9");
            Encoder::set_fallback(&EncoderCfg::VPX(VpxEncoderConfig {
                width: c.width as _,
                height: c.height as _,
                quality,
                codec: VpxVideoCodecId::VP9,
                keyframe_interval: None,
            }));
            setup_encoder(
                &c,
                sp.name(),
                quality,
                last_portable_service_running,
                vs.source,
            )?
        }
    };
    #[cfg(feature = "vram")]
    c.set_output_texture(encoder.input_texture());
    VIDEO_QOS.lock().unwrap().store_bitrate(encoder.bitrate());
    VIDEO_QOS
        .lock()
        .unwrap()
        .set_support_changing_quality(&sp.name(), encoder.support_changing_quality());
    log::info!("initial quality: {quality:?}");

    if sp.is_option_true(OPTION_REFRESH) {
        sp.set_option_bool(OPTION_REFRESH, false);
    }

    let mut frame_controller = VideoFrameController::new(display_idx);

    let start = time::Instant::now();
    let mut last_check_displays = time::Instant::now();
    let mut try_gdi = 1;
    log::info!("gdi: {}", c.is_gdi());
    start_uac_elevation_check();

    let mut yuv = Vec::new();
    let mut mid_data = Vec::new();
    let mut repeat_encode_counter = 0;
    let repeat_encode_max = 10;
    let mut encode_fail_counter = 0;
    let mut first_frame = true;
    // Set once the encoder emits its first packet; until then it may legitimately return nothing.
    let mut encoder_warmed_up = false;
    let mut static_refresh = static_refresh::StaticRefresh::new(
        codec_format,
        &encoder_cfg,
        &sp,
        display_idx,
    );
    let (mut second_instant, mut send_counter) = (Instant::now(), 0);

    while sp.ok() {
        check_qos(
            &mut encoder,
            &mut quality,
            &mut spf,
            &mut send_counter,
            &mut second_instant,
            &sp.name(),
        )?;
        // On macOS, a refresh here would destroy a hardware encoder that has been fed a frame but
        // not yet produced its first packet, restarting the warm-up forever. Defer until it has;
        // one that never does is still switched away from by max_fail_times in handle_one_frame.
        if sp.is_option_true(OPTION_REFRESH)
            && refresh_allowed(first_frame, encoder_warmed_up, encoder.latency_free())
        {
            let _ = try_broadcast_display_changed(&sp, display_idx, &c, true);
            log::info!("switch to refresh");
            bail!("SWITCH");
        }
        if codec_format != Encoder::negotiated_codec() {
            log::info!(
                "switch due to codec changed, {:?} -> {:?}",
                codec_format,
                Encoder::negotiated_codec()
            );
            bail!("SWITCH");
        }
        if last_portable_service_running != crate::portable_service::client::running() {
            log::info!("switch due to portable service running changed");
            bail!("SWITCH");
        }
        if Encoder::use_i444(&encoder_cfg) != use_i444 {
            log::info!("switch due to i444 changed");
            bail!("SWITCH");
        }
        #[cfg(feature = "vram")]
        if c.is_gdi() && encoder.input_texture() {
            log::info!("changed to gdi when using vram");
            VRamEncoder::set_fallback_gdi(sp.name(), true);
            bail!("SWITCH");
        }
        {
            if crate::platform::windows::desktop_changed()
                && !crate::portable_service::client::running()
            {
                bail!("Desktop changed");
            }
        }
        let now = time::Instant::now();
        if last_check_displays.elapsed().as_millis() > 1000 {
            last_check_displays = now;
            // This check may be redundant, but it is better to be safe.
            // The previous check in `sp.is_option_true(OPTION_REFRESH)` block may be enough.
            try_broadcast_display_changed(&sp, display_idx, &c, false)?;
        }

        frame_controller.reset();

        let time = now - start;
        let ms = (time.as_secs() * 1000 + time.subsec_millis() as u64) as i64;
        let res = match c.frame(spf) {
            Ok(frame) => {
                repeat_encode_counter = 0;
                if frame.valid() {
                    let frame = frame.to(encoder.yuvfmt(), &mut yuv, &mut mid_data)?;
                    static_refresh.on_frame(&frame);
                    let result = handle_one_frame(
                        display_idx,
                        &sp,
                        frame,
                        ms,
                        &mut encoder,
                        &mut encode_fail_counter,
                        &mut first_frame,
                        &mut encoder_warmed_up,
                    )?;
                    static_refresh.on_frame_encoded(&result);
                    frame_controller.set_send(now, result.send_conn_ids);
                    send_counter += 1;
                }
                {
                    #[cfg(feature = "vram")]
                    if try_gdi == 1 && !c.is_gdi() {
                        VRamEncoder::set_fallback_gdi(sp.name(), false);
                    }
                    try_gdi = 0;
                }
                Ok(())
            }
            Err(err) => Err(err),
        };

        match res {
            Err(ref e) if e.kind() == WouldBlock => {
                if dxgi_recovery_started
                    .map(|started| started.elapsed() >= DXGI_RECOVERY_FRAME_GRACE)
                    .unwrap_or(true)
                {
                    if try_gdi > 0 && !c.is_gdi() {
                        if try_gdi > 3 {
                            c.set_gdi();
                            try_gdi = 0;
                            log::info!("No image, fall back to gdi");
                        }
                        try_gdi += 1;
                    }
                }
                if !encoder.latency_free() && yuv.len() > 0 {
                    // yun.len() > 0 means the frame is not texture.
                    if repeat_encode_counter < repeat_encode_max {
                        repeat_encode_counter += 1;
                        let send_conn_ids = handle_one_frame(
                            display_idx,
                            &sp,
                            EncodeInput::YUV(&yuv),
                            ms,
                            &mut encoder,
                            &mut encode_fail_counter,
                            &mut first_frame,
                            &mut encoder_warmed_up,
                        )?
                        .send_conn_ids;
                        static_refresh.on_encoded(!send_conn_ids.is_empty());
                        frame_controller.set_send(now, send_conn_ids);
                        send_counter += 1;
                    }
                }
                static_refresh.try_encode(
                    &yuv,
                    spf,
                    quality,
                    now,
                    ms,
                    &mut encoder,
                    &mut frame_controller,
                )?;
            }
            Err(err) => {
                // The display-change check can restart capture before error handling below.
                let recovery_attempt = if !c.is_gdi() && err.kind() == ConnectionReset {
                    dxgi_recovery_state.lock().unwrap().next_attempt()
                } else {
                    None
                };
                // This check may be redundant, but it is better to be safe.
                // The previous check in `sp.is_option_true(OPTION_REFRESH)` block may be enough.
                try_broadcast_display_changed(&sp, display_idx, &c, true)?;

                if !c.is_gdi() {
                    if err.kind() == ConnectionReset {
                        if let Some(attempt) = recovery_attempt {
                            log::debug!(
                                "dxgi access lost, restart capture: attempt {attempt}, error: {err:?}"
                            );
                            bail!("SWITCH");
                        }
                        log::warn!(
                            "dxgi access lost after {DXGI_RECOVERY_LIMIT} restarts in {} seconds, fall back to gdi: {err:?}",
                            DXGI_RECOVERY_WINDOW.as_secs()
                        );
                        dxgi_recovery_state.lock().unwrap().take_fallback_pending();
                    }
                    c.set_gdi();
                    log::info!("dxgi error, fall back to gdi: {:?}", err);
                    continue;
                }
                return Err(err.into());
            }
            _ => {
            }
        }

        let mut fetched_conn_ids = HashSet::new();
        let timeout_millis = 3_000u64;
        let wait_begin = Instant::now();
        while wait_begin.elapsed().as_millis() < timeout_millis as _ {
            frame_controller.try_wait_next(&mut fetched_conn_ids, 300);
            // break if all connections have received current frame
            if fetched_conn_ids.len() >= frame_controller.send_conn_ids.len() {
                break;
            }
        }
        DISPLAY_CONN_IDS.lock().unwrap().remove(&display_idx);

        let elapsed = now.elapsed();
        // may need to enable frame(timeout)
        log::trace!("{:?} {:?}", time::Instant::now(), elapsed);
        if elapsed < spf {
            std::thread::sleep(spf - elapsed);
        }
    }

    Ok(())
}

struct Raii {
    display_idx: usize,
    name: String,
    try_vram: bool,
}

impl Raii {
    fn new(display_idx: usize, name: String) -> Self {
        log::info!("new video service: {}", name);
        VIDEO_QOS.lock().unwrap().new_display(name.clone());
        Raii {
            display_idx,
            name,
            try_vram: true,
        }
    }
}

impl Drop for Raii {
    fn drop(&mut self) {
        log::info!("stop video service: {}", self.name);
        #[cfg(feature = "vram")]
        if self.try_vram {
            VRamEncoder::set_not_use(self.name.clone(), false);
        }
        #[cfg(feature = "vram")]
        Encoder::update(scrap::codec::EncodingUpdate::Check);
        VIDEO_QOS.lock().unwrap().remove_display(&self.name);
        DISPLAY_CONN_IDS.lock().unwrap().remove(&self.display_idx);
    }
}

fn setup_encoder(
    c: &CapturerInfo,
    name: String,
    quality: f32,
    last_portable_service_running: bool,
    source: VideoSource,
) -> ResultType<(Encoder, EncoderCfg, CodecFormat, bool)> {
    let encoder_cfg = get_encoder_config(
        &c,
        name.to_string(),
        quality,
        last_portable_service_running,
        source,
    );
    Encoder::set_fallback(&encoder_cfg);
    let codec_format = Encoder::negotiated_codec();
    let use_i444 = Encoder::use_i444(&encoder_cfg);
    let encoder = Encoder::new(encoder_cfg.clone(), use_i444)?;
    Ok((encoder, encoder_cfg, codec_format, use_i444))
}

fn get_encoder_config(
    c: &CapturerInfo,
    _name: String,
    quality: f32,
    _portable_service: bool,
    _source: VideoSource,
) -> EncoderCfg {
    #[cfg(feature = "vram")]
    if _portable_service || c.is_gdi() {
        log::info!("gdi:{}, portable:{}", c.is_gdi(), _portable_service);
        VRamEncoder::set_not_use(_name, true);
    }
    #[cfg(feature = "vram")]
    Encoder::update(scrap::codec::EncodingUpdate::Check);
    let keyframe_interval = None;
    let negotiated_codec = Encoder::negotiated_codec();
    match negotiated_codec {
        CodecFormat::H264 | CodecFormat::H265 => {
            #[cfg(feature = "vram")]
            if let Some(feature) = VRamEncoder::try_get(&c.device(), negotiated_codec) {
                return EncoderCfg::VRAM(VRamEncoderConfig {
                    device: c.device(),
                    width: c.width,
                    height: c.height,
                    quality,
                    feature,
                    keyframe_interval,
                });
            }
            #[cfg(feature = "hwcodec")]
            if let Some(hw) = HwRamEncoder::try_get(negotiated_codec) {
                return EncoderCfg::HWRAM(HwRamEncoderConfig {
                    name: hw.name,
                    mc_name: hw.mc_name,
                    width: c.width,
                    height: c.height,
                    quality,
                    keyframe_interval,
                });
            }
            EncoderCfg::VPX(VpxEncoderConfig {
                width: c.width as _,
                height: c.height as _,
                quality,
                codec: VpxVideoCodecId::VP9,
                keyframe_interval,
            })
        }
        format @ (CodecFormat::VP8 | CodecFormat::VP9) => EncoderCfg::VPX(VpxEncoderConfig {
            width: c.width as _,
            height: c.height as _,
            quality,
            codec: if format == CodecFormat::VP8 {
                VpxVideoCodecId::VP8
            } else {
                VpxVideoCodecId::VP9
            },
            keyframe_interval,
        }),
        CodecFormat::AV1 => EncoderCfg::AOM(AomEncoderConfig {
            width: c.width as _,
            height: c.height as _,
            quality,
            keyframe_interval,
        }),
        _ => EncoderCfg::VPX(VpxEncoderConfig {
            width: c.width as _,
            height: c.height as _,
            quality,
            codec: VpxVideoCodecId::VP9,
            keyframe_interval,
        }),
    }
}


#[inline]
fn refresh_allowed(unfed: bool, warmed_up: bool, latency_free: bool) -> bool {
    // Only VideoToolbox has been seen to need this; other platforms keep refreshing as before.
    !cfg!(target_os = "macos") || unfed || warmed_up || latency_free
}

#[inline]
fn handle_one_frame(
    display: usize,
    sp: &GenericService,
    frame: EncodeInput,
    ms: i64,
    encoder: &mut Encoder,
    encode_fail_counter: &mut usize,
    first_frame: &mut bool,
    encoder_warmed_up: &mut bool,
) -> ResultType<static_refresh::FrameEncodeResult> {
    sp.snapshot(|sps| {
        // so that new sub and old sub share the same encoder after switch
        if sps.has_subscribes() {
            log::info!("switch due to new subscriber");
            bail!("SWITCH");
        }
        Ok(())
    })?;

    let mut send_conn_ids: HashSet<i32> = Default::default();
    let mut vpx_no_output = false;
    let first = *first_frame;
    *first_frame = false;
    match encoder.encode_to_message(frame, ms) {
        Ok(mut vf) => {
            *encode_fail_counter = 0;
            *encoder_warmed_up = true;
            vf.display = display as _;
            let mut msg = Message::new();
            msg.set_video_frame(vf);
            send_conn_ids = sp.send_video_frame(msg);
        }
        Err(e) => {
            vpx_no_output = e.is::<scrap::VpxNoOutput>();
            *encode_fail_counter += 1;
            // Encoding errors are not frequent except on Android
            if !cfg!(target_os = "android") {
                log::error!("encode fail: {e:?}, times: {}", *encode_fail_counter,);
            }
            let max_fail_times = if cfg!(target_os = "android") && encoder.is_hardware() {
                9
            } else {
                3
            };
            let repeat = !encoder.latency_free();
            // repeat encoders can reach max_fail_times on the first frame
            if (first && !repeat) || *encode_fail_counter >= max_fail_times {
                *encode_fail_counter = 0;
                if encoder.is_hardware() {
                    encoder.disable();
                    log::error!("switch due to encoding fails, first frame: {first}, error: {e:?}");
                    bail!("SWITCH");
                }
            }
            match e.to_string().as_str() {
                scrap::codec::ENCODE_NEED_SWITCH => {
                    encoder.disable();
                    log::error!("switch due to encoder need switch");
                    bail!("SWITCH");
                }
                _ => {}
            }
        }
    }
    Ok(static_refresh::FrameEncodeResult {
        send_conn_ids,
        vpx_no_output,
    })
}

#[inline]
pub fn refresh() {
}

fn start_uac_elevation_check() {
    static START: Once = Once::new();
    START.call_once(|| {
        if !crate::platform::is_root() {
            std::thread::spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if let Ok(uac) = is_process_consent_running() {
                    *IS_UAC_RUNNING.lock().unwrap() = uac;
                }
                if !crate::platform::is_elevated(None).unwrap_or(false) {
                    if let Ok(elevated) = crate::platform::is_foreground_window_elevated() {
                        *IS_FOREGROUND_WINDOW_ELEVATED.lock().unwrap() = elevated;
                    }
                }
            });
        }
    });
}

#[inline]
fn try_broadcast_display_changed(
    sp: &GenericService,
    display_idx: usize,
    cap: &CapturerInfo,
    refresh: bool,
) -> ResultType<()> {
    if refresh {
        // Get display information immediately.
        crate::display_service::check_displays_changed().ok();
    }
    if let Some(display) = check_display_changed(
        cap.ndisplay,
        cap.current,
        (cap.origin.0, cap.origin.1, cap.width, cap.height),
    ) {
        log::info!("Display {} changed", display);
        if let Some(msg_out) =
            make_display_changed_msg(display_idx, Some(display), VideoSource::Monitor)
        {
            let msg_out = Arc::new(msg_out);
            sp.send_shared(msg_out.clone());
            // switch display may occur before the first video frame, add snapshot to send to new subscribers
            sp.snapshot(move |sps| {
                sps.send_shared(msg_out.clone());
                Ok(())
            })?;
            bail!("SWITCH");
        }
    }
    Ok(())
}

pub fn make_display_changed_msg(
    display_idx: usize,
    opt_display: Option<DisplayInfo>,
    source: VideoSource,
) -> Option<Message> {
    let display = match opt_display {
        Some(d) => d,
        None => match source {
            VideoSource::Monitor => display_service::get_display_info(display_idx)?,
        },
    };
    let mut misc = Misc::new();
    misc.set_switch_display(SwitchDisplay {
        display: display_idx as _,
        x: display.x,
        y: display.y,
        width: display.width,
        height: display.height,
        cursor_embedded: match source {
            VideoSource::Monitor => display_service::capture_cursor_embedded(),
        },
        resolutions: Some(SupportedResolutions {
            resolutions: match source {
                VideoSource::Monitor => {
                    if display.name.is_empty() {
                        vec![]
                    } else {
                        crate::platform::resolutions(&display.name)
                    }
                }
            },
            ..SupportedResolutions::default()
        })
        .into(),
        original_resolution: display.original_resolution,
        ..Default::default()
    });
    let mut msg_out = Message::new();
    msg_out.set_misc(misc);
    Some(msg_out)
}

fn check_qos(
    encoder: &mut Encoder,
    ratio: &mut f32,
    spf: &mut Duration,
    send_counter: &mut usize,
    second_instant: &mut Instant,
    name: &str,
) -> ResultType<()> {
    let mut video_qos = VIDEO_QOS.lock().unwrap();
    *spf = video_qos.spf();
    if *ratio != video_qos.ratio() {
        *ratio = video_qos.ratio();
        if encoder.support_changing_quality() {
            allow_err!(encoder.set_quality(*ratio));
            video_qos.store_bitrate(encoder.bitrate());
        } else {
            // Now only vaapi doesn't support changing quality
            if !video_qos.in_vbr_state() && !video_qos.latest_quality().is_custom() {
                log::info!("switch to change quality");
                bail!("SWITCH");
            }
        }
    }
    if second_instant.elapsed() > Duration::from_secs(1) {
        *second_instant = Instant::now();
        video_qos.update_display_data(&name, *send_counter);
        *send_counter = 0;
    }
    drop(video_qos);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::refresh_allowed;

    #[test]
    fn refresh_waits_for_hw_encoder_warmup() {
        // Deferred on macOS only; everywhere else a refresh is always allowed.
        assert_eq!(
            refresh_allowed(false, false, false),
            !cfg!(target_os = "macos")
        );
        assert!(refresh_allowed(false, true, false));
        assert!(refresh_allowed(false, false, true));
        // Nothing fed yet: no warm-up to lose, and a refresh can restart a stalled capturer.
        assert!(refresh_allowed(true, false, false));
    }
}
