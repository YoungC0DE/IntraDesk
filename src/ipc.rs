#[path = "ipc/auth.rs"]
mod ipc_auth;
use crate::{
    common::CheckTestNatType,
    rendezvous_mediator::RendezvousMediator,
    ui_interface::{get_local_option, set_local_option},
};
use bytes::Bytes;
pub use clipboard::ClipboardFile;
use hbb_common::{
    allow_err, bail, bytes,
    bytes_codec::BytesCodec,
    config::{self, Config, Config2},
    futures::StreamExt as _,
    futures_util::sink::SinkExt,
    log, password_security as password, timeout,
    tokio::{
        self,
        io::{AsyncRead, AsyncWrite},
    },
    tokio_util::codec::Framed,
    ResultType,
};
use base::config::keys::{self, OPTION_ALLOW_WEBSOCKET};
pub(crate) use ipc_auth::authorize_windows_portable_service_ipc_connection;
use ipc_auth::{
    authorize_windows_main_ipc_connection, portable_service_listener_security_attributes,
    should_allow_everyone_create_on_windows,
};
// Gated with the module that uses it, so a `drm`-less build does not carry an unused import.
use parity_tokio_ipc::{
    Connection as Conn, ConnectionClient as ConnClient, Endpoint, Incoming, SecurityAttributes,
};
use serde_derive::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

// IPC actions here.
pub const IPC_ACTION_CLOSE: &str = "close";
const PORTABLE_SERVICE_IPC_HANDSHAKE_TIMEOUT_MS: u64 = 3_000;
pub(crate) const IPC_TOKEN_LEN: usize = 64;
const IPC_TOKEN_RANDOM_BYTES: usize = IPC_TOKEN_LEN / 2;
const _: () = assert!(IPC_TOKEN_LEN % 2 == 0);
pub static EXIT_RECV_CLOSE: AtomicBool = AtomicBool::new(true);





