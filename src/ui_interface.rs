use base::config::keys;
use hbb_common::{
    allow_err,
    bytes::Bytes,
    config::{self, Config, LocalConfig, PeerConfig},
    log,
    tokio,
};
use hbb_common::{
    sleep,
    tokio::{sync::mpsc, time},
};
use serde_derive::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::ipc;

#[derive(Clone, Debug, Serialize)]
pub struct UiStatus {
    pub mouse_time: i64,
    pub video_conn_count: usize,
    /// Filled when read, see `get_connect_status`.
    pub local_ips: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginDeviceInfo {
    pub os: String,
    pub r#type: String,
    pub name: String,
}

lazy_static::lazy_static! {
    static ref UI_STATUS : Arc<Mutex<UiStatus>> = Arc::new(Mutex::new(UiStatus{
        mouse_time: 0,
        video_conn_count: 0,
        local_ips: Vec::new(),
    }));
    static ref TEMPORARY_PASSWD : Arc<Mutex<String>> = Arc::new(Mutex::new("".to_owned()));
    static ref IS_REMOTE_MODIFY_ENABLED_BY_CONTROL_PERMISSIONS : Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
}

lazy_static::lazy_static! {
    static ref OPTION_SYNCED: Arc<Mutex<bool>> = Default::default();
    static ref OPTIONS : Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(Config::get_options()));
    pub static ref SENDER : Mutex<mpsc::UnboundedSender<ipc::Data>> = Mutex::new(check_connect_status(true));
}

lazy_static::lazy_static! {
    pub static ref IS_FILE_TRANSFER_ENABLED: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
}

#[inline]
pub fn get_id() -> String {
    return ipc::get_id();
}

#[inline]
pub fn refresh_options() {
    {
        *OPTIONS.lock().unwrap() = Config::get_options();
    }
}

#[inline]
pub fn get_option<T: AsRef<str>>(key: T) -> String {
    {
        let map = OPTIONS.lock().unwrap();
        if let Some(v) = map.get(key.as_ref()) {
            v.to_owned()
        } else {
            "".to_owned()
        }
    }
}

#[inline]
pub fn use_texture_render() -> bool {



    {
        if !cfg!(feature = "flutter") {
            return false;
        }
        // https://learn.microsoft.com/en-us/windows/win32/sysinfo/targeting-your-application-at-windows-8-1
        #[cfg(debug_assertions)]
        let default_texture = true;
        #[cfg(not(debug_assertions))]
        let default_texture = crate::platform::is_win_10_or_greater();
        if default_texture {
            LocalConfig::get_option(keys::OPTION_TEXTURE_RENDER) != "N"
        } else {
            return LocalConfig::get_option(keys::OPTION_TEXTURE_RENDER) == "Y";
        }
    }
}

#[inline]
pub fn is_option_fixed(key: &str) -> bool {
    config::OVERWRITE_DISPLAY_SETTINGS
        .read()
        .unwrap()
        .contains_key(key)
        || config::OVERWRITE_LOCAL_SETTINGS
            .read()
            .unwrap()
            .contains_key(key)
        || config::OVERWRITE_SETTINGS.read().unwrap().contains_key(key)
}

#[inline]
pub fn get_local_option(key: String) -> String {
    crate::get_local_option(&key)
}

#[inline]
pub fn get_hard_option(key: String) -> String {
    config::HARD_SETTINGS
        .read()
        .unwrap()
        .get(&key)
        .cloned()
        .unwrap_or_default()
}

#[inline]
pub fn get_builtin_option(key: &str) -> String {
    crate::get_builtin_option(key)
}

#[inline]
pub fn set_local_option(key: String, value: String) {
    LocalConfig::set_option(key.clone(), value);
}

#[inline]
pub fn get_local_flutter_option(key: String) -> String {
    LocalConfig::get_flutter_option(&key)
}

#[inline]
pub fn set_local_flutter_option(key: String, value: String) {
    LocalConfig::set_flutter_option(key, value);
}

#[inline]
pub fn get_kb_layout_type() -> String {
    LocalConfig::get_kb_layout_type()
}

