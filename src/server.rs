use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, RwLock, Weak},
    time::Duration,
};

use bytes::Bytes;

pub use connection::*;
use hbb_common::config::Config2;
use hbb_common::tcp;
use hbb_common::{
    allow_err,
    anyhow::Context,
    bail,
    config::{self, Config, CONNECT_TIMEOUT, RENDEZVOUS_PORT},
    log,
    protobuf::{Enum, Message as _},
    rendezvous_proto::*,
    sodiumoxide::crypto::{box_, sign},
    timeout, tokio,
    ResultType, Stream,
};
use base::{config::keys::OPTION_DIRECT_SERVER, message_proto::*};
use service::ServiceTmpl;
use service::{EmptyExtraFieldService, GenericService, Service, Subscriber};
use video_service::VideoSource;

use crate::ipc::Data;

pub mod audio_service;
mod clipboard_service;
pub mod input_service;

mod connection;
pub mod display_service;
pub mod portable_service;
mod service;
mod video_qos;
pub mod video_service;

pub type Childs = Arc<Mutex<Vec<std::process::Child>>>;
type ConnMap = HashMap<i32, ConnInner>;

#[derive(Clone, Default)]
pub struct ConnectionMeta {
    pub control_permissions: Option<ControlPermissions>,
    pub controlled_context: Option<ControlledContext>,
}


lazy_static::lazy_static! {
    pub static ref CHILD_PROCESS: Childs = Default::default();
}

pub struct Server {
    connections: ConnMap,
    services: HashMap<String, Box<dyn Service>>,
    id_count: i32,
}

pub type ServerPtr = Arc<RwLock<Server>>;
pub type ServerPtrWeak = Weak<RwLock<Server>>;

#[cfg(test)]
pub fn new_for_test() -> ServerPtr {
    Arc::new(RwLock::new(Server {
        connections: HashMap::new(),
        services: HashMap::new(),
        id_count: 1000,
    }))
}

pub fn new() -> ServerPtr {
    let mut server = Server {
        connections: HashMap::new(),
        services: HashMap::new(),
        id_count: hbb_common::rand::random::<i32>() % 1000 + 1000, // ensure positive
    };
    server.add_service(Box::new(audio_service::new()));
    {
        server.add_service(Box::new(display_service::new()));
        server.add_service(Box::new(clipboard_service::new(
            clipboard_service::NAME.to_owned(),
        )));
    }
    {
        if !display_service::capture_cursor_embedded() {
            server.add_service(Box::new(input_service::new_cursor()));
            server.add_service(Box::new(input_service::new_pos()));
            server.add_service(Box::new(input_service::new_window_focus()));
        }
    }
    Arc::new(RwLock::new(server))
}

pub async fn create_tcp_connection(
    server: ServerPtr,
    stream: Stream,
    addr: SocketAddr,
    secure: bool,
    meta: ConnectionMeta,
) -> ResultType<()> {
    let mut stream = stream;
    // The address the connection layer keys on, whitelist and admission alike.
    let addr = hbb_common::try_into_v4(addr);
    let id = server.write().unwrap().get_new_id();
    // Admitted before the identity handshake, so a peer that stalls in it, or after it without
    // logging in, holds its place the whole time; an address over its share is turned away.
    let Some(unauthorized) = admit_unauthorized(id, addr.ip()) else {
        bail!("too many unauthenticated connections from {}", addr.ip());
    };
    // Before the handshake, so its read is bounded too; lifted again at authorization.
    stream.set_max_packet_length(MAX_UNAUTHORIZED_MESSAGE);
    tokio::select! {
        handshake = identity_handshake(&mut stream, secure) => handshake?,
        _ = unauthorized.evicted() => {
            bail!("evicted to make room for a newer unauthenticated connection");
        }
    }

    Connection::start(
        addr,
        stream,
        id,
        Arc::downgrade(&server),
        meta,
        unauthorized,
    )
    .await;
    Ok(())
}