#[inline]
pub async fn connect_service(ms_timeout: u64) -> ResultType<ConnectionTmpl<ConnClient>> {
    connect(ms_timeout, crate::POSTFIX_SERVICE).await
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum FS {
    ReadEmptyDirs {
        dir: String,
        include_hidden: bool,
    },
    ReadDir {
        dir: String,
        include_hidden: bool,
    },
    RemoveDir {
        path: String,
        id: i32,
        recursive: bool,
    },
    RemoveFile {
        path: String,
        id: i32,
        file_num: i32,
    },
    CreateDir {
        path: String,
        id: i32,
    },
    NewWrite {
        path: String,
        id: i32,
        file_num: i32,
        files: Vec<(String, u64)>,
        overwrite_detection: bool,
        total_size: u64,
        conn_id: i32,
    },
    CancelWrite {
        id: i32,
    },
    WriteBlock {
        id: i32,
        file_num: i32,
        data: Bytes,
        compressed: bool,
    },
    WriteDone {
        id: i32,
        file_num: i32,
    },
    WriteError {
        id: i32,
        file_num: i32,
        err: String,
    },
    WriteOffset {
        id: i32,
        file_num: i32,
        offset_blk: u32,
    },
    CheckDigest {
        id: i32,
        file_num: i32,
        file_size: u64,
        last_modified: u64,
        is_upload: bool,
        is_resume: bool,
    },
    SendConfirm(Vec<u8>),
    Rename {
        id: i32,
        path: String,
        new_name: String,
    },
    // CM-side file reading operations (Windows only)
    // These enable Connection Manager to read files and stream them back to Connection
    ReadFile {
        path: String,
        id: i32,
        file_num: i32,
        include_hidden: bool,
        conn_id: i32,
        overwrite_detection: bool,
    },
    CancelRead {
        id: i32,
        conn_id: i32,
    },
    SendConfirmForRead {
        id: i32,
        file_num: i32,
        skip: bool,
        offset_blk: u32,
        conn_id: i32,
    },
    ReadAllFiles {
        path: String,
        id: i32,
        include_hidden: bool,
        conn_id: i32,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t")]
pub struct ClipboardNonFile {
    pub compress: bool,
    pub content: bytes::Bytes,
    pub content_len: usize,
    pub next_raw: bool,
    pub width: i32,
    pub height: i32,
    // message.proto: ClipboardFormat
    pub format: i32,
    pub special_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum DataKeyboard {
    Sequence(String),
    KeyDown(enigo::Key),
    KeyUp(enigo::Key),
    KeyClick(enigo::Key),
    GetKeyState(enigo::Key),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum DataKeyboardResponse {
    GetKeyState(bool),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum DataMouse {
    MoveTo(i32, i32),
    MoveRelative(i32, i32),
    Down(enigo::MouseButton),
    Up(enigo::MouseButton),
    Click(enigo::MouseButton),
    ScrollX(i32),
    ScrollY(i32),
    Refresh,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum DataControl {
    Resolution {
        minx: i32,
        maxx: i32,
        miny: i32,
        maxy: i32,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum DataPortableService {
    Ping,
    Pong,
    AuthToken(String),
    AuthResult(bool),
    ConnCount(Option<usize>),
    Mouse((Vec<u8>, i32, String, u32, bool, bool)),
    Pointer((Vec<u8>, i32)),
    Key(Vec<u8>),
    RequestStart,
    WillClose,
    CmShowElevation(bool),
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum SwitchSidesUuidAction {
    Check,
    Consume,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum Data {
    Login {
        id: i32,
        is_file_transfer: bool,
        peer_id: String,
        name: String,
        avatar: String,
        authorized: bool,
        keyboard: bool,
        clipboard: bool,
        audio: bool,
        file: bool,
        file_transfer_enabled: bool,
        restart: bool,
        from_switch: bool,
    },
    SwitchPermission {
        name: String,
        enabled: bool,
    },
    SystemInfo(Option<String>),
    ClickTime(i64),
    MouseMoveTime(i64),
    Authorize,
    Close,
    SAS,
    UserSid(Option<u32>),
    OnlineStatus(Option<(i64, bool)>),
    Config((String, Option<String>)),
    Options(Option<HashMap<String, String>>),
    NatType(Option<i32>),
    ConfirmedKey(Option<(Vec<u8>, Vec<u8>)>),
    RawMessage(Vec<u8>),
    Socks(Option<config::Socks5Server>),
    FS(FS),
    Test,
    SyncConfig(Option<Box<(Config, Config2)>>),
    ClipboardFile(ClipboardFile),
    ClipboardFileEnabled(bool),
    ClipboardNonFile(Option<(String, Vec<ClipboardNonFile>)>),
    TestRendezvousServer,
    Deployed,
    Keyboard(DataKeyboard),
    KeyboardResponse(DataKeyboardResponse),
    Mouse(DataMouse),
    Control(DataControl),
    Theme(String),
    Language(String),
    Empty,
    Disconnected,
    DataPortableService(DataPortableService),
    SwitchSidesRequest(String),
    SwitchSidesUuid(String, String, SwitchSidesUuidAction, Option<bool>),
    SwitchSidesBack,
    UrlLink(String),
    SyncWinCpuUsage(Option<f64>),
    FileTransferLog((String, String)),
    ControlledSessionCount(usize),
    CmErr(String),
    // CM-side file reading responses (Windows only)
    // These are sent from CM back to Connection when CM handles file reading
    /// Response to ReadFile: contains initial file list or error
    ReadJobInitResult {
        id: i32,
        file_num: i32,
        include_hidden: bool,
        conn_id: i32,
        /// Serialized protobuf bytes of FileDirectory, or error string
        result: Result<Vec<u8>, String>,
    },
    /// File data block read by CM.
    ///
    /// The actual data is sent separately via `send_raw()` after this message to avoid
    /// JSON encoding overhead for large binary data. This mirrors the `WriteBlock` pattern.
    ///
    /// **Protocol:**
    /// - Sender: `send(FileBlockFromCM{...})` then `send_raw(data)`
    /// - Receiver: `next()` returns `FileBlockFromCM`, then `next_raw()` returns data bytes
    ///
    /// **Note on empty data (e.g., empty files):**
    /// Empty data is supported. The IPC connection uses `BytesCodec` with `raw=false` (default),
    /// which prefixes each frame with a length header. So `send_raw(Bytes::new())` sends a
    /// 1-byte frame (length=0), and `next_raw()` correctly returns an empty `BytesMut`.
    /// See `libs/hbb_common/src/bytes_codec.rs` test `test_codec2` for verification.
    FileBlockFromCM {
        id: i32,
        file_num: i32,
        /// Data is sent separately via `send_raw()` to avoid JSON encoding overhead.
        /// This field is skipped during serialization; sender must call `send_raw()` after sending.
        /// Receiver must call `next_raw()` and populate this field manually.
        #[serde(skip)]
        data: bytes::Bytes,
        compressed: bool,
        conn_id: i32,
    },
    /// File read completed successfully
    FileReadDone {
        id: i32,
        file_num: i32,
        conn_id: i32,
    },
    /// File read failed with error
    FileReadError {
        id: i32,
        file_num: i32,
        err: String,
        conn_id: i32,
    },
    /// Digest info from CM for overwrite detection
    FileDigestFromCM {
        id: i32,
        file_num: i32,
        last_modified: u64,
        file_size: u64,
        is_resume: bool,
        conn_id: i32,
    },
    /// Response to ReadAllFiles: recursive directory listing
    AllFilesResult {
        id: i32,
        conn_id: i32,
        path: String,
        /// Serialized protobuf bytes of FileDirectory, or error string
        result: Result<Vec<u8>, String>,
    },
    CheckHwcodec,
    VideoConnCount(Option<usize>),
    // Although the key is not necessary, it is used to avoid hardcoding the key.
    WaylandScreencastRestoreToken((String, String)),
    HwCodecConfig(Option<String>),
    RemoveTrustedDevices(Vec<Bytes>),
    ClearTrustedDevices,
    SocksWs(Option<Box<(Option<config::Socks5Server>, String)>>),
    ControlPermissionsRemoteModify(Option<bool>),
    FileTransferEnabledState(Option<bool>),
    // --- DRM/KMS capture (opt-in `drm` feature) over the `_drm` service-scoped channel ---
    // All of the following are `cfg(all(linux, drm))`, so the drm-off IPC wire is byte-identical
    // to upstream. `_drm` protocol: the client sends `DrmHello`, the service `DrmDisplayList`, the
    // client replies `DrmStart{display}`, then the service streams `DrmFrame` + send_raw(BGRA) and
    // `DrmCursor` + send_raw(RGBA). A frame/cursor header is ALWAYS immediately followed by exactly
    // one `send_raw()` payload (the same header-then-raw pairing as `FileBlockFromCM`). This keeps
    // the header extensible. The zero-copy `DrmFrameDmabuf(DmabufDesc)` sibling below carries only a
    // small JSON metadata descriptor; the scanout dma-buf fd rides an SCM_RIGHTS ancillary message on
    // the same `DrmConn` send (see `DrmConn::send_msg`), so it has NO trailing `send_raw()` body.
}


#[tokio::main(flavor = "current_thread")]
pub async fn start(postfix: &str) -> ResultType<()> {
    let mut incoming = new_listener(postfix).await?;
    loop {
        if let Some(result) = incoming.next().await {
            match result {
                Ok(stream) => {
                    let mut stream = Connection::new(stream);
                    let postfix = postfix.to_owned();
                    if postfix.is_empty() {
                        // Windows main IPC (`postfix == ""`) is authorized here.
                        // Other security-sensitive channels use dedicated authorization paths:
                        // - `_portable_service`: portable-service listener + handshake policy
                        // - service-scoped postfixes: service-specific listener/authorization
                        if !authorize_windows_main_ipc_connection(&stream, &postfix) {
                            continue;
                        }
                    }
                    tokio::spawn(async move {
                        loop {
                            match stream.next().await {
                                Err(err) => {
                                    log::trace!("ipc '{}' connection closed: {}", postfix, err);
                                    break;
                                }
                                Ok(Some(data)) => {
                                    // On Linux/macOS, the protected `_service` channel is used only for
                                    // syncing config between root service and the active user process.
                                    //
                                    // NOTE: `is_service_ipc_postfix()` also includes `_uinput_*`, but those
                                    // channels are handled by the dedicated uinput listener/protocol in
                                    // `src/server/uinput.rs` and therefore do not share this Data enum
                                    // allowlist. The SyncConfig allowlist here is intentionally scoped to the
                                    // `_service` channel only.
                                    //
                                    // Keep this explicit branch to avoid policy drift between `_service` and
                                    // uinput IPC paths while still minimizing exposed message surface here.
                                    handle(data, &mut stream).await;
                                }
                                Ok(None) => {
                                    // `Ok(None)` means a complete frame arrived but did not
                                    // deserialize into `Data`. Peer close/reset is returned as
                                    // `Err` by `ConnectionTmpl::next()`. Keep the historical
                                    // ignore behavior except on the protected `_service` channel.
                                }
                            }
                        }
                    });
                }
                Err(err) => {
                    log::error!("Couldn't get client: {:?}", err);
                }
            }
        }
    }
}

pub async fn new_listener(postfix: &str) -> ResultType<Incoming> {
    let path = Config::ipc_path(postfix);
    let mut endpoint = Endpoint::new(path.clone());
    let security_attrs = {
        {
            if postfix == "_portable_service" {
                portable_service_listener_security_attributes()
            } else if should_allow_everyone_create_on_windows(postfix) {
                SecurityAttributes::allow_everyone_create()
            } else {
                Ok(SecurityAttributes::empty())
            }
        }
    };
    match security_attrs {
        Ok(attr) => endpoint.set_security_attributes(attr),
        Err(err) => {
            log::error!("Failed to set ipc{} security: {}", postfix, err);
            if postfix == "_portable_service" {
                // Fail closed for `_portable_service` when SDDL construction fails.
                // This endpoint is security-critical and must not start with default ACLs.
                return Err(err.into());
            }
        }
    };
    match endpoint.incoming() {
        Ok(incoming) => {
            if postfix == crate::POSTFIX_SERVICE {
                log::info!("Started protected ipc service server: postfix={}", postfix);
            } else {
                log::info!("Started ipc{} server at path: {}", postfix, &path);
            }
            Ok(incoming)
        }
        Err(err) => {
            log::error!(
                "Failed to start ipc{} server at path {}: {}",
                postfix,
                path,
                err
            );
            Err(err.into())
        }
    }
}

pub struct CheckIfRestart {
    stop_service: String,
    rendezvous_servers: Vec<String>,
    audio_input: String,
    ws: String,
    disable_udp: String,
    allow_insecure_tls_fallback: String,
    api_server: String,
}

impl CheckIfRestart {
    pub fn new() -> CheckIfRestart {
        CheckIfRestart {
            stop_service: Config::get_option("stop-service"),
            rendezvous_servers: Config::get_rendezvous_servers(),
            audio_input: Config::get_option("audio-input"),
            ws: Config::get_option(OPTION_ALLOW_WEBSOCKET),
            disable_udp: Config::get_option(keys::OPTION_DISABLE_UDP),
            allow_insecure_tls_fallback: Config::get_option(
                keys::OPTION_ALLOW_INSECURE_TLS_FALLBACK,
            ),
            api_server: Config::get_option("api-server"),
        }
    }
}
impl Drop for CheckIfRestart {
    fn drop(&mut self) {
        // If https proxy is used, we need to restart rendezvous mediator.
        // No need to check if https proxy is used, because this option does not change frequently
        // and restarting mediator is safe even https proxy is not used.
        let allow_insecure_tls_fallback_changed = self.allow_insecure_tls_fallback
            != Config::get_option(keys::OPTION_ALLOW_INSECURE_TLS_FALLBACK);
        if allow_insecure_tls_fallback_changed
            || self.stop_service != Config::get_option("stop-service")
            || self.rendezvous_servers != Config::get_rendezvous_servers()
            || self.ws != Config::get_option(OPTION_ALLOW_WEBSOCKET)
            || self.disable_udp != Config::get_option(keys::OPTION_DISABLE_UDP)
            || self.api_server != Config::get_option("api-server")
        {
            if allow_insecure_tls_fallback_changed {
                hbb_common::tls::reset_tls_cache();
            }
            RendezvousMediator::restart();
        }
        if self.audio_input != Config::get_option("audio-input") {
            crate::audio_service::restart();
        }
    }
}

async fn handle(data: Data, stream: &mut Connection) {
    match data {
        Data::SystemInfo(_) => {
            let info = format!(
                "log_path: {}, config: {}, username: {}",
                Config::log_path().to_str().unwrap_or(""),
                Config::file().to_str().unwrap_or(""),
                crate::username(),
            );
            allow_err!(stream.send(&Data::SystemInfo(Some(info))).await);
        }
        Data::ClickTime(_) => {
            let t = crate::server::CLICK_TIME.load(Ordering::SeqCst);
            allow_err!(stream.send(&Data::ClickTime(t)).await);
        }
        Data::MouseMoveTime(_) => {
            let t = crate::server::MOUSE_MOVE_TIME.load(Ordering::SeqCst);
            allow_err!(stream.send(&Data::MouseMoveTime(t)).await);
        }
        Data::Close => {
            log::info!("Receive close message");
            if EXIT_RECV_CLOSE.load(Ordering::SeqCst) {
                crate::server::input_service::fix_key_down_timeout_at_exit();
                std::process::exit(-1); // to make sure --server luauchagent process can restart because SuccessfulExit used
            }
        }
        Data::OnlineStatus(_) => {
            let x = config::get_online_state();
            let confirmed = Config::get_key_confirmed();
            allow_err!(stream.send(&Data::OnlineStatus(Some((x, confirmed)))).await);
        }
        Data::ConfirmedKey(None) => {
            let out = if Config::get_key_confirmed() {
                Some(Config::get_key_pair())
            } else {
                None
            };
            allow_err!(stream.send(&Data::ConfirmedKey(out)).await);
        }
        Data::Socks(s) => match s {
            None => {
                allow_err!(stream.send(&Data::Socks(Config::get_socks())).await);
            }
            Some(data) => {
                let _nat = CheckTestNatType::new();
                if data.proxy.is_empty() {
                    Config::set_socks(None);
                } else {
                    Config::set_socks(Some(data));
                }
                RendezvousMediator::restart();
                log::info!("socks updated");
            }
        },
        Data::SocksWs(s) => match s {
            None => {
                allow_err!(
                    stream
                        .send(&Data::SocksWs(Some(Box::new((
                            Config::get_socks(),
                            Config::get_option(OPTION_ALLOW_WEBSOCKET)
                        )))))
                        .await
                );
            }
            _ => {}
        },
        Data::VideoConnCount(None) => {
            let n = crate::server::AUTHED_CONNS
                .lock()
                .unwrap()
                .iter()
                .filter(|x| x.conn_type == crate::server::AuthConnType::Remote)
                .count();
            allow_err!(stream.send(&Data::VideoConnCount(Some(n))).await);
        }
        Data::Config((name, value)) => match value {
            None => {
                let value;
                if name == "id" {
                    value = Some(Config::get_id());
                } else if name == "temporary-password" {
                    value = Some(password::temporary_password());
                } else if name == "permanent-password-storage-and-salt" {
                    let (storage, salt) = Config::get_local_permanent_password_storage_and_salt();
                    value = Some(storage + "\n" + &salt);
                } else if name == "permanent-password-set" {
                    value = Some(if Config::has_permanent_password() {
                        "Y".to_owned()
                    } else {
                        "N".to_owned()
                    });
                } else if name == "permanent-password-is-preset" {
                    value = Some(if Config::is_using_preset_password() {
                        "Y".to_owned()
                    } else {
                        "N".to_owned()
                    });
                } else if name == "salt" {
                    value = Some(Config::get_salt());
                } else if name == "rendezvous_server" {
                    value = Some(format!(
                        "{},{}",
                        Config::get_rendezvous_server(),
                        Config::get_rendezvous_servers().join(",")
                    ));
                } else if name == "rendezvous_servers" {
                    value = Some(Config::get_rendezvous_servers().join(","));
                } else if name == "fingerprint" {
                    value = if Config::get_key_confirmed() {
                        Some(crate::common::pk_to_fingerprint(Config::get_key_pair().1))
                    } else {
                        None
                    };
                } else if name == "hide_cm" {
                    value = if crate::common::is_custom_client() {
                        Some(hbb_common::password_security::hide_cm().to_string())
                    } else {
                        None
                    };
                } else if name == "unlock-pin" {
                    value = Some(Config::get_unlock_pin());
                } else if name == "trusted-devices" {
                    value = Some(Config::get_trusted_devices_json());
                } else {
                    value = None;
                }
                allow_err!(stream.send(&Data::Config((name, value))).await);
            }
            Some(value) => {
                let mut updated = true;
                if name == "id" {
                    // An empty id would wipe the local id and unconfirm the key (cf. #15626).
                    if value.is_empty() {
                        log::warn!("Ignoring empty id write over IPC");
                        updated = false;
                    } else {
                        Config::set_key_confirmed(false);
                        Config::set_id(&value);
                    }
                } else if name == "temporary-password" {
                    password::update_temporary_password();
                } else if name == "permanent-password" {
                    if Config::is_disable_change_permanent_password() {
                        log::warn!("Changing permanent password is disabled");
                        updated = false;
                    } else {
                        updated = Config::set_permanent_password(&value);
                    }
                    // Explicitly ACK/NACK permanent-password writes. This allows UIs/FFI to
                    // distinguish "accepted by daemon" vs "IPC send succeeded" without
                    // reading back any secret.
                    let ack = if updated { "Y" } else { "N" }.to_owned();
                    allow_err!(stream.send(&Data::Config((name.clone(), Some(ack)))).await);
                } else if name == "salt" {
                    Config::set_salt(&value);
                } else if name == "unlock-pin" {
                    Config::set_unlock_pin(&value);
                } else {
                    return;
                }
                if updated {
                    log::info!("{} updated", name);
                }
            }
        },
        Data::Options(value) => match value {
            None => {
                let v = Config::get_options();
                allow_err!(stream.send(&Data::Options(Some(v))).await);
            }
            Some(value) => {
                let _chk = CheckIfRestart::new();
                let _nat = CheckTestNatType::new();
                Config::set_options(value);
                allow_err!(stream.send(&Data::Options(None)).await);
            }
        },
        Data::NatType(_) => {
            let t = Config::get_nat_type();
            allow_err!(stream.send(&Data::NatType(Some(t))).await);
        }
        Data::SyncConfig(Some(configs)) => {
            let (config, config2) = *configs;
            let _chk = CheckIfRestart::new();
            Config::set(config);
            Config2::set(config2);
            allow_err!(stream.send(&Data::SyncConfig(None)).await);
        }
        Data::SyncConfig(None) => {
            allow_err!(
                stream
                    .send(&Data::SyncConfig(Some(
                        (Config::get(), Config2::get()).into()
                    )))
                    .await
            );
        }
        Data::SyncWinCpuUsage(None) => {
            allow_err!(
                stream
                    .send(&Data::SyncWinCpuUsage(
                        base::platform::windows::cpu_uage_one_minute()
                    ))
                    .await
            );
        }
        Data::TestRendezvousServer => {
            crate::test_rendezvous_server();
        }
        Data::Deployed => {
            crate::rendezvous_mediator::NEEDS_DEPLOY.store(false, Ordering::SeqCst);
            crate::rendezvous_mediator::RendezvousMediator::restart();
        }
        Data::SwitchSidesRequest(id) => {
            let uuid = uuid::Uuid::new_v4();
            crate::server::insert_switch_sides_uuid(id, uuid.clone());
            allow_err!(
                stream
                    .send(&Data::SwitchSidesRequest(uuid.to_string()))
                    .await
            );
        }
        Data::SwitchSidesUuid(uuid, id, action, None) => {
            let allowed = uuid
                .parse::<uuid::Uuid>()
                .map(|uuid| match action {
                    SwitchSidesUuidAction::Check => {
                        crate::server::has_pending_switch_sides_uuid(&id, &uuid)
                    }
                    SwitchSidesUuidAction::Consume => {
                        crate::server::claim_pending_switch_sides_uuid(&id, &uuid)
                    }
                })
                .unwrap_or(false);
            allow_err!(
                stream
                    .send(&Data::SwitchSidesUuid(uuid, id, action, Some(allowed)))
                    .await
            );
        }
        Data::ControlledSessionCount(_) => {
            allow_err!(
                stream
                    .send(&Data::ControlledSessionCount(
                        crate::Connection::alive_conns().len()
                    ))
                    .await
            );
        }
        #[cfg(feature = "hwcodec")]
        Data::CheckHwcodec => {
            scrap::hwcodec::start_check_process();
        }
        #[cfg(feature = "hwcodec")]
        Data::HwCodecConfig(c) => {
            match c {
                None => {
                    let v = match scrap::hwcodec::HwCodecConfig::get_set_value() {
                        Some(v) => Some(serde_json::to_string(&v).unwrap_or_default()),
                        None => None,
                    };
                    allow_err!(stream.send(&Data::HwCodecConfig(v)).await);
                }
                Some(v) => {
                    // --server and portable
                    scrap::hwcodec::HwCodecConfig::set(v);
                }
            }
        }
        Data::WaylandScreencastRestoreToken((key, value)) => {
            let v = if value == "get" {
                let opt = get_local_option(key.clone());
                {
                    Some(opt)
                }
            } else if value == "clear" {
                set_local_option(key.clone(), "".to_owned());
                // The session slot is held while a portal request waits for the user, and this ipc
                // listener runs on one thread: wait for the slot off it, then close what the
                // request brings back.
                Some("".to_owned())
            } else {
                None
            };
            if let Some(v) = v {
                allow_err!(
                    stream
                        .send(&Data::WaylandScreencastRestoreToken((key, v)))
                        .await
                );
            }
        }
        Data::RemoveTrustedDevices(v) => {
            Config::remove_trusted_devices(&v);
        }
        Data::ClearTrustedDevices => {
            Config::clear_trusted_devices();
        }
        Data::ControlPermissionsRemoteModify(_) => {
            use hbb_common::rendezvous_proto::control_permissions::Permission;
            let state =
                crate::server::get_control_permission_state(Permission::remote_modify, true);
            allow_err!(
                stream
                    .send(&Data::ControlPermissionsRemoteModify(state))
                    .await
            );
        }
        Data::FileTransferEnabledState(_) => {
            use hbb_common::rendezvous_proto::control_permissions::Permission;
            let state = crate::server::get_control_permission_state(Permission::file, false);
            let enabled = state.unwrap_or_else(|| {
                crate::server::Connection::is_permission_enabled_locally(
                    keys::OPTION_ENABLE_FILE_TRANSFER,
                )
            });
            allow_err!(
                stream
                    .send(&Data::FileTransferEnabledState(Some(enabled)))
                    .await
            );
        }
        _ => {}
    };
}

pub(crate) fn generate_one_time_ipc_token() -> ResultType<String> {
    use hbb_common::rand::{rngs::OsRng, RngCore as _};
    use std::fmt::Write as _;

    let mut random_bytes = [0u8; IPC_TOKEN_RANDOM_BYTES];
    let mut rng = OsRng;
    rng.try_fill_bytes(&mut random_bytes).map_err(|err| {
        hbb_common::anyhow::anyhow!(
            "failed to generate portable service ipc token from OsRng: {}",
            err
        )
    })?;

    let mut token = String::with_capacity(IPC_TOKEN_LEN);
    for byte in random_bytes {
        let _ = write!(token, "{:02x}", byte);
    }
    Ok(token)
}

pub(crate) fn constant_time_ipc_token_eq(expected: &str, candidate: &str) -> bool {
    if expected.len() != IPC_TOKEN_LEN || candidate.len() != IPC_TOKEN_LEN {
        return false;
    }
    expected
        .as_bytes()
        .iter()
        .zip(candidate.as_bytes().iter())
        .fold(0u8, |diff, (left, right)| diff | (*left ^ *right))
        == 0
}

pub(crate) async fn portable_service_ipc_handshake_as_client<T>(
    stream: &mut ConnectionTmpl<T>,
    token: &str,
) -> ResultType<()>
where
    T: AsyncRead + AsyncWrite + std::marker::Unpin,
{
    stream
        .send(&Data::DataPortableService(DataPortableService::AuthToken(
            token.to_owned(),
        )))
        .await?;
    match stream
        .next_timeout(PORTABLE_SERVICE_IPC_HANDSHAKE_TIMEOUT_MS)
        .await?
    {
        Some(Data::DataPortableService(DataPortableService::AuthResult(true))) => Ok(()),
        Some(Data::DataPortableService(DataPortableService::AuthResult(false))) => {
            bail!("portable service ipc handshake was rejected by server")
        }
        Some(_) | None => bail!("portable service ipc handshake returned an unexpected response"),
    }
}

pub(crate) async fn portable_service_ipc_handshake_as_server<T, F>(
    stream: &mut ConnectionTmpl<T>,
    mut validate_token: F,
) -> ResultType<()>
where
    T: AsyncRead + AsyncWrite + std::marker::Unpin,
    // Token validators must use `constant_time_ipc_token_eq` or an equivalent
    // fixed-length comparison; this handshake is part of the privilege boundary.
    F: FnMut(&str) -> bool,
{
    let authorized = match stream
        .next_timeout(PORTABLE_SERVICE_IPC_HANDSHAKE_TIMEOUT_MS)
        .await?
    {
        Some(Data::DataPortableService(DataPortableService::AuthToken(token))) => {
            validate_token(&token)
        }
        Some(_) | None => false,
    };
    stream
        .send(&Data::DataPortableService(DataPortableService::AuthResult(
            authorized,
        )))
        .await?;
    if !authorized {
        bail!("portable service ipc handshake failed")
    }
    Ok(())
}

#[inline]
async fn connect_with_path(ms_timeout: u64, path: &str) -> ResultType<ConnectionTmpl<ConnClient>> {
    let client = timeout(ms_timeout, Endpoint::connect(path)).await??;
    Ok(ConnectionTmpl::new(client))
}




pub async fn connect(ms_timeout: u64, postfix: &str) -> ResultType<ConnectionTmpl<ConnClient>> {
    {
        let path = Config::ipc_path(postfix);
        connect_with_path(ms_timeout, &path).await
    }
}



pub struct ConnectionTmpl<T> {
    inner: Framed<T, BytesCodec>,
}

pub type Connection = ConnectionTmpl<Conn>;

impl<T> ConnectionTmpl<T>
where
    T: AsyncRead + AsyncWrite + std::marker::Unpin,
{
    pub fn new(conn: T) -> Self {
        Self {
            inner: Framed::new(conn, BytesCodec::new()),
        }
    }

    pub async fn send(&mut self, data: &Data) -> ResultType<()> {
        let v = serde_json::to_vec(data)?;
        self.inner.send(bytes::Bytes::from(v)).await?;
        Ok(())
    }

    async fn send_config(&mut self, name: &str, value: String) -> ResultType<()> {
        self.send(&Data::Config((name.to_owned(), Some(value))))
            .await
    }

    pub async fn next_timeout(&mut self, ms_timeout: u64) -> ResultType<Option<Data>> {
        Ok(timeout(ms_timeout, self.next()).await??)
    }

    pub async fn next_timeout2(&mut self, ms_timeout: u64) -> Option<ResultType<Option<Data>>> {
        if let Ok(x) = timeout(ms_timeout, self.next()).await {
            Some(x)
        } else {
            None
        }
    }

    pub async fn next(&mut self) -> ResultType<Option<Data>> {
        match self.inner.next().await {
            Some(res) => {
                let bytes = res?;
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    if let Ok(data) = serde_json::from_str::<Data>(s) {
                        return Ok(Some(data));
                    }
                }
                return Ok(None);
            }
            _ => {
                bail!("reset by the peer");
            }
        }
    }

    pub async fn send_raw(&mut self, data: Bytes) -> ResultType<()> {
        self.inner.send(data).await?;
        Ok(())
    }

    pub async fn next_raw(&mut self) -> ResultType<bytes::BytesMut> {
        match self.inner.next().await {
            Some(Ok(res)) => Ok(res),
            _ => {
                bail!("reset by the peer");
            }
        }
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn get_config(name: &str) -> ResultType<Option<String>> {
    get_config_async(name, 1_000).await
}

async fn get_config_async(name: &str, ms_timeout: u64) -> ResultType<Option<String>> {
    let mut c = connect(ms_timeout, "").await?;
    c.send(&Data::Config((name.to_owned(), None))).await?;
    if let Some(Data::Config((name2, value))) = c.next_timeout(ms_timeout).await? {
        if name == name2 {
            return Ok(value);
        }
    }
    return Ok(None);
}

pub async fn set_config_async(name: &str, value: String) -> ResultType<()> {
    let mut c = connect(1000, "").await?;
    c.send_config(name, value).await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn set_data(data: &Data) -> ResultType<()> {
    set_data_async(data).await
}

async fn set_data_async(data: &Data) -> ResultType<()> {
    let mut c = connect(1000, "").await?;
    c.send(data).await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn set_config(name: &str, value: String) -> ResultType<()> {
    set_config_async(name, value).await
}

pub fn update_temporary_password() -> ResultType<()> {
    set_config("temporary-password", "".to_owned())
}

fn apply_permanent_password_storage_and_salt_payload(payload: Option<&str>) -> ResultType<()> {
    let Some(payload) = payload else {
        return Ok(());
    };
    let Some((storage, salt)) = payload.split_once('\n') else {
        bail!("Invalid permanent-password-storage-and-salt payload");
    };

    Config::set_permanent_password_storage_for_sync(storage, salt)?;
    Ok(())
}

pub fn sync_permanent_password_storage_from_daemon() -> ResultType<()> {
    let v = get_config("permanent-password-storage-and-salt")?;
    apply_permanent_password_storage_and_salt_payload(v.as_deref())
}

async fn sync_permanent_password_storage_from_daemon_async() -> ResultType<()> {
    let ms_timeout = 1_000;
    let v = get_config_async("permanent-password-storage-and-salt", ms_timeout).await?;
    apply_permanent_password_storage_and_salt_payload(v.as_deref())
}

pub fn is_permanent_password_set() -> bool {
    match get_config("permanent-password-set") {
        Ok(Some(v)) => {
            let v = v.trim();
            return v == "Y";
        }
        Ok(None) => {
            // No response/value (timeout).
        }
        Err(_) => {
            // Connection error.
        }
    }
    log::warn!("Failed to query permanent password state from daemon");
    false
}

pub fn is_permanent_password_preset() -> bool {
    if let Ok(Some(v)) = get_config("permanent-password-is-preset") {
        let v = v.trim();
        return v == "Y";
    }
    false
}

pub fn get_fingerprint() -> String {
    get_config("fingerprint")
        .unwrap_or_default()
        .unwrap_or_default()
}

pub fn set_permanent_password(v: String) -> ResultType<()> {
    if Config::is_disable_change_permanent_password() {
        bail!("Changing permanent password is disabled");
    }
    if set_permanent_password_with_ack(v)? {
        Ok(())
    } else {
        bail!("Changing permanent password was rejected by daemon");
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn set_permanent_password_with_ack(v: String) -> ResultType<bool> {
    set_permanent_password_with_ack_async(v).await
}

async fn set_permanent_password_with_ack_async(v: String) -> ResultType<bool> {
    // The daemon ACK/NACK is expected quickly since it applies the config in-process.
    let ms_timeout = 1_000;
    let mut c = connect(ms_timeout, "").await?;
    c.send_config("permanent-password", v).await?;
    if let Some(Data::Config((name2, Some(v)))) = c.next_timeout(ms_timeout).await? {
        if name2 == "permanent-password" {
            let v = v.trim();
            let ok = v == "Y";
            if ok {
                // Ensure the hashed permanent password storage is written to the user config file.
                // This sync must not affect the daemon ACK outcome.
                if let Err(err) = sync_permanent_password_storage_from_daemon_async().await {
                    log::warn!("Failed to sync permanent password storage from daemon: {err}");
                }
            }
            return Ok(ok);
        }
    }
    Ok(false)
}

pub fn set_unlock_pin(v: String, translate: bool) -> ResultType<()> {
    let v = v.trim().to_owned();
    let min_len = 4;
    let max_len = crate::ui_interface::max_encrypt_len();
    let len = v.chars().count();
    if !v.is_empty() {
        if len < min_len {
            let err = if translate {
                crate::lang::translate(
                    "Requires at least {".to_string() + &format!("{min_len}") + "} characters",
                )
            } else {
                // Sometimes, translated can't show normally in command line
                format!("Requires at least {} characters", min_len)
            };
            bail!(err);
        }
        if len > max_len {
            bail!("No more than {max_len} characters");
        }
    }
    Config::set_unlock_pin(&v);
    set_config("unlock-pin", v)
}

pub fn get_unlock_pin() -> String {
    if let Ok(Some(v)) = get_config("unlock-pin") {
        Config::set_unlock_pin(&v);
        v
    } else {
        Config::get_unlock_pin()
    }
}

pub fn get_trusted_devices() -> String {
    if let Ok(Some(v)) = get_config("trusted-devices") {
        v
    } else {
        Config::get_trusted_devices_json()
    }
}

pub fn remove_trusted_devices(hwids: Vec<Bytes>) {
    Config::remove_trusted_devices(&hwids);
    allow_err!(set_data(&Data::RemoveTrustedDevices(hwids)));
}

pub fn clear_trusted_devices() {
    Config::clear_trusted_devices();
    allow_err!(set_data(&Data::ClearTrustedDevices));
}

pub fn get_id() -> String {
    // An empty id may come from a process that took over the main IPC with a
    // config scope that has no id yet (e.g. a user GUI that became the server
    // while the installed service was restarting). Treat it as no answer,
    // otherwise the empty id is adopted below and wipes the local one.
    if let Ok(Some(v)) = get_config("id") {
        if !v.is_empty() {
            // update salt also, so that next time reinstallation not causing first-time auto-login failure
            if let Ok(Some(v2)) = get_config("salt") {
                Config::set_salt(&v2);
            }
            if v != Config::get_id() {
                Config::set_key_confirmed(false);
                Config::set_id(&v);
            }
            return v;
        }
    }
    Config::get_id()
}

pub async fn get_rendezvous_server(ms_timeout: u64) -> (String, Vec<String>) {
    if let Ok(Some(v)) = get_config_async("rendezvous_server", ms_timeout).await {
        let mut urls = v.split(",");
        let a = urls.next().unwrap_or_default().to_owned();
        let b: Vec<String> = urls.map(|x| x.to_owned()).collect();
        (a, b)
    } else {
        (
            Config::get_rendezvous_server(),
            Config::get_rendezvous_servers(),
        )
    }
}

async fn get_options_(ms_timeout: u64) -> ResultType<HashMap<String, String>> {
    let mut c = connect(ms_timeout, "").await?;
    c.send(&Data::Options(None)).await?;
    if let Some(Data::Options(Some(value))) = c.next_timeout(ms_timeout).await? {
        Config::set_options(value.clone());
        Ok(value)
    } else {
        Ok(Config::get_options())
    }
}

pub async fn get_options_async() -> HashMap<String, String> {
    get_options_(1000).await.unwrap_or(Config::get_options())
}

#[tokio::main(flavor = "current_thread")]
pub async fn get_options() -> HashMap<String, String> {
    get_options_async().await
}

pub async fn get_option_async(key: &str) -> String {
    if let Some(v) = get_options_async().await.get(key) {
        v.clone()
    } else {
        "".to_owned()
    }
}

pub fn set_option(key: &str, value: &str) {
    let mut options = get_options();
    if value.is_empty() {
        options.remove(key);
    } else {
        options.insert(key.to_owned(), value.to_owned());
    }
    set_options(options).ok();
}

#[tokio::main(flavor = "current_thread")]
pub async fn set_options(value: HashMap<String, String>) -> ResultType<()> {
    let _nat = CheckTestNatType::new();
    if let Ok(mut c) = connect(1000, "").await {
        c.send(&Data::Options(Some(value.clone()))).await?;
        // do not put below before connect, because we need to check should_exit
        c.next_timeout(1000).await.ok();
    }
    Config::set_options(value);
    Ok(())
}

#[inline]
async fn get_nat_type_(ms_timeout: u64) -> ResultType<i32> {
    let mut c = connect(ms_timeout, "").await?;
    c.send(&Data::NatType(None)).await?;
    if let Some(Data::NatType(Some(value))) = c.next_timeout(ms_timeout).await? {
        Config::set_nat_type(value);
        Ok(value)
    } else {
        Ok(Config::get_nat_type())
    }
}

pub async fn get_nat_type(ms_timeout: u64) -> i32 {
    get_nat_type_(ms_timeout)
        .await
        .unwrap_or(Config::get_nat_type())
}

pub async fn get_rendezvous_servers(ms_timeout: u64) -> Vec<String> {
    if let Ok(Some(v)) = get_config_async("rendezvous_servers", ms_timeout).await {
        return v.split(',').map(|x| x.to_owned()).collect();
    }
    return Config::get_rendezvous_servers();
}

#[inline]
async fn get_socks_(ms_timeout: u64) -> ResultType<Option<config::Socks5Server>> {
    let mut c = connect(ms_timeout, "").await?;
    c.send(&Data::Socks(None)).await?;
    if let Some(Data::Socks(value)) = c.next_timeout(ms_timeout).await? {
        Config::set_socks(value.clone());
        Ok(value)
    } else {
        Ok(Config::get_socks())
    }
}

pub async fn get_socks_async(ms_timeout: u64) -> Option<config::Socks5Server> {
    get_socks_(ms_timeout).await.unwrap_or(Config::get_socks())
}

#[tokio::main(flavor = "current_thread")]
pub async fn get_socks() -> Option<config::Socks5Server> {
    get_socks_async(1_000).await
}

#[tokio::main(flavor = "current_thread")]
pub async fn set_socks(value: config::Socks5Server) -> ResultType<()> {
    let _nat = CheckTestNatType::new();
    Config::set_socks(if value.proxy.is_empty() {
        None
    } else {
        Some(value.clone())
    });
    connect(1_000, "")
        .await?
        .send(&Data::Socks(Some(value)))
        .await?;
    Ok(())
}

async fn get_socks_ws_(ms_timeout: u64) -> ResultType<(Option<config::Socks5Server>, String)> {
    let mut c = connect(ms_timeout, "").await?;
    c.send(&Data::SocksWs(None)).await?;
    if let Some(Data::SocksWs(Some(value))) = c.next_timeout(ms_timeout).await? {
        Config::set_socks(value.0.clone());
        Config::set_option(OPTION_ALLOW_WEBSOCKET.to_string(), value.1.clone());
        Ok(*value)
    } else {
        Ok((
            Config::get_socks(),
            Config::get_option(OPTION_ALLOW_WEBSOCKET),
        ))
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn get_socks_ws() -> (Option<config::Socks5Server>, String) {
    get_socks_ws_(1_000).await.unwrap_or((
        Config::get_socks(),
        Config::get_option(OPTION_ALLOW_WEBSOCKET),
    ))
}

pub fn get_proxy_status() -> bool {
    Config::get_socks().is_some()
}
#[tokio::main(flavor = "current_thread")]
pub async fn test_rendezvous_server() -> ResultType<()> {
    let mut c = connect(1000, "").await?;
    c.send(&Data::TestRendezvousServer).await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn notify_deployed() -> ResultType<()> {
    let mut c = connect(1000, "").await?;
    c.send(&Data::Deployed).await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn send_url_scheme(url: String) -> ResultType<()> {
    connect(1_000, "_url")
        .await?
        .send(&Data::UrlLink(url))
        .await?;
    Ok(())
}

// Emit `close` events to ipc.
pub fn close_all_instances() -> ResultType<bool> {
    match crate::ipc::send_url_scheme(IPC_ACTION_CLOSE.to_owned()) {
        Ok(_) => Ok(true),
        Err(err) => Err(err),
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn connect_to_user_session(usid: Option<u32>) -> ResultType<()> {
    let mut stream = crate::ipc::connect_service(1000).await?;
    timeout(1000, stream.send(&crate::ipc::Data::UserSid(usid))).await??;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn notify_server_to_check_hwcodec() -> ResultType<()> {
    connect(1_000, "").await?.send(&&Data::CheckHwcodec).await?;
    Ok(())
}

#[cfg(feature = "hwcodec")]
#[tokio::main(flavor = "current_thread")]
pub async fn get_hwcodec_config_from_server() -> ResultType<()> {
    if !scrap::codec::enable_hwcodec_option() || scrap::hwcodec::HwCodecConfig::already_set() {
        return Ok(());
    }
    let mut c = connect(50, "").await?;
    c.send(&Data::HwCodecConfig(None)).await?;
    if let Some(Data::HwCodecConfig(v)) = c.next_timeout(50).await? {
        match v {
            Some(v) => {
                scrap::hwcodec::HwCodecConfig::set(v);
                return Ok(());
            }
            None => {
                bail!("hwcodec config is none");
            }
        }
    }
    bail!("failed to get hwcodec config");
}

#[cfg(feature = "hwcodec")]
#[tokio::main(flavor = "current_thread")]
pub async fn hwcodec_process() {
    let s = scrap::hwcodec::check_available_hwcodec();
    for _ in 0..5 {
        match crate::ipc::connect(1000, "").await {
            Ok(mut conn) => {
                match conn
                    .send(&crate::ipc::Data::HwCodecConfig(Some(s.clone())))
                    .await
                {
                    Ok(()) => {
                        log::info!("send ok");
                        break;
                    }
                    Err(e) => {
                        log::error!("send failed: {e:?}");
                    }
                }
            }
            Err(e) => {
                log::error!("connect failed: {e:?}");
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn get_wayland_screencast_restore_token(key: String) -> ResultType<String> {
    let v = handle_wayland_screencast_restore_token(key, "get".to_owned()).await?;
    Ok(v.unwrap_or_default())
}

#[tokio::main(flavor = "current_thread")]
pub async fn clear_wayland_screencast_restore_token(key: String) -> ResultType<bool> {
    if let Some(v) = handle_wayland_screencast_restore_token(key, "clear".to_owned()).await? {
        return Ok(v.is_empty());
    }
    return Ok(false);
}

async fn handle_wayland_screencast_restore_token(
    key: String,
    value: String,
) -> ResultType<Option<String>> {
    let ms_timeout = 1_000;
    let mut c = connect(ms_timeout, "").await?;
    c.send(&Data::WaylandScreencastRestoreToken((key, value)))
        .await?;
    if let Some(Data::WaylandScreencastRestoreToken((_key, v))) = c.next_timeout(ms_timeout).await?
    {
        return Ok(Some(v));
    }
    return Ok(None);
}

#[cfg(test)]
mod test {





}