#[inline]
pub fn set_kb_layout_type(kb_layout_type: String) {
    LocalConfig::set_kb_layout_type(kb_layout_type);
}

#[inline]
pub fn peer_has_password(id: String) -> bool {
    !PeerConfig::load(&id).password.is_empty()
}

#[inline]
pub fn forget_password(id: String) {
    let mut c = PeerConfig::load(&id);
    c.password.clear();
    c.store(&id);
}

#[inline]
pub fn get_peer_option(id: String, name: String) -> String {
    let c = PeerConfig::load(&id);
    c.options.get(&name).unwrap_or(&"".to_owned()).to_owned()
}

#[inline]
pub fn get_peer_flutter_option(id: String, name: String) -> String {
    let c = PeerConfig::load(&id);
    c.ui_flutter.get(&name).unwrap_or(&"".to_owned()).to_owned()
}

#[inline]
pub fn set_peer_flutter_option(id: String, name: String, value: String) {
    let mut c = PeerConfig::load(&id);
    if value.is_empty() {
        c.ui_flutter.remove(&name);
    } else {
        c.ui_flutter.insert(name, value);
    }
    c.store(&id);
}

#[inline]
pub fn set_peer_option(id: String, name: String, value: String) {
    let mut c = PeerConfig::load(&id);
    if value.is_empty() {
        c.options.remove(&name);
    } else {
        c.options.insert(name, value);
    }
    c.store(&id);
}

#[inline]
pub fn get_options() -> String {
    let options = {
        {
            OPTIONS.lock().unwrap()
        }
    };
    let mut m = serde_json::Map::new();
    for (k, v) in options.iter() {
        m.insert(k.into(), v.to_owned().into());
    }
    serde_json::to_string(&m).unwrap_or_default()
}

#[inline]
pub fn get_sound_inputs() -> Vec<String> {
    let mut a = Vec::new();
    {
        fn get_sound_inputs_() -> Vec<String> {
            let mut out = Vec::new();
            use cpal::traits::{DeviceTrait, HostTrait};
            // Do not use `cpal::host_from_id(cpal::HostId::ScreenCaptureKit)` for feature = "screencapturekit"
            // Because we explicitly handle the "System Sound" device.
            let host = cpal::default_host();
            if let Ok(devices) = host.devices() {
                for device in devices {
                    if device.default_input_config().is_err() {
                        continue;
                    }
                    if let Ok(name) = device.name() {
                        out.push(name);
                    }
                }
            }
            out
        }

        let inputs = Arc::new(Mutex::new(Vec::new()));
        let cloned = inputs.clone();
        std::thread::spawn(move || *cloned.lock().unwrap() = get_sound_inputs_())
            .join()
            .ok();
        for name in inputs.lock().unwrap().drain(..) {
            a.push(name);
        }
    }
    a
}

#[inline]
pub fn set_options(m: HashMap<String, String>) {
    {
        *OPTIONS.lock().unwrap() = m.clone();
        ipc::set_options(m).ok();
    }
}

#[inline]
pub fn set_option(key: String, value: String) {
    if &key == "stop-service" {
    } else if &key == "audio-input" {
        crate::audio_service::restart();
    }
    {
        let mut options = OPTIONS.lock().unwrap();
        if value.is_empty() {
            options.remove(&key);
        } else {
            options.insert(key.clone(), value.clone());
        }
        ipc::set_options(options.clone()).ok();
    }
}

#[inline]
pub fn get_mouse_time() -> f64 {
    UI_STATUS.lock().unwrap().mouse_time as f64
}

#[inline]
pub fn check_mouse_time() {
    {
        let sender = SENDER.lock().unwrap();
        allow_err!(sender.send(ipc::Data::MouseMoveTime(0)));
    }
}

#[inline]
pub fn get_connect_status() -> UiStatus {
    let mut status = UI_STATUS.lock().unwrap().clone();
    status.local_ips = crate::lan::local_ipv4s();
    status
}

#[inline]
pub fn temporary_password() -> String {
    return TEMPORARY_PASSWD.lock().unwrap().clone();
}

#[inline]
pub fn update_temporary_password() {
    allow_err!(ipc::update_temporary_password());
}