/// Our signed identity goes out and, when `secure`, the controller's reply keys `stream`.
/// Separate so it can be raced against the connection's eviction.
async fn identity_handshake(stream: &mut Stream, secure: bool) -> ResultType<()> {
    let (sk, pk) = Config::get_key_pair();
    if secure && pk.len() == sign::PUBLICKEYBYTES && sk.len() == sign::SECRETKEYBYTES {
        let mut sk_ = [0u8; sign::SECRETKEYBYTES];
        sk_[..].copy_from_slice(&sk);
        let sk = sign::SecretKey(sk_);
        let mut msg_out = Message::new();
        let (our_pk_b, our_sk_b) = box_::gen_keypair();
        // On a WebRTC transport, bind our DTLS certificate fingerprint to our signed identity so
        // the controller can verify the DTLS channel it negotiated actually terminates at us
        // (not a rendezvous/relay that swapped the SDP fingerprint). Empty on other transports.
        // Fail immediately on WebRTC if the local fingerprint is unavailable: signing "" would
        // only make the client fail-closed after a wasted round-trip.
        let dtls_fingerprint = stream.dtls_fingerprint(true).await.unwrap_or_default();
        if stream.is_webrtc() && dtls_fingerprint.is_empty() {
            bail!("WebRTC local DTLS fingerprint unavailable");
        }
        // A WebRTC stream is encrypted by DTLS and takes no stream key of its own, split or
        // not, so what it advertises is what it runs: 0. Saying 1 there would have a future
        // controller that splits keys on WebRTC agree on a version this side never applies.
        let advertised = if stream.is_webrtc() {
            0
        } else {
            tcp::KX_VERSION_LATEST
        };
        msg_out.set_signed_id(SignedId {
            id: sign::sign(
                &IdPk {
                    id: Config::get_id(),
                    pk: Bytes::from(our_pk_b.0.to_vec()),
                    dtls_fingerprint,
                    kx_version: advertised,
                    ..Default::default()
                }
                .write_to_bytes()
                .unwrap_or_default(),
                &sk,
            )
            .into(),
            ..Default::default()
        });
        timeout(CONNECT_TIMEOUT, stream.send(&msg_out)).await??;
        match timeout(CONNECT_TIMEOUT, stream.next()).await? {
            Some(res) => {
                let bytes = res?;
                if let Ok(msg_in) = Message::parse_from_bytes(&bytes) {
                    if let Some(message::Union::PublicKey(pk)) = msg_in.union {
                        if pk.asymmetric_value.len() == box_::PUBLICKEYBYTES {
                            let key = tcp::Encrypt::decode(
                                &pk.symmetric_value,
                                &pk.asymmetric_value,
                                &our_sk_b,
                            )?;
                            if pk.kx_version > advertised {
                                bail!(
                                    "Handshake failed: key exchange version {} not offered, {} was",
                                    pk.kx_version,
                                    advertised
                                );
                            }
                            stream.set_negotiated_key(
                                key,
                                false,
                                &tcp::KxTranscript {
                                    initiator_pk: &pk.asymmetric_value,
                                    responder_pk: &our_pk_b.0,
                                    advertised,
                                    picked: pk.kx_version,
                                },
                            )?;
                        } else if pk.asymmetric_value.is_empty() {
                            Config::set_key_confirmed(false);
                            log::info!("Force to update pk");
                        } else {
                            bail!("Handshake failed: invalid public sign key length from peer");
                        }
                    } else {
                        log::error!("Handshake failed: invalid message type");
                    }
                } else {
                    bail!("Handshake failed: invalid message format");
                }
            }
            None => {
                bail!("Failed to receive public key");
            }
        }
    }

    Ok(())
}

impl Server {
    fn is_video_service_name(name: &str) -> bool {
        name.starts_with(VideoSource::Monitor.service_name_prefix())
    }

    pub fn try_add_monitor_service(&mut self, display_idx: usize) {
        let monitor_service_name =
            video_service::get_service_name(VideoSource::Monitor, display_idx);
        if !self.contains(&monitor_service_name) {
            self.add_service(Box::new(video_service::new(
                VideoSource::Monitor,
                display_idx,
            )));
        }
    }

    pub fn add_monitor_connection(
        &mut self,
        conn: ConnInner,
        noperms: &Vec<&'static str>,
        display_idx: usize,
    ) {
        let monitor_service_name =
            video_service::get_service_name(VideoSource::Monitor, display_idx);
        for s in self.services.values() {
            let name = s.name();
            if Self::is_video_service_name(&name) && name != monitor_service_name {
                continue;
            }
            if !noperms.contains(&(&name as _)) {
                s.on_subscribe(conn.clone());
            }
        }
        self.connections.insert(conn.id(), conn);
    }

    pub fn remove_connection(&mut self, conn: &ConnInner) {
        for s in self.services.values() {
            s.on_unsubscribe(conn.id());
        }
        self.connections.remove(&conn.id());
    }

    pub fn close_connections(&mut self) {
        let conn_inners: Vec<_> = self.connections.values_mut().collect();
        for c in conn_inners {
            let mut misc = Misc::new();
            misc.set_stop_service(true);
            let mut msg = Message::new();
            msg.set_misc(misc);
            c.send(Arc::new(msg));
        }
    }

    fn add_service(&mut self, service: Box<dyn Service>) {
        let name = service.name();
        self.services.insert(name, service);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.services.contains_key(name)
    }

    pub fn subscribe(&mut self, name: &str, conn: ConnInner, sub: bool) {
        if let Some(s) = self.services.get(name) {
            if s.is_subed(conn.id()) == sub {
                return;
            }
            if sub {
                s.on_subscribe(conn.clone());
            } else {
                s.on_unsubscribe(conn.id());
            }
        }
    }

    // get a new unique id
    pub fn get_new_id(&mut self) -> i32 {
        self.id_count += 1;
        self.id_count
    }

    pub fn set_video_service_opt(
        &self,
        display: Option<(VideoSource, usize)>,
        opt: &str,
        value: &str,
    ) {
        for (k, v) in self.services.iter() {
            if let Some((source, display)) = display {
                if k != &video_service::get_service_name(source, display) {
                    continue;
                }
            }

            if Self::is_video_service_name(k) {
                v.set_option(opt, value);
            }
        }
    }

    fn get_subbed_displays_count(&self, conn_id: i32) -> usize {
        self.services
            .keys()
            .filter(|k| {
                Self::is_video_service_name(k)
                    && self
                        .services
                        .get(*k)
                        .map(|s| s.is_subed(conn_id))
                        .unwrap_or(false)
            })
            .count()
    }

    fn capture_displays(
        &mut self,
        conn: ConnInner,
        source: VideoSource,
        displays: &[usize],
        include: bool,
        exclude: bool,
    ) {
        let displays = displays
            .iter()
            .map(|d| video_service::get_service_name(source, *d))
            .collect::<Vec<_>>();
        let keys = self.services.keys().cloned().collect::<Vec<_>>();
        for name in keys.iter() {
            if Self::is_video_service_name(&name) {
                if displays.contains(&name) {
                    if include {
                        self.subscribe(&name, conn.clone(), true);
                    }
                } else {
                    if exclude {
                        self.subscribe(&name, conn.clone(), false);
                    }
                }
            }
        }
    }

}

impl Drop for Server {
    fn drop(&mut self) {
        for s in self.services.values() {
            s.join();
        }
    }
}

pub fn check_zombie() {
    std::thread::spawn(|| loop {
        let mut lock = CHILD_PROCESS.lock().unwrap();
        let mut i = 0;
        while i != lock.len() {
            let c = &mut (*lock)[i];
            if let Ok(Some(_)) = c.try_wait() {
                lock.remove(i);
            } else {
                i += 1;
            }
        }
        drop(lock);
        std::thread::sleep(Duration::from_millis(100));
    });
}