#[inline]
pub fn is_permanent_password_set() -> bool {
    {
        let daemon_is_set = ipc::is_permanent_password_set();
        // `daemon_is_set` is authoritative for the return value. Local storage is only used to
        // decide whether we should attempt a sync to clear stale user-side state.
        let local_storage_is_empty = if daemon_is_set {
            true
        } else {
            let (storage, _) = Config::get_local_permanent_password_storage_and_salt();
            storage.is_empty()
        };
        if daemon_is_set || !local_storage_is_empty {
            allow_err!(ipc::sync_permanent_password_storage_from_daemon());
        }
        daemon_is_set
    }
}

#[inline]
pub fn is_local_permanent_password_set() -> bool {
    {
        allow_err!(ipc::sync_permanent_password_storage_from_daemon());
        Config::has_local_permanent_password()
    }
}

pub fn set_permanent_password_with_result(password: String) -> bool {
    if config::Config::is_disable_change_permanent_password() {
        return false;
    }
    {
        match crate::ipc::set_permanent_password_with_ack(password) {
            Ok(ok) => ok,
            Err(err) => {
                log::warn!("Failed to set permanent password via IPC: {err}");
                false
            }
        }
    }
}

#[inline]
pub fn get_peer(id: String) -> PeerConfig {
    PeerConfig::load(&id)
}

#[inline]
pub fn get_fav() -> Vec<String> {
    LocalConfig::get_fav()
}

#[inline]
pub fn store_fav(fav: Vec<String>) {
    LocalConfig::set_fav(fav);
}

#[inline]
pub fn is_process_trusted(_prompt: bool) -> bool {
    return true;
}

#[inline]
pub fn is_can_screen_recording(_prompt: bool) -> bool {
    return true;
}

#[inline]
pub fn is_installed_daemon(_prompt: bool) -> bool {
    return true;
}

#[inline]
pub fn is_can_input_monitoring(_prompt: bool) -> bool {
    return true;
}

#[inline]
pub fn get_error() -> String {
    return "".to_owned();
}

#[inline]
pub fn is_login_wayland() -> bool {
    return false;
}

#[inline]
pub fn current_is_wayland() -> bool {
    return false;
}

#[inline]
pub fn get_version() -> String {
    crate::VERSION.to_owned()
}

#[inline]
pub fn get_app_name() -> String {
    crate::get_app_name()
}

#[inline]
pub fn create_shortcut(_id: String) {
    crate::platform::windows::create_shortcut(&_id).ok();
}

#[inline]
pub fn discover() {
    crate::lan::start_discovery();
}

pub fn peer_to_map(id: String, p: PeerConfig) -> HashMap<&'static str, String> {
    use hbb_common::sodiumoxide::base64;
    HashMap::<&str, String>::from_iter([
        ("id", id),
        ("username", p.info.username.clone()),
        ("hostname", p.info.hostname.clone()),
        ("platform", p.info.platform.clone()),
        (
            "alias",
            p.options.get("alias").unwrap_or(&"".to_owned()).to_owned(),
        ),
        (
            "hash",
            base64::encode(p.password, base64::Variant::Original),
        ),
    ])
}

pub fn peer_exists(id: &str) -> bool {
    PeerConfig::exists(id)
}

#[inline]
pub fn get_lan_peers() -> Vec<HashMap<&'static str, String>> {
    config::LanPeers::load()
        .peers
        .iter()
        .map(|peer| {
            HashMap::<&str, String>::from_iter([
                ("id", peer.id.clone()),
                ("username", peer.username.clone()),
                ("hostname", peer.hostname.clone()),
                ("platform", peer.platform.clone()),
                ("ip", peer.ip_mac.keys().min().cloned().unwrap_or_default()),
                ("version", peer.version.clone()),
            ])
        })
        .collect()
}

#[inline]
pub fn remove_discovered(id: String) {
    let mut peers = config::LanPeers::load().peers;
    peers.retain(|x| x.id != id);
    config::LanPeers::store(&peers);
}

#[inline]
pub fn get_uuid() -> String {
    crate::encode64(hbb_common::get_uuid())
}

#[inline]
pub fn get_langs() -> String {
    use serde_json::json;
    let mut x: Vec<(&str, String)> = crate::lang::LANGS
        .iter()
        .map(|a| (a.0, format!("{} ({})", a.1, a.0)))
        .collect();
    x.sort_by(|a, b| a.0.cmp(b.0));
    json!(x).to_string()
}

#[inline]
pub fn has_hwcodec() -> bool {
    // Has real hardware codec using gpu
    (cfg!(feature = "hwcodec") && cfg!(not(target_os = "ios"))) || cfg!(feature = "mediacodec")
}

#[inline]
pub fn has_vram() -> bool {
    cfg!(feature = "vram")
}

#[inline]
pub fn supported_hwdecodings() -> (bool, bool) {
    let decoding =
        scrap::codec::Decoder::supported_decodings(None, use_texture_render(), None, &vec![]);
    #[allow(unused_mut)]
    let (mut h264, mut h265) = (decoding.ability_h264 > 0, decoding.ability_h265 > 0);
    #[cfg(feature = "vram")]
    {
        // supported_decodings check runtime luid
        let vram = scrap::vram::VRamDecoder::possible_available_without_check();
        if vram.0 {
            h264 = true;
        }
        if vram.1 {
            h265 = true;
        }
    }
    (h264, h265)
}

#[inline]
pub fn is_root() -> bool {
    crate::platform::is_root()
}


#[inline]
pub fn check_super_user_permission() -> bool {
    return crate::platform::check_super_user_permission().unwrap_or(false);
}

// Make sure `SENDER` is inited here.
#[inline]
pub fn start_option_status_sync() {
    let _sender = SENDER.lock().unwrap();
}

// not call directly
fn check_connect_status(reconnect: bool) -> mpsc::UnboundedSender<ipc::Data> {
    let (tx, rx) = mpsc::unbounded_channel::<ipc::Data>();
    std::thread::spawn(move || check_connect_status_(reconnect, rx));
    tx
}

pub fn set_user_default_option(key: String, value: String) {
    use hbb_common::config::UserDefaultConfig;
    UserDefaultConfig::load().set(key, value);
}

pub fn get_user_default_option(key: String) -> String {
    use hbb_common::config::UserDefaultConfig;
    UserDefaultConfig::load().get(&key)
}

pub fn get_fingerprint() -> String {
    return ipc::get_fingerprint();
}

#[inline]
pub fn get_login_device_info() -> LoginDeviceInfo {
    LoginDeviceInfo {
        // std::env::consts::OS is better than whoami::platform() here.
        os: std::env::consts::OS.to_owned(),
        r#type: "client".to_owned(),
        name: crate::common::hostname(),
    }
}

#[inline]
pub fn get_login_device_info_json() -> String {
    serde_json::to_string(&get_login_device_info()).unwrap_or("{}".to_string())
}