/// Start the host server that allows the remote peer to control the current machine.
///
/// # Arguments
///
/// * `is_server` - Whether the current client is definitely the server.
/// If true, the server will be started.
/// Otherwise, client will check if there's already a server and start one if not.
/// * `no_server` - If `is_server` is false, whether to start a server if not found.
#[tokio::main]
pub async fn start_server(is_server: bool, no_server: bool) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        base::platform::windows::start_cpu_performance_monitor();
    });

    if is_server {
        crate::common::set_server_running(true);
        std::thread::spawn(move || {
            if let Err(err) = crate::ipc::start("") {
                log::error!("Failed to start ipc: {}", err);
                if crate::is_server() {
                    log::error!("ipc is occupied by another process, try kill it");
                    std::thread::spawn(stop_main_window_process).join().ok();
                }
                std::process::exit(-1);
            }
        });
        // Warm the DRM availability cache before any client connects, so the first connection does
        // not race a cold `_drm` probe and ship an empty display list ("No displays" + retry).
        // X11 is skipped -- probing there makes the root service open DRM readers for a path this
        // session can never take -- but that decision belongs to `warm_availability`, which already
        // makes it, and NOT to this call site. Deciding it here is the same one-shot-at-startup
        // mistake the pre-warm had: `is_x11()` answers "x11" whenever loginctl cannot yet name the
        // seat0 session, which during a boot is exactly when this runs, and nothing revisits it --
        // so a Wayland host that came up slowly skipped the warm for the life of the process and
        // got back the cold-probe "No displays" symptom the warm exists to remove.
        input_service::fix_key_down_timeout_loop();
        #[cfg(feature = "hwcodec")]
        scrap::hwcodec::start_check_process();
        run_listeners().await;
    } else {
        match crate::ipc::connect(1000, "").await {
            Ok(mut conn) => {
                if conn.send(&Data::SyncConfig(None)).await.is_ok() {
                    if let Ok(Some(data)) = conn.next_timeout(1000).await {
                        match data {
                            Data::SyncConfig(Some(configs)) => {
                                let (config, config2) = *configs;
                                if Config::set(config) {
                                    log::info!("config synced");
                                }
                                if Config2::set(config2) {
                                    log::info!("config2 synced");
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(err) => {
                log::info!("server not started: {err:?}, no_server: {no_server}");
                if no_server {
                    hbb_common::sleep(1.0).await;
                    std::thread::spawn(|| start_server(false, true));
                } else {
                    log::info!("try start server");
                    std::thread::spawn(|| start_server(true, false));
                }
            }
        }
    }
}


/// Serves incoming sessions on the direct-access port and answers LAN discovery until the
/// process exits.
async fn run_listeners() {
    if config::is_outgoing_only() {
        loop {
            hbb_common::sleep(1.).await;
        }
    }
    check_zombie();
    let server = new();
    tokio::spawn(direct_server(server.clone()));
    std::thread::spawn(move || {
        allow_err!(crate::lan::start_listening());
    });
    scrap::codec::test_av1();
    loop {
        if is_service_stopped() {
            server.write().unwrap().close_connections();
        }
        hbb_common::sleep(1.).await;
    }
}

fn is_service_stopped() -> bool {
    config::option2bool("stop-service", &Config::get_option("stop-service"))
}

fn get_direct_port() -> i32 {
    let port = Config::get_option("direct-access-port")
        .parse::<i32>()
        .unwrap_or(0);
    if port <= 0 {
        RENDEZVOUS_PORT + 2
    } else {
        port
    }
}

async fn direct_server(server: ServerPtr) {
    let mut listener = None;
    let mut port = 0;
    loop {
        let disabled = !config::option2bool(
            OPTION_DIRECT_SERVER,
            &Config::get_option(OPTION_DIRECT_SERVER),
        ) || is_service_stopped();
        if !disabled && listener.is_none() {
            port = get_direct_port();
            match tcp::listen_any(port as _).await {
                Ok(l) => {
                    log::info!("Direct server listening on: {:?}", l.local_addr());
                    listener = Some(l);
                }
                Err(err) => {
                    log::error!(
                        "Failed to start direct server on port: {}, error: {}",
                        port,
                        err
                    );
                    while port == get_direct_port() {
                        hbb_common::sleep(1.).await;
                    }
                }
            }
        }
        if let Some(l) = listener.as_mut() {
            if disabled || port != get_direct_port() {
                log::info!("Exit direct access listen");
                listener = None;
                continue;
            }
            if let Ok(Ok((stream, addr))) = timeout(1000, l.accept()).await {
                stream.set_nodelay(true).ok();
                log::info!("direct access from {}", addr);
                let local_addr = stream
                    .local_addr()
                    .unwrap_or(Config::get_any_listen_addr(true));
                let server = server.clone();
                tokio::spawn(async move {
                    allow_err!(
                        create_tcp_connection(
                            server,
                            Stream::from(stream, local_addr),
                            addr,
                            false,
                            ConnectionMeta::default(),
                        )
                        .await
                    );
                });
            } else {
                hbb_common::sleep(0.1).await;
            }
        } else {
            hbb_common::sleep(1.).await;
        }
    }
}

#[tokio::main(flavor = "current_thread")]
pub async fn stop_main_window_process() {
    // this may also kill another --server process,
    // but --server usually can be auto restarted by --service, so it is ok
    if let Ok(mut conn) = crate::ipc::connect(1000, "").await {
        conn.send(&crate::ipc::Data::Close).await.ok();
    }
    {
        // in case above failure, e.g. zombie process
        if let Err(e) = crate::platform::try_kill_rustdesk_main_window_process() {
            log::error!("kill failed: {}", e);
        }
    }
}