// notice: avoiding create ipc connection repeatedly,
// because windows named pipe has serious memory leak issue.
#[tokio::main(flavor = "current_thread")]
async fn check_connect_status_(reconnect: bool, rx: mpsc::UnboundedReceiver<ipc::Data>) {
    let mut rx = rx;
    let is_cm = crate::common::is_cm();

    loop {
        if let Ok(mut c) = ipc::connect(1000, "").await {
            let mut timer = crate::rustdesk_interval(time::interval(time::Duration::from_secs(1)));
            loop {
                tokio::select! {
                    res = c.next() => {
                        match res {
                            Err(err) => {
                                log::error!("ipc connection closed: {}", err);
                                if is_cm {
                                    crate::ui_cm_interface::quit_cm();
                                }
                                break;
                            }
                            Ok(Some(ipc::Data::MouseMoveTime(v))) => {
                                UI_STATUS.lock().unwrap().mouse_time = v;
                            }
                            Ok(Some(ipc::Data::Options(Some(v)))) => {
                                *OPTIONS.lock().unwrap() = v;
                                *OPTION_SYNCED.lock().unwrap() = true;
                            }
                            Ok(Some(ipc::Data::Config((name, Some(value))))) => {
                                if name == "temporary-password" {
                                    *TEMPORARY_PASSWD.lock().unwrap() = value;
                                }
                            }
                            Ok(Some(ipc::Data::VideoConnCount(Some(n)))) => {
                                UI_STATUS.lock().unwrap().video_conn_count = n;
                            }
                            Ok(Some(ipc::Data::ControlPermissionsRemoteModify(v))) => {
                                *IS_REMOTE_MODIFY_ENABLED_BY_CONTROL_PERMISSIONS.lock().unwrap() = v;
                            }
                            #[cfg(target_os = "windows")]
                            Ok(Some(ipc::Data::FileTransferEnabledState(v))) => {
                                if let Some(enabled) = v {
                                    let mut lock = IS_FILE_TRANSFER_ENABLED.lock().unwrap();
                                    if *lock != v {
                                        clipboard::ContextSend::enable(enabled);
                                        *lock = v;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(data) = rx.recv() => {
                        allow_err!(c.send(&data).await);
                    }
                    _ = timer.tick() => {
                        c.send(&ipc::Data::Options(None)).await.ok();
                        c.send(&ipc::Data::Config(("temporary-password".to_owned(), None))).await.ok();
                        #[cfg(feature = "flutter")]
                        c.send(&ipc::Data::VideoConnCount(None)).await.ok();
                        c.send(&ipc::Data::ControlPermissionsRemoteModify(None)).await.ok();
                        #[cfg(target_os = "windows")]
                        c.send(&ipc::Data::FileTransferEnabledState(None)).await.ok();
                    }
                }
            }
        }
        if !reconnect {
            OPTIONS
                .lock()
                .unwrap()
                .insert("ipc-closed".to_owned(), "Y".to_owned());
            break;
        }
        sleep(1.).await;
    }
}

#[allow(dead_code)]
pub fn option_synced() -> bool {
    {
        OPTION_SYNCED.lock().unwrap().clone()
    }
}

#[tokio::main(flavor = "current_thread")]
pub(crate) async fn send_to_cm(data: &ipc::Data) {
    if let Ok(mut c) = ipc::connect(1000, "_cm").await {
        c.send(data).await.ok();
    }
}

// if it's relay id, return id processed, otherwise return original id
pub fn handle_relay_id(id: &str) -> &str {
    if id.ends_with(r"\r") || id.ends_with(r"/r") {
        &id[0..id.len() - 2]
    } else {
        id
    }
}

pub fn support_remove_wallpaper() -> bool {
    return crate::platform::WallPaperRemover::support();
}

pub fn has_valid_2fa() -> bool {
    let raw = get_option("2fa");
    crate::auth_2fa::get_2fa(Some(raw)).is_some()
}

pub fn generate2fa() -> String {
    crate::auth_2fa::generate2fa()
}

pub fn verify2fa(code: String) -> bool {
    let res = crate::auth_2fa::verify2fa(code);
    if res {
        refresh_options();
    }
    res
}

pub fn check_hwcodec() {
    #[cfg(feature = "hwcodec")]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();

        ONCE.call_once(|| {
            scrap::hwcodec::start_check_process();
        })
    }
}

pub fn get_unlock_pin() -> String {
    return ipc::get_unlock_pin();
}

pub fn set_unlock_pin(pin: String) -> String {
    match ipc::set_unlock_pin(pin, true) {
        Ok(_) => String::default(),
        Err(err) => err.to_string(),
    }
}

pub fn get_trusted_devices() -> String {
    return ipc::get_trusted_devices();
}

pub fn remove_trusted_devices(json: &str) {
    let hwids = serde_json::from_str::<Vec<Bytes>>(json).unwrap_or_default();
    ipc::remove_trusted_devices(hwids);
}

pub fn clear_trusted_devices() {
    ipc::clear_trusted_devices();
}

pub fn max_encrypt_len() -> usize {
    hbb_common::config::ENCRYPT_MAX_LEN
}

pub fn is_remote_modify_enabled_by_control_permissions() -> Option<bool> {
    *IS_REMOTE_MODIFY_ENABLED_BY_CONTROL_PERMISSIONS
        .lock()
        .unwrap()
}
