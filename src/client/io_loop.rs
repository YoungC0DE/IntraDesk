use crate::clipboard::{clipboard_listener, update_clipboard, ClipboardSide};
use crate::clipboard::CLIPBOARD_INTERVAL;
use crate::{
    client::{
        self, Client, Data, HostKey, Interface, MediaData, MediaSender, MILLI1, SEC30,
    },
    ui_session_interface::{InvokeUiSession, Session},
};

// Empirical no-data window before exposing the restart reconnect state to the UI.
// Restart msgbox text is kept as a legacy UI fallback; Flutter handles the type as a control event.
const RESTART_REMOTE_DEVICE_NO_DATA_TIMEOUT: Duration = Duration::from_secs(5);
use base::{
    fs::{
        self, can_enable_overwrite_detection, get_job, get_string, new_send_confirm,
        DigestCheckResult,
    },
    message_proto::{permission_info::Permission, *},
};
use clipboard::ContextSend;
use crossbeam_queue::ArrayQueue;
use hbb_common::{
    allow_err,
    config::{self, PeerConfig, TransferSerde},
    log,
    protobuf::Message as _,
    rendezvous_proto::ConnType,
    timeout,
    tokio::{
        self,
        sync::mpsc,
        time::{self, Duration, Instant},
    },
    Stream,
};
use hbb_common::{tokio::sync::Mutex as TokioMutex, ResultType};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, RwLock},
};

pub struct Remote<T: InvokeUiSession> {
    handler: Session<T>,
    audio_sender: MediaSender,
    receiver: mpsc::UnboundedReceiver<Data>,
    sender: mpsc::UnboundedSender<Data>,
    read_jobs: Vec<fs::TransferJob>,
    write_jobs: Vec<fs::TransferJob>,
    remove_jobs: HashMap<i32, RemoveJob>,
    timer: crate::RustDeskInterval,
    last_update_jobs_status: (Instant, HashMap<i32, u64>),
    // Set after PeerInfo for this round, not when the transport connects.
    is_connected: bool,
    // Whether the scheduled initial snapshot may still be sent.
    // The connection loop clears this when handling its result or a live clipboard update.
    initial_clipboard_pending: bool,
    first_frame: bool,
    client_conn_id: i32, // used for file clipboard
    elevation_requested: bool,
    video_threads: HashMap<usize, VideoThread>,
    sent_close_reason: bool,
    cursor_dedupe: CursorDedupe,
}

impl<T: InvokeUiSession> Remote<T> {
    pub fn new(
        handler: Session<T>,
        receiver: mpsc::UnboundedReceiver<Data>,
        sender: mpsc::UnboundedSender<Data>,
    ) -> Self {
        Self {
            handler,
            audio_sender: crate::client::start_audio_thread(),
            receiver,
            sender,
            read_jobs: Vec::new(),
            write_jobs: Vec::new(),
            remove_jobs: Default::default(),
            timer: crate::rustdesk_interval(time::interval(SEC30)),
            last_update_jobs_status: (Instant::now(), Default::default()),
            is_connected: false,
            initial_clipboard_pending: false,
            first_frame: false,
            client_conn_id: 0,
            elevation_requested: false,
            video_threads: Default::default(),
            sent_close_reason: false,
            cursor_dedupe: Default::default(),
        }
    }

    pub async fn io_loop(&mut self, round: u32) {
        let _file_clip_context_holder = {
            if self.handler.is_default() {
                // It is ok to call this function multiple times.
                ContextSend::enable(true);
                Some(crate::SimpleCallOnReturn {
                    b: true,
                    f: Box::new(|| {
                        // It's better to check if the peers are windows(support file copy&paste), but it's not necessary.
                        if !crate::flutter::sessions::has_sessions_running(ConnType::DEFAULT_CONN) {
                            ContextSend::enable(false);
                        };
                    }),
                })
            } else {
                None
            }
        };

        match Client::start(&self.handler.get_id(), self.handler.clone()).await {
            Ok((mut peer, host_key)) => {
                self.handler
                    .connection_round_state
                    .lock()
                    .unwrap()
                    .set_connected();
                self.handler.set_connection_type(peer.is_secured()); // flutter -> connection_ready
                match host_key {
                    HostKey::Known => {}
                    HostKey::New(key) => self.handler.lc.write().unwrap().set_host_key(key),
                    HostKey::Changed(key) => {
                        if !client::confirm_host_key_change(&self.handler, &mut self.receiver).await
                        {
                            self.send_close_reason(&mut peer, "").await;
                            self.handle_disconnected(round);
                            return;
                        }
                        self.handler.lc.write().unwrap().set_host_key(key);
                    }
                }

                // just build for now

                let (_tx_holder, rx) = mpsc::unbounded_channel();
                let mut rx_clip_client_holder = (Arc::new(TokioMutex::new(rx)), None);
                {
                    if self.handler.is_default() {
                        (self.client_conn_id, rx_clip_client_holder.0) =
                            clipboard::get_rx_cliprdr_client(&self.handler.get_id());
                        log::debug!("get cliprdr client for conn_id {}", self.client_conn_id);
                        let client_conn_id = self.client_conn_id;
                        rx_clip_client_holder.1 = Some(crate::SimpleCallOnReturn {
                            b: true,
                            f: Box::new(move || {
                                clipboard::remove_channel_by_conn_id(client_conn_id);
                            }),
                        });
                    };
                }
                let mut rx_clip_client = rx_clip_client_holder.0.lock().await;

                let mut status_timer =
                    crate::rustdesk_interval(time::interval(Duration::new(1, 0)));
                let mut fps_instant = Instant::now();

                let mut last_recv_time = Instant::now();

                loop {
                    tokio::select! {
                        res = peer.next() => {
                            if let Some(res) = res {
                                match res {
                                    Err(err) => {
                                        self.handler.on_establish_connection_error(err.to_string());
                                        break;
                                    }
                                    Ok(ref bytes) => {
                                        last_recv_time = Instant::now();
                                        if !self.handle_msg_from_peer(bytes, &mut peer).await {
                                            break
                                        }
                                    }
                                }
                            } else {
                                if self.handler.is_restarting_remote_device() {
                                    log::info!("Restart remote device");
                                    self.handler.msgbox("restarting", "Restarting remote device", "Connection in progress. Please wait.", "");
                                } else {
                                    log::info!("Reset by the peer");
                                    self.handler.msgbox("error", "Connection Error", "Reset by the peer", "");
                                }
                                break;
                            }
                        }
                        d = self.receiver.recv() => {
                            if let Some(d) = d {
                                if !self.handle_msg_from_ui(d, &mut peer).await {
                                    break;
                                }
                            }
                        }
                        _msg = rx_clip_client.recv() => {
                            #[cfg(any(target_os = "windows", feature = "unix-file-copy-paste"))]
                            self.handle_local_clipboard_msg(&mut peer, _msg).await;
                        }
                        _ = self.timer.tick() => {
                            if last_recv_time.elapsed() >= SEC30 {
                                self.handler.msgbox("error", "Connection Error", "Timeout", "");
                                break;
                            }
                            if !self.read_jobs.is_empty() {
                                if let Err(err) = fs::handle_read_jobs(&mut self.read_jobs, &mut peer).await {
                                    self.handler.msgbox("error", "Connection Error", &err.to_string(), "");
                                    break;
                                }
                                self.update_jobs_status();
                            } else {
                                self.timer = crate::rustdesk_interval(time::interval_at(Instant::now() + SEC30, SEC30));
                            }
                        }
                        _ = status_timer.tick() => {
                            if self.handler.is_restarting_remote_device()
                                && last_recv_time.elapsed() >= RESTART_REMOTE_DEVICE_NO_DATA_TIMEOUT
                            {
                                self.handler.msgbox("restarting-show", "Restarting remote device", "Connection in progress. Please wait.", "");
                                break;
                            }
                            let elapsed = fps_instant.elapsed().as_millis();
                            if elapsed < 1000 {
                                continue;
                            }
                            fps_instant = Instant::now();

                            let fps = self.video_threads.iter().map(|(k, v)| {
                                // Correcting the inaccuracy of status_timer
                                (k.clone(), (*v.frame_count.read().unwrap() as i32) * 1000 / elapsed as i32)
                            }).collect::<HashMap<usize, i32>>();
                            self.video_threads.iter().for_each(|(_, v)| {
                                *v.frame_count.write().unwrap() = 0;
                            });
                            self.fps_control(true, fps);
                        }
                    }
                }
                log::debug!("Exit io_loop of id={}", self.handler.get_id());
            }
            Err(err) => {
                self.handler.on_establish_connection_error(err.to_string());
            }
        }
        self.handle_disconnected(round);
    }

    fn handle_disconnected(&self, round: u32) {
        // set_disconnected_ok is used to check if new connection round is started.
        let _set_disconnected_ok = self
            .handler
            .connection_round_state
            .lock()
            .unwrap()
            .set_disconnected(round);

        if self.handler.is_default() && _set_disconnected_ok {
            // Other sessions may keep the listener running after this one disconnects.
            crate::flutter::update_text_clipboard_required();
            Client::try_stop_clipboard();
        }

        if self.handler.is_default() && _set_disconnected_ok {
            // Linux client cleanup runs synchronously in try_stop_clipboard() before FUSE is
            // unmounted. Keep this async path for other file-clipboard platforms.
            crate::clipboard::try_empty_clipboard_files(ClipboardSide::Client, self.client_conn_id);
        }
    }

    async fn handle_local_clipboard_msg(
        &mut self,
        peer: &mut Stream,
        msg: Option<clipboard::ClipboardFile>,
    ) {
        match msg {
            Some(clip) => match clip {
                clipboard::ClipboardFile::NotifyCallback {
                    r#type,
                    title,
                    text,
                } => {
                    self.handler.msgbox(&r#type, &title, &text, "");
                }
                // File-data responses bypass is_stopping_allowed, but still require login.
                _ if !self.is_connected => {
                    log::debug!("Discarding local file clipboard message before login");
                }
                _ => {
                    let is_stopping_allowed = clip.is_stopping_allowed();
                    let server_file_transfer_enabled =
                        *self.handler.server_file_transfer_enabled.read().unwrap();
                    let file_transfer_enabled =
                        self.handler.lc.read().unwrap().enable_file_copy_paste.v;
                    let stop = is_stopping_allowed
                        && !(server_file_transfer_enabled && file_transfer_enabled);
                    log::debug!(
                        "Process clipboard message from system, stop: {}, is_stopping_allowed: {}, server_file_transfer_enabled: {}, file_transfer_enabled: {}",
                        stop, is_stopping_allowed, server_file_transfer_enabled, file_transfer_enabled
                    );
                    if stop {
                        {
                            ContextSend::set_is_stopped();
                        }
                    } else {
                        if let Err(e) = ContextSend::make_sure_enabled() {
                            log::error!("failed to restart clipboard context: {}", e);
                            // to-do: Show msgbox with "Don't show again" option
                        };
                        log::debug!("Send system clipboard message to remote");
                        if matches!(&clip, clipboard::ClipboardFile::FormatList { .. }) {
                            self.initial_clipboard_pending = false;
                        }
                        let msg = crate::clipboard_file::clip_2_msg(clip);
                        allow_err!(peer.send(&msg).await);
                    }
                }
            },
            None => {
                // unreachable!()
            }
        }
    }

    fn handle_job_status(&mut self, id: i32, file_num: i32, err: Option<String>) {
        if let Some(err) = err {
            self.handler.job_error(id, err, file_num);
        } else {
            self.handler.job_done(id, file_num);
        }
    }

    async fn send_close_reason(&mut self, peer: &mut Stream, reason: &str) {
        if self.sent_close_reason {
            return;
        }
        let mut misc = Misc::new();
        misc.set_close_reason(reason.to_owned());
        let mut msg = Message::new();
        msg.set_misc(misc);
        allow_err!(peer.send(&msg).await);
        self.sent_close_reason = true;
    }

    fn start_initial_clipboard_sync(&mut self) {
        let peer_info = {
            let lc = self.handler.lc.read().unwrap();
            lc.peer_info
                .as_ref()
                .map(|pi| (pi.version.clone(), pi.platform.clone()))
        };
        let Some((peer_version, peer_platform)) = peer_info else {
            log::error!("Cannot start initial clipboard sync without peer information");
            return;
        };

        self.initial_clipboard_pending = true;
        let sender = self.sender.clone();
        let permission_config = self.handler.get_permission_config();
        // Clipboard access and encoding must not block the connection loop.
        let read_clipboard = move || {
            // Capture after readiness, before reading; later changes invalidate this snapshot.
            let generation = clipboard_listener::current_generation();
            let msg_out = if permission_config.is_text_clipboard_required() {
                crate::clipboard::get_current_clipboard_msg(
                    &peer_version,
                    &peer_platform,
                    crate::clipboard::ClipboardSide::Client,
                )
            } else {
                None
            };
            let msg_out = msg_out.filter(|_| permission_config.is_text_clipboard_required());
            // Empty or failed reads must also finish the pending initial-sync attempt.
            if let Err(err) = sender.send(Data::InitialClipboard(generation, msg_out)) {
                log::debug!("Failed to send initial clipboard result: {}", err);
            }
        };
        tokio::task::spawn_blocking(read_clipboard);
    }

    // Linux listener creation returns before X11/Wayland has subscribed to changes.
    // Capture after readiness so changes during the read can invalidate the snapshot.
    // A separate task keeps a stalled backend from blocking the connection loop.

    async fn handle_msg_from_ui(&mut self, data: Data, peer: &mut Stream) -> bool {
        match data {
            Data::Close => {
                self.send_close_reason(peer, "").await;
                return false;
            }
            Data::Login((password, remember)) => {
                self.handler
                    .handle_login_from_ui(password, remember, peer)
                    .await;
            }
            Data::InitialClipboard(generation, msg) => {
                // A live update supersedes any initial snapshot still being prepared.
                if !self.initial_clipboard_pending {
                    return true;
                }
                self.initial_clipboard_pending = false;
                if !self.handler.is_text_clipboard_required() {
                    return true;
                }
                if generation != clipboard_listener::current_generation() {
                    // A clipboard change or listener restart can invalidate the snapshot
                    // without sending a live update that supersedes it.
                    drop(msg);
                    self.start_initial_clipboard_sync();
                    return true;
                }
                if let Some(msg) = msg {
                    allow_err!(peer.send(&msg).await);
                }
            }
            Data::Message(msg) => {
                // The Flutter clipboard broadcast is process-wide, so a clipboard can reach this
                // round's queue before the round has logged in; it is dropped here, on the round
                // itself.
                if !self.is_connected
                    && matches!(
                        msg.union.as_ref(),
                        Some(message::Union::Clipboard(_))
                            | Some(message::Union::MultiClipboards(_))
                    )
                {
                    return true;
                }
                match &msg.union {
                    Some(message::Union::Cliprdr(_)) if !self.is_connected => {
                        log::debug!("Discarding outgoing file clipboard message before login");
                        return true;
                    }
                    Some(message::Union::Clipboard(_)) | Some(message::Union::MultiClipboards(_)) => {
                        self.initial_clipboard_pending = false;
                    }
                    Some(message::Union::Cliprdr(clip))
                        if matches!(clip.union.as_ref(), Some(cliprdr::Union::FormatList(_))) =>
                    {
                        self.initial_clipboard_pending = false;
                    }
                    Some(message::Union::Misc(misc)) => match misc.union {
                        Some(misc::Union::RefreshVideo(_)) => {
                            self.video_threads.iter().for_each(|(_, v)| {
                                *v.discard_queue.write().unwrap() = true;
                            });
                        }
                        Some(misc::Union::RefreshVideoDisplay(display)) => {
                            if let Some(v) = self.video_threads.get_mut(&(display as usize)) {
                                *v.discard_queue.write().unwrap() = true;
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
                allow_err!(peer.send(&msg).await);
            }
            Data::SendFiles((id, r#type, path, to, file_num, include_hidden, is_remote)) => {
                log::info!("send files, is remote {}", is_remote);
                let od = can_enable_overwrite_detection(self.handler.lc.read().unwrap().version);
                if is_remote {
                    log::debug!("New job {}, write to {} from remote {}", id, to, path);
                    let to = match r#type {
                        fs::JobType::Generic => {
                            if let Err(err) = fs::download::validate_download_name(
                                &path,
                                self.handler.peer_platform() == "Windows",
                            ) {
                                self.handle_job_status(id, -1, Some(err.to_string()));
                                return true;
                            }
                            fs::DataSource::FilePath(PathBuf::from(&to))
                        }
                        fs::JobType::Printer => {
                            fs::DataSource::MemoryCursor(std::io::Cursor::new(Vec::new()))
                        }
                    };
                    self.write_jobs.push(fs::TransferJob::new_write(
                        id,
                        r#type,
                        path.clone(),
                        to,
                        file_num,
                        include_hidden,
                        is_remote,
                        od,
                    ));
                    allow_err!(
                        peer.send(&fs::new_send(id, r#type, path, file_num, include_hidden))
                            .await
                    );
                } else {
                    match fs::TransferJob::new_read(
                        id,
                        r#type,
                        to.clone(),
                        fs::DataSource::FilePath(PathBuf::from(&path)),
                        file_num,
                        include_hidden,
                        is_remote,
                        od,
                    ) {
                        Err(err) => {
                            self.handle_job_status(id, -1, Some(err.to_string()));
                        }
                        Ok(job) => {
                            log::debug!(
                                "New job {}, read {} to remote {}, {} files",
                                id,
                                path,
                                to,
                                job.files().len()
                            );
                            self.handler.update_folder_files(
                                job.id(),
                                job.files(),
                                path,
                                !is_remote,
                                true,
                            );
                            let mut files = job.files().clone();
                            if self.handler.peer_platform() != "Windows" {
                                // peer is not windows, need transform \ to /
                                fs::transform_windows_path(&mut files);
                            }
                            let total_size = job.total_size();
                            self.read_jobs.push(job);
                            self.timer = crate::rustdesk_interval(time::interval(MILLI1));
                            allow_err!(
                                peer.send(&fs::new_receive(id, to, file_num, files, total_size))
                                    .await
                            );
                        }
                    }
                }
            }
            Data::AddJob((id, r#type, path, to, file_num, include_hidden, is_remote)) => {
                let od = can_enable_overwrite_detection(self.handler.lc.read().unwrap().version);
                if is_remote {
                    log::debug!(
                        "new write waiting job {}, write to {} from remote {}",
                        id,
                        to,
                        path
                    );
                    let mut job = fs::TransferJob::new_write(
                        id,
                        r#type,
                        path.clone(),
                        fs::DataSource::FilePath(PathBuf::from(&to)),
                        file_num,
                        include_hidden,
                        is_remote,
                        od,
                    );
                    job.is_last_job = true;
                    self.write_jobs.push(job);
                } else {
                    match fs::TransferJob::new_read(
                        id,
                        r#type,
                        to.clone(),
                        fs::DataSource::FilePath(PathBuf::from(&path)),
                        file_num,
                        include_hidden,
                        is_remote,
                        od,
                    ) {
                        Err(err) => {
                            self.handle_job_status(id, -1, Some(err.to_string()));
                        }
                        Ok(mut job) => {
                            log::debug!(
                                "new read waiting job {}, read {} to remote {}, {} files",
                                id,
                                path,
                                to,
                                job.files().len()
                            );
                            self.handler.update_folder_files(
                                job.id(),
                                job.files(),
                                path,
                                !is_remote,
                                true,
                            );
                            job.is_last_job = true;
                            self.read_jobs.push(job);
                            self.timer = crate::rustdesk_interval(time::interval(MILLI1));
                        }
                    }
                }
            }
            Data::ResumeJob((id, is_remote)) => {
                if is_remote {
                    if let Some(job) = get_job(id, &mut self.write_jobs) {
                        job.is_last_job = false;
                        job.is_resume = true;
                        allow_err!(
                            peer.send(&fs::new_send(
                                id,
                                fs::JobType::Generic,
                                job.remote.clone(),
                                job.file_num,
                                job.show_hidden
                            ))
                            .await
                        );
                    }
                } else {
                    if let Some(job) = get_job(id, &mut self.read_jobs) {
                        match &job.data_source {
                            fs::DataSource::FilePath(_p) => {
                                job.is_last_job = false;
                                job.is_resume = true;
                                job.set_finished_size_on_resume();
                                let mut files = job.files().clone();
                                if self.handler.peer_platform() != "Windows" {
                                    // peer is not windows, need transform \ to /
                                    fs::transform_windows_path(&mut files);
                                }
                                allow_err!(
                                    peer.send(&fs::new_receive(
                                        id,
                                        job.remote.clone(),
                                        job.file_num,
                                        files,
                                        job.total_size(),
                                    ))
                                    .await
                                );
                            }
                            fs::DataSource::MemoryCursor(_) => {
                                // unreachable!()
                                log::error!("Resume job with memory cursor");
                            }
                        }
                    }
                }
            }
            Data::SetConfirmOverrideFile((id, file_num, need_override, remember, is_upload)) => {
                if is_upload {
                    if let Some(job) = fs::get_job(id, &mut self.read_jobs) {
                        if remember {
                            job.set_overwrite_strategy(Some(need_override));
                        }
                        job.confirm(&FileTransferSendConfirmRequest {
                            id,
                            file_num,
                            union: if need_override {
                                Some(file_transfer_send_confirm_request::Union::OffsetBlk(0))
                            } else {
                                Some(file_transfer_send_confirm_request::Union::Skip(true))
                            },
                            ..Default::default()
                        })
                        .await;
                    }
                } else {
                    if let Some(job) = fs::get_job(id, &mut self.write_jobs) {
                        if remember {
                            job.set_overwrite_strategy(Some(need_override));
                        }
                        let mut msg = Message::new();
                        let mut file_action = FileAction::new();
                        let req = FileTransferSendConfirmRequest {
                            id,
                            file_num,
                            union: if need_override {
                                Some(file_transfer_send_confirm_request::Union::OffsetBlk(0))
                            } else {
                                Some(file_transfer_send_confirm_request::Union::Skip(true))
                            },
                            ..Default::default()
                        };
                        job.confirm(&req).await;
                        file_action.set_send_confirm(req);
                        msg.set_file_action(file_action);
                        allow_err!(peer.send(&msg).await);
                    }
                }
            }
            Data::RemoveDirAll((id, path, is_remote, include_hidden)) => {
                if is_remote {
                    let mut msg_out = Message::new();
                    let mut file_action = FileAction::new();
                    file_action.set_all_files(ReadAllFiles {
                        id,
                        path: path.clone(),
                        include_hidden,
                        ..Default::default()
                    });
                    msg_out.set_file_action(file_action);
                    allow_err!(peer.send(&msg_out).await);
                    self.remove_jobs
                        .insert(id, RemoveJob::new(Vec::new()));
                } else {
                    match fs::get_recursive_files(&path, include_hidden) {
                        Ok(entries) => {
                            self.handler.update_folder_files(
                                id,
                                &entries,
                                path.clone(),
                                !is_remote,
                                false,
                            );
                            self.remove_jobs
                                .insert(id, RemoveJob::new(entries));
                        }
                        Err(err) => {
                            self.handle_job_status(id, -1, Some(err.to_string()));
                        }
                    }
                }
            }
            Data::CancelJob(id) => {
                self.cancel_transfer_job(id, peer).await;
            }
            Data::RemoveDir((id, path)) => {
                let mut msg_out = Message::new();
                let mut file_action = FileAction::new();
                file_action.set_remove_dir(FileRemoveDir {
                    id,
                    path,
                    recursive: true,
                    ..Default::default()
                });
                msg_out.set_file_action(file_action);
                allow_err!(peer.send(&msg_out).await);
            }
            Data::RemoveFile((id, path, file_num, is_remote)) => {
                if is_remote {
                    let mut msg_out = Message::new();
                    let mut file_action = FileAction::new();
                    file_action.set_remove_file(FileRemoveFile {
                        id,
                        path,
                        file_num,
                        ..Default::default()
                    });
                    msg_out.set_file_action(file_action);
                    allow_err!(peer.send(&msg_out).await);
                } else {
                    match fs::remove_file(&path) {
                        Err(err) => {
                            self.handle_job_status(id, file_num, Some(err.to_string()));
                        }
                        Ok(()) => {
                            self.handle_job_status(id, file_num, None);
                        }
                    }
                }
            }
            Data::CreateDir((id, path, is_remote)) => {
                if is_remote {
                    let mut msg_out = Message::new();
                    let mut file_action = FileAction::new();
                    file_action.set_create(FileDirCreate {
                        id,
                        path,
                        ..Default::default()
                    });
                    msg_out.set_file_action(file_action);
                    allow_err!(peer.send(&msg_out).await);
                } else {
                    match fs::create_dir(&path) {
                        Err(err) => {
                            self.handle_job_status(id, -1, Some(err.to_string()));
                        }
                        Ok(()) => {
                            self.handle_job_status(id, -1, None);
                        }
                    }
                }
            }
            Data::RenameFile((id, path, new_name, is_remote)) => {
                if is_remote {
                    let mut msg_out = Message::new();
                    let mut file_action = FileAction::new();
                    file_action.set_rename(FileRename {
                        id,
                        path,
                        new_name,
                        ..Default::default()
                    });
                    msg_out.set_file_action(file_action);
                    allow_err!(peer.send(&msg_out).await);
                } else {
                    let err = fs::rename_file(&path, &new_name)
                        .err()
                        .map(|e| e.to_string());
                    self.handle_job_status(id, -1, err);
                }
            }
            Data::ElevateDirect => {
                let mut request = ElevationRequest::new();
                request.set_direct(true);
                let mut misc = Misc::new();
                misc.set_elevation_request(request);
                let mut msg = Message::new();
                msg.set_misc(misc);
                allow_err!(peer.send(&msg).await);
                self.elevation_requested = true;
            }
            Data::ElevateWithLogon(username, password) => {
                let mut request = ElevationRequest::new();
                request.set_logon(ElevationRequestWithLogon {
                    username,
                    password,
                    ..Default::default()
                });
                let mut misc = Misc::new();
                misc.set_elevation_request(request);
                let mut msg = Message::new();
                msg.set_misc(misc);
                allow_err!(peer.send(&msg).await);
                self.elevation_requested = true;
            }
            Data::ResetDecoder(display) => match display {
                Some(display) => {
                    if let Some(v) = self.video_threads.get_mut(&display) {
                        v.video_sender.send(MediaData::Reset).ok();
                    }
                }
                None => {
                    for (_, v) in self.video_threads.iter_mut() {
                        v.video_sender.send(MediaData::Reset).ok();
                    }
                }
            },
            _ => {}
        }
        true
    }

    #[inline]
    fn update_job_status(
        job: &fs::TransferJob,
        elapsed: i32,
        last_update_jobs_status: &mut (Instant, HashMap<i32, u64>),
        handler: &Session<T>,
    ) {
        if elapsed <= 0 {
            return;
        }
        let transferred = job.transferred();
        let last_transferred = {
            if let Some(v) = last_update_jobs_status.1.get(&job.id()) {
                v.to_owned()
            } else {
                0
            }
        };
        last_update_jobs_status.1.insert(job.id(), transferred);
        let speed = (transferred - last_transferred) as f64 / (elapsed as f64 / 1000.);
        let file_num = job.file_num() - 1;
        handler.job_progress(job.id(), file_num, speed, job.finished_size() as f64);
    }

    fn update_jobs_status(&mut self) {
        let elapsed = self.last_update_jobs_status.0.elapsed().as_millis() as i32;
        if elapsed >= 1000 {
            for job in self.read_jobs.iter() {
                Self::update_job_status(
                    job,
                    elapsed,
                    &mut self.last_update_jobs_status,
                    &self.handler,
                );
            }
            for job in self.write_jobs.iter() {
                Self::update_job_status(
                    job,
                    elapsed,
                    &mut self.last_update_jobs_status,
                    &mut self.handler,
                );
            }
            self.last_update_jobs_status.0 = Instant::now();
        }
    }

    async fn cancel_transfer_job(&mut self, id: i32, peer: &mut Stream) {
        let mut msg_out = Message::new();
        let mut file_action = FileAction::new();
        file_action.set_cancel(FileTransferCancel {
            id,
            ..Default::default()
        });
        msg_out.set_file_action(file_action);
        allow_err!(peer.send(&msg_out).await);
        if let Some(job) = fs::remove_job(id, &mut self.write_jobs) {
            job.remove_download_file();
        }
        let _ = fs::remove_job(id, &mut self.read_jobs);
        self.remove_jobs.remove(&id);
    }

    pub async fn sync_jobs_status_to_local(&mut self) -> bool {
        if !self.is_connected {
            return false;
        }
        let mut config: PeerConfig = self.handler.load_config();
        let mut transfer_metas = TransferSerde::default();
        for job in self.read_jobs.iter() {
            let json_str = serde_json::to_string(&job.gen_meta()).unwrap_or_default();
            transfer_metas.read_jobs.push(json_str);
        }
        for job in self.write_jobs.iter() {
            let json_str = serde_json::to_string(&job.gen_meta()).unwrap_or_default();
            transfer_metas.write_jobs.push(json_str);
        }
        log::info!("meta: {:?}", transfer_metas);
        if config.transfer != transfer_metas {
            config.transfer = transfer_metas;
            self.handler.save_config(config);
        }
        true
    }

    fn contains_key_frame(vf: &VideoFrame) -> bool {
        use video_frame::Union::*;
        match &vf.union {
            Some(vf) => match vf {
                Vp8s(f) | Vp9s(f) | Av1s(f) | H264s(f) | H265s(f) => f.frames.iter().any(|e| e.key),
                _ => false,
            },
            None => false,
        }
    }

    // Currently, this function only considers decoding speed and queue length, not network delay.
    // The controlled end can consider auto fps as the maximum decoding fps.
    #[inline]
    fn fps_control(&mut self, direct: bool, real_fps_map: HashMap<usize, i32>) {
        self.video_threads.iter_mut().for_each(|(k, v)| {
            let real_fps = real_fps_map.get(k).cloned().unwrap_or_default();
            if real_fps == 0 {
                v.fps_control.inactive_counter += 1;
            } else {
                v.fps_control.inactive_counter = 0;
            }
        });
        let custom_fps = self.handler.lc.read().unwrap().custom_fps.clone();
        let custom_fps = custom_fps.lock().unwrap().clone();
        let mut custom_fps = custom_fps.unwrap_or(30);
        if custom_fps < 5 || custom_fps > 120 {
            custom_fps = 30;
        }
        let inactive_threshold = 15;
        let max_queue_len = self
            .video_threads
            .iter()
            .map(|v| v.1.video_queue.read().unwrap().len())
            .max()
            .unwrap_or_default();
        let min_decode_fps = self
            .video_threads
            .iter()
            .filter(|v| v.1.fps_control.inactive_counter < inactive_threshold)
            .map(|v| *v.1.decode_fps.read().unwrap())
            .min()
            .flatten();
        let Some(min_decode_fps) = min_decode_fps else {
            return;
        };
        let mut limited_fps = if direct {
            min_decode_fps * 9 / 10 // 30 got 27
        } else {
            min_decode_fps * 4 / 5 // 30 got 24
        };
        if limited_fps > custom_fps {
            limited_fps = custom_fps;
        }
        let last_auto_fps = self.handler.lc.read().unwrap().last_auto_fps.clone();
        let displays = self.video_threads.keys().cloned().collect::<Vec<_>>();
        let mut fps_trending = |display: usize| {
            let thread = self.video_threads.get_mut(&display)?;
            let ctl = &mut thread.fps_control;
            let len = thread.video_queue.read().unwrap().len();
            let decode_fps = thread.decode_fps.read().unwrap().clone()?;
            let last_auto_fps = last_auto_fps.clone().unwrap_or(custom_fps as _);
            if ctl.inactive_counter > inactive_threshold {
                return None;
            }
            if len > 1 && last_auto_fps > limited_fps || len > std::cmp::max(1, decode_fps / 2) {
                ctl.idle_counter = 0;
                return Some(false);
            }
            if len <= 1 {
                ctl.idle_counter += 1;
                if ctl.idle_counter > 3 && last_auto_fps + 3 <= limited_fps {
                    return Some(true);
                }
            }
            if len > 1 {
                ctl.idle_counter = 0;
            }
            None
        };
        let trendings: Vec<_> = displays.iter().map(|k| fps_trending(*k)).collect();
        let should_decrease = trendings.iter().any(|v| *v == Some(false));
        let should_increase = !should_decrease && trendings.iter().any(|v| *v == Some(true));
        if last_auto_fps.is_none() || should_decrease || should_increase {
            // limited_fps to ensure decoding is faster than encoding
            let mut auto_fps = limited_fps;
            if should_decrease && limited_fps < max_queue_len {
                auto_fps = limited_fps / 2;
            }
            if auto_fps < 1 {
                auto_fps = 1;
            }
            if Some(auto_fps) != last_auto_fps {
                let mut misc = Misc::new();
                misc.set_option(OptionMessage {
                    custom_fps: auto_fps as _,
                    ..Default::default()
                });
                let mut msg = Message::new();
                msg.set_misc(misc);
                self.sender.send(Data::Message(msg)).ok();
                log::info!("Set fps to {}", auto_fps);
                self.handler.lc.write().unwrap().last_auto_fps = Some(auto_fps);
            }
        }
        // send refresh
        for (display, thread) in self.video_threads.iter_mut() {
            let ctl = &mut thread.fps_control;
            let video_queue = thread.video_queue.read().unwrap();
            let tolerable = std::cmp::min(min_decode_fps, video_queue.capacity() / 2);
            if ctl.refresh_times < 20 // enough
                    && (video_queue.len() > tolerable
                            && (ctl.refresh_times == 0 || ctl.last_refresh_instant.map(|t|t.elapsed().as_secs() > 10).unwrap_or(false)))
            {
                // Refresh causes client set_display, left frames cause flickering.
                drop(video_queue);
                self.handler.refresh_video(*display as _);
                log::info!("Refresh display {} to reduce delay", display);
                ctl.refresh_times += 1;
                ctl.last_refresh_instant = Some(Instant::now());
            }
        }
    }

    async fn handle_msg_from_peer(&mut self, data: &[u8], peer: &mut Stream) -> bool {
        if let Ok(msg_in) = Message::parse_from_bytes(&data) {
            match msg_in.union {
                Some(message::Union::VideoFrame(vf)) => {
                    if !self.first_frame {
                        self.first_frame = true;
                    }

                    let display = vf.display as usize;
                    if !self.video_threads.contains_key(&display) {
                        self.new_video_thread(display);
                    }
                    let Some(thread) = self.video_threads.get_mut(&display) else {
                        return true;
                    };
                    if Self::contains_key_frame(&vf) {
                        thread
                            .video_sender
                            .send(MediaData::VideoFrame(Box::new(vf)))
                            .ok();
                    } else {
                        let video_queue = thread.video_queue.read().unwrap();
                        if video_queue.force_push(vf).is_some() {
                            drop(video_queue);
                            self.handler.refresh_video(display as _);
                        } else {
                            thread.video_sender.send(MediaData::VideoQueue).ok();
                        }
                    }
                }
                Some(message::Union::Hash(hash)) => {
                    if !self
                        .handler
                        .handle_hash(&self.handler.password.clone(), hash, peer)
                        .await
                    {
                        return false;
                    }
                }
                Some(message::Union::LoginResponse(lr)) => match lr.union {
                    Some(login_response::Union::Error(err)) => {
                        if !self.handler.handle_login_error(&err) {
                            return false;
                        }
                    }
                    Some(login_response::Union::PeerInfo(pi)) => {
                        self.handler.handle_peer_info(pi);
                        if self.handler.is_default() {
                            // Startup notifications must see the current sessions' clipboard requirements.
                            crate::flutter::update_text_clipboard_required();


                            let rx = Client::try_start_clipboard();
                            // To make sure current text clipboard data is updated.
                            if let Some(mut rx) = rx {
                                timeout(CLIPBOARD_INTERVAL, rx.recv()).await.ok();
                            }

                            // `is_connected`` becomes true after the first PeerInfo.
                            // Reconnects create a new Remote. Refreshes after a generation change
                            // use a separate entry point and are not blocked by this guard.
                            if !self.is_connected
                                && self.handler.is_text_clipboard_required()
                                && self.handler.lc.read().unwrap().sync_init_clipboard.v
                            {
                                self.start_initial_clipboard_sync();
                            }
                            // to-do: Android, is `sync_init_clipboard` really needed?
                            // https://github.com/rustdesk/rustdesk/discussions/9010
                        }

                        if self.handler.is_file_transfer() {
                            self.handler.load_last_jobs();
                        }

                        self.is_connected = true;
                    }
                    _ => {}
                },
                Some(message::Union::CursorData(cd)) if self.dedupes_cursors() => {
                    self.set_cursor_data_by_content(cd);
                }
                Some(message::Union::CursorId(id)) if self.dedupes_cursors() => {
                    self.handler
                        .set_cursor_id(self.cursor_dedupe.id(id).to_string());
                }
                Some(message::Union::CursorData(cd)) => {
                    let id = cd.id;
                    let compressed = cd.colors.clone();
                    match decode_cursor_data(cd) {
                        Ok(cd) => {
                            self.keep_cursor_shape(cursor_shape(id, &cd, &compressed));
                            self.handler.set_cursor_data(cd)
                        }
                        Err(err) => {
                            log::warn!("Rejected cursor {id}: {err}");
                            // The peer shows it now: selected, the UI treats it as a shape it
                            // lacks, as it will when the peer selects it again.
                            self.handler.set_cursor_id(id.to_string());
                        }
                    }
                }
                Some(message::Union::CursorId(id)) => {
                    self.handler.set_cursor_id(id.to_string());
                }
                Some(message::Union::CursorPosition(cp)) => {
                    self.handler.set_cursor_position(cp);
                }
                Some(message::Union::Clipboard(cb)) => {
                    let clipboard_allowed = {
                        let lc = self.handler.lc.read().unwrap();
                        !lc.disable_clipboard.v
                    };
                    if clipboard_allowed {
                        self.initial_clipboard_pending = false;
                        update_clipboard(vec![cb], ClipboardSide::Client);
                    }
                }
                Some(message::Union::MultiClipboards(_mcb)) => {
                    let clipboard_allowed = {
                        let lc = self.handler.lc.read().unwrap();
                        !lc.disable_clipboard.v
                    };
                    if clipboard_allowed {
                        self.initial_clipboard_pending = false;
                        update_clipboard(_mcb.clipboards, ClipboardSide::Client);
                    }
                }
                Some(message::Union::Cliprdr(clip)) => {
                    self.handle_cliprdr_msg(clip, peer).await;
                }
                Some(message::Union::FileResponse(fr)) => {
                    match fr.union {
                        Some(file_response::Union::EmptyDirs(res)) => {
                            self.handler.update_empty_dirs(res);
                        }
                        Some(file_response::Union::Dir(fd)) => {
                            let entries = fd.entries.to_vec();
                            if fd.id <= 0 {
                                if let Err(err) = fs::download::validate_listing(
                                    &entries,
                                    self.handler.peer_platform() == "Windows",
                                ) {
                                    self.handle_job_status(fd.id, -1, Some(err.to_string()));
                                    return true;
                                }
                            }
                            // We cannot call cancel_transfer_job/handle_job_status while holding
                            // a mutable borrow from fs::get_job(&mut self.write_jobs), so defer
                            // the error handling until after the borrow scope ends.
                            let mut set_files_err = None;
                            if let Some(job) = fs::get_job(fd.id, &mut self.write_jobs) {
                                log::info!("job set_files: {:?}", entries);
                                if let Err(err) = job.set_files(entries) {
                                    set_files_err = Some(err.to_string());
                                } else {
                                    job.set_finished_size_on_resume();
                                    self.handler.update_folder_files(
                                        fd.id,
                                        job.files(),
                                        fd.path,
                                        false,
                                        false,
                                    );
                                }
                            } else if let Some(job) = self.remove_jobs.get_mut(&fd.id) {
                                // Intentionally keep raw entries here:
                                // - remote remove flow executes deletions on peer side;
                                // - local remove flow is populated from local get_recursive_files().
                                job.files = entries;
                                self.handler
                                    .update_folder_files(fd.id, &job.files, fd.path, false, false);
                            } else {
                                self.handler
                                    .update_folder_files(fd.id, &entries, fd.path, false, false);
                            }
                            if let Some(err) = set_files_err {
                                log::warn!(
                                    "Rejected unsafe file list from remote peer for job {}: {}",
                                    fd.id,
                                    err
                                );
                                self.cancel_transfer_job(fd.id, peer).await;
                                self.handle_job_status(fd.id, -1, Some(err));
                            }
                        }
                        Some(file_response::Union::Digest(digest)) => {
                            if digest.is_upload {
                                if let Some(job) = fs::get_job(digest.id, &mut self.read_jobs) {
                                    if let Some(file) = job.files().get(digest.file_num as usize) {
                                        if let fs::DataSource::FilePath(p) = &job.data_source {
                                            let read_path =
                                                get_string(&fs::TransferJob::join(p, &file.name));
                                            let mut overwrite_strategy =
                                                job.default_overwrite_strategy();
                                            let mut offset = 0;
                                            if digest.is_identical && job.is_resume {
                                                if digest.transferred_size > 0 {
                                                    overwrite_strategy = Some(true);
                                                    offset = digest.transferred_size as _;
                                                }
                                            }
                                            if let Some(overwrite) = overwrite_strategy {
                                                let req = FileTransferSendConfirmRequest {
                                                    id: digest.id,
                                                    file_num: digest.file_num,
                                                    union: Some(if overwrite {
                                                        file_transfer_send_confirm_request::Union::OffsetBlk(offset)
                                                    } else {
                                                        file_transfer_send_confirm_request::Union::Skip(
                                                            true,
                                                        )
                                                    }),
                                                    ..Default::default()
                                                };
                                                job.confirm(&req).await;
                                                let msg = new_send_confirm(req);
                                                allow_err!(peer.send(&msg).await);
                                            } else {
                                                self.handler.override_file_confirm(
                                                    digest.id,
                                                    digest.file_num,
                                                    read_path,
                                                    true,
                                                    digest.is_identical,
                                                );
                                            }
                                        }
                                    }
                                }
                            } else {
                                if let Some(job) = fs::get_job(digest.id, &mut self.write_jobs) {
                                    if let Some(file) = job.files().get(digest.file_num as usize) {
                                        if let fs::DataSource::FilePath(p) = &job.data_source {
                                            let write_path =
                                                get_string(&fs::TransferJob::join(p, &file.name));
                                            job.set_digest(digest.file_size, digest.last_modified);
                                            let peer_ver = self.handler.lc.read().unwrap().version;
                                            let is_support_resume =
                                                crate::is_support_file_transfer_resume_num(
                                                    peer_ver,
                                                );
                                            match job.download_digest(is_support_resume, &digest) {
                                                Ok(res) => match res {
                                                    DigestCheckResult::IsSame => {
                                                        let req = FileTransferSendConfirmRequest {
                                                            id: digest.id,
                                                            file_num: digest.file_num,
                                                            union: Some(file_transfer_send_confirm_request::Union::Skip(true)),
                                                            ..Default::default()
                                                        };
                                                        job.confirm(&req).await;
                                                        let msg = new_send_confirm(req);
                                                        allow_err!(peer.send(&msg).await);
                                                    }
                                                    DigestCheckResult::NeedConfirm(digest) => {
                                                        let mut overwrite_strategy =
                                                            job.default_overwrite_strategy();
                                                        let mut offset = 0;
                                                        if digest.is_identical
                                                            && job.is_resume
                                                            && digest.transferred_size > 0
                                                        {
                                                            overwrite_strategy = Some(true);
                                                            offset = digest.transferred_size as _;
                                                        }
                                                        if let Some(overwrite) = overwrite_strategy
                                                        {
                                                            let req =
                                                                FileTransferSendConfirmRequest {
                                                                    id: digest.id,
                                                                    file_num: digest.file_num,
                                                                    union: Some(if overwrite {
                                                                        file_transfer_send_confirm_request::Union::OffsetBlk(offset)
                                                                    } else {
                                                                        file_transfer_send_confirm_request::Union::Skip(true)
                                                                    }),
                                                                    ..Default::default()
                                                                };
                                                            job.confirm(&req).await;
                                                            let msg = new_send_confirm(req);
                                                            allow_err!(peer.send(&msg).await);
                                                        } else {
                                                            self.handler.override_file_confirm(
                                                                digest.id,
                                                                digest.file_num,
                                                                write_path,
                                                                false,
                                                                digest.is_identical,
                                                            );
                                                        }
                                                    }
                                                    DigestCheckResult::NoSuchFile => {
                                                        let req = FileTransferSendConfirmRequest {
                                                        id: digest.id,
                                                        file_num: digest.file_num,
                                                        union: Some(file_transfer_send_confirm_request::Union::OffsetBlk(0)),
                                                        ..Default::default()
                                                    };
                                                        // This did not approve replacing an existing local file.
                                                        let msg = new_send_confirm(req);
                                                        allow_err!(peer.send(&msg).await);
                                                    }
                                                },
                                                Err(err) => {
                                                    self.cancel_transfer_job(digest.id, peer).await;
                                                    self.handle_job_status(
                                                        digest.id,
                                                        -1,
                                                        Some(err.to_string()),
                                                    );
                                                    return true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(file_response::Union::Block(block)) => {
                            let id = block.id;
                            let mut write_error = None;
                            if let Some(job) = fs::get_job(block.id, &mut self.write_jobs) {
                                let result = job.write(block).await;
                                if job.r#type == fs::JobType::Generic {
                                    write_error = result.err().map(|err| err.to_string());
                                    self.update_jobs_status();
                                }
                            }
                            if let Some(err) = write_error {
                                self.cancel_transfer_job(id, peer).await;
                                self.handle_job_status(id, -1, Some(err));
                            }
                        }
                        Some(file_response::Union::Done(d)) => {
                            let mut err: Option<String> = None;
                            let mut job_type = fs::JobType::Generic;
                            if let Some(job) = fs::remove_job(d.id, &mut self.write_jobs) {
                                err = job
                                    .finish_download()
                                    .err()
                                    .map(|err| err.to_string())
                                    .or_else(|| job.job_error());
                                job_type = job.r#type;
                            }
                            if job_type == fs::JobType::Generic {
                                self.handle_job_status(d.id, d.file_num, err);
                            }
                        }
                        Some(file_response::Union::Error(e)) => {
                            let job_type = fs::remove_job(e.id, &mut self.write_jobs)
                                .or_else(|| fs::remove_job(e.id, &mut self.read_jobs))
                                .map(|j| j.r#type)
                                .unwrap_or(fs::JobType::Generic);
                            if job_type == fs::JobType::Generic {
                                self.handle_job_status(e.id, e.file_num, Some(e.error));
                            }
                        }
                        _ => {}
                    }
                }
                Some(message::Union::Misc(misc)) => match misc.union {
                    Some(misc::Union::AudioFormat(f)) => {
                        self.audio_sender.send(MediaData::AudioFormat(f)).ok();
                    }
                    Some(misc::Union::PermissionInfo(p)) => {
                        log::info!("Change permission {:?} -> {}", p.permission, p.enabled);
                        // https://github.com/rustdesk/rustdesk/issues/3703#issuecomment-1474734754
                        match p.permission.enum_value() {
                            Ok(Permission::Keyboard) => {
                                *self.handler.server_keyboard_enabled.write().unwrap() = p.enabled;
                                crate::flutter::update_text_clipboard_required();
                                self.handler.set_permission("keyboard", p.enabled);
                            }
                            Ok(Permission::Clipboard) => {
                                *self.handler.server_clipboard_enabled.write().unwrap() = p.enabled;
                                crate::flutter::update_text_clipboard_required();
                                self.handler.set_permission("clipboard", p.enabled);
                            }
                            Ok(Permission::Audio) => {
                                self.handler.set_permission("audio", p.enabled);
                            }
                            Ok(Permission::File) => {
                                *self.handler.server_file_transfer_enabled.write().unwrap() =
                                    p.enabled;
                                if !p.enabled && self.handler.is_file_transfer() {
                                    return true;
                                }
                                self.handler.set_permission("file", p.enabled);
                            }
                            Ok(Permission::Restart) => {
                                self.handler.set_permission("restart", p.enabled);
                            }
                            _ => {}
                        }
                    }
                    Some(misc::Union::SwitchDisplay(s)) => {
                        self.handler.handle_peer_switch_display(&s);
                        if let Some(thread) = self.video_threads.get_mut(&(s.display as usize)) {
                            thread.video_sender.send(MediaData::Reset).ok();
                        }
                    }
                    Some(misc::Union::CloseReason(c)) => {
                        self.sent_close_reason = true; // The controlled end will close, no need to send close reason
                        self.handler.msgbox("error", "Connection Error", &c, "");
                        return false;
                    }
                    Some(misc::Union::Uac(uac)) => {
                        let keyboard = self.handler.server_keyboard_enabled.read().unwrap().clone();
                        {
                            if uac && keyboard {
                                self.handler.msgbox(
                                    "on-uac",
                                    "Prompt",
                                    "Please wait for confirmation of UAC...",
                                    "",
                                );
                            } else {
                                self.handler.cancel_msgbox("on-uac");
                                self.handler.cancel_msgbox("wait-uac");
                                self.handler.cancel_msgbox("elevation-error");
                            }
                        }
                    }
                    Some(misc::Union::ForegroundWindowElevated(elevated)) => {
                        let keyboard = self.handler.server_keyboard_enabled.read().unwrap().clone();
                        {
                            if elevated && keyboard {
                                self.handler.msgbox(
                                    "on-foreground-elevated",
                                    "Prompt",
                                    "elevated_foreground_window_tip",
                                    "",
                                );
                            } else {
                                self.handler.cancel_msgbox("on-foreground-elevated");
                                self.handler.cancel_msgbox("wait-uac");
                                self.handler.cancel_msgbox("elevation-error");
                            }
                        }
                    }
                    Some(misc::Union::ElevationResponse(err)) => {
                        if err.is_empty() {
                            self.handler.msgbox("wait-uac", "", "", "");
                        } else {
                            self.handler.cancel_msgbox("wait-uac");
                            self.handler
                                .msgbox("elevation-error", "Elevation Error", &err, "");
                        }
                    }
                    Some(misc::Union::PortableServiceRunning(b)) => {
                        self.handler.portable_service_running(b);
                        if self.elevation_requested && b {
                            self.handler.msgbox(
                                "custom-nocancel-success",
                                "Successful",
                                "Elevate successfully",
                                "",
                            );
                        }
                    }
                    Some(misc::Union::SupportedEncoding(e)) => {
                        log::info!("update supported encoding:{:?}", e);
                        self.handler.lc.write().unwrap().supported_encoding = e;
                    }
                    Some(misc::Union::FollowCurrentDisplay(d_idx)) => {
                        self.handler.set_current_display(d_idx);
                    }
                    _ => {}
                },
                Some(message::Union::TestDelay(t)) => {
                    self.handler.handle_test_delay(t, peer).await;
                }
                Some(message::Union::AudioFrame(frame)) => {
                    if !self.handler.lc.read().unwrap().disable_audio.v {
                        self.audio_sender
                            .send(MediaData::AudioFrame(Box::new(frame)))
                            .ok();
                    }
                }
                Some(message::Union::FileAction(action)) => match action.union {
                    Some(file_action::Union::SendConfirm(c)) => {
                        if let Some(job) = fs::get_job(c.id, &mut self.read_jobs) {
                            job.confirm(&c).await;
                        }
                    }
                    _ => {}
                },
                Some(message::Union::MessageBox(msgbox)) => {
                    let mut link = msgbox.link;
                    if let Some(v) = config::HELPER_URL.get(&link as &str) {
                        link = v.to_string();
                    } else {
                        log::warn!("Message box ignore link {} for security", &link);
                        link = "".to_string();
                    }
                    self.handler
                        .msgbox(&msgbox.msgtype, &msgbox.title, &msgbox.text, &link);
                }
                Some(message::Union::PeerInfo(pi)) => {
                    self.handler.set_displays(&pi.displays);
                    self.handler.set_platform_additions(&pi.platform_additions);
                }
                _ => {}
            }
        }
        true
    }

    async fn handle_cliprdr_msg(&mut self, clip: base::message_proto::Cliprdr, _peer: &mut Stream) {
        if !self.is_connected {
            log::debug!("Discarding incoming file clipboard message before login");
            return;
        }
        log::debug!("handling cliprdr msg from server peer");
        if let Some(base::message_proto::cliprdr::Union::FormatList(_)) = &clip.union {
            if self.client_conn_id
                != clipboard::get_client_conn_id(&crate::flutter::get_cur_peer_id()).unwrap_or(0)
            {
                return;
            }
        }

        let Some(clip) = crate::clipboard_file::msg_2_clip(clip) else {
            log::warn!("failed to decode cliprdr msg from server peer");
            return;
        };

        let is_stopping_allowed = clip.is_beginning_message();
        let file_transfer_enabled = self.handler.is_file_clipboard_required();
        let stop = is_stopping_allowed && !file_transfer_enabled;
        log::debug!(
                "Process clipboard message from server peer, stop: {}, is_stopping_allowed: {}, file_transfer_enabled: {}",
                stop, is_stopping_allowed, file_transfer_enabled);
        if !stop {
            if let Err(e) = ContextSend::make_sure_enabled() {
                log::error!("failed to restart clipboard context: {}", e);
            };
            {
                if matches!(&clip, clipboard::ClipboardFile::FormatList { .. }) {
                    self.initial_clipboard_pending = false;
                }
                let _ = ContextSend::proc(|context| -> ResultType<()> {
                    context
                        .server_clip_file(self.client_conn_id, clip)
                        .map_err(|e| e.into())
                });
            }
        }
    }

    fn new_video_thread(&mut self, display: usize) {
        let video_queue = Arc::new(RwLock::new(ArrayQueue::new(client::VIDEO_QUEUE_SIZE)));
        let (video_sender, video_receiver) = std::sync::mpsc::channel::<MediaData>();
        let decode_fps = Arc::new(RwLock::new(None));
        let frame_count = Arc::new(RwLock::new(0));
        let discard_queue = Arc::new(RwLock::new(false));
        let video_thread = VideoThread {
            video_queue: video_queue.clone(),
            video_sender,
            decode_fps: decode_fps.clone(),
            frame_count: frame_count.clone(),
            fps_control: Default::default(),
            discard_queue: discard_queue.clone(),
        };
        let handler = self.handler.ui_handler.clone();
        crate::client::start_video_thread(
            self.handler.clone(),
            video_receiver,
            video_queue,
            decode_fps,
            discard_queue,
            move |display: usize, data: &mut scrap::ImageRgb| {
                *frame_count.write().unwrap() += 1;
                handler.on_rgba(display, data);
            },
        );
        self.video_threads.insert(display, video_thread);
    }

    /// A shape that decoded, for the UI to ask for again; see `Session::cursor_shapes`.
    fn keep_cursor_shape(&self, shape: CursorData) {
        self.handler
            .cursor_shapes
            .write()
            .unwrap()
            .insert(shape.id, shape);
    }

    fn dedupes_cursors(&self) -> bool {
        !crate::is_peer_naming_cursors_by_content(self.handler.lc.read().unwrap().version)
    }

    /// A shape the UI already has, under whatever handle, is only selected.
    fn set_cursor_data_by_content(&mut self, cd: CursorData) {
        let peer_id = cd.id;
        let id = CursorDedupe::name(&cd);
        if self.cursor_dedupe.has_decoded(id) {
            self.cursor_dedupe.record(peer_id, id);
            self.handler.set_cursor_id(id.to_string());
            return;
        }
        let compressed = cd.colors.clone();
        match decode_cursor_data(cd) {
            Ok(mut cd) => {
                cd.id = id;
                self.cursor_dedupe.record(peer_id, id);
                self.keep_cursor_shape(cursor_shape(id, &cd, &compressed));
                self.handler.set_cursor_data(cd);
            }
            Err(err) => {
                log::warn!("Rejected cursor {peer_id}: {err}");
                self.handler
                    .set_cursor_id(self.cursor_dedupe.rejected(peer_id, id).to_string());
            }
        }
    }
}

/// A shape that decoded, still compressed, under the id the UI knows it by. Copied once it
/// decoded: the message's colors may be a slice of a larger received buffer, which a clone
/// would keep alive.
fn cursor_shape(id: u64, cd: &CursorData, compressed: &[u8]) -> CursorData {
    CursorData {
        id,
        hotx: cd.hotx,
        hoty: cd.hoty,
        width: cd.width,
        height: cd.height,
        colors: compressed.to_vec().into(),
        ..Default::default()
    }
}

/// The RGBA of a shape kept by `Session::cursor_shapes`, for the UI to draw it again.
pub(crate) fn kept_cursor_rgba(shape: CursorData) -> hbb_common::ResultType<CursorData> {
    decode_cursor_data(shape)
}

// Both UI handlers receive validated, uncompressed RGBA from the receive loop.
fn decode_cursor_data(data: CursorData) -> hbb_common::ResultType<CursorData> {
    use hbb_common::{anyhow::anyhow, bail};

    // Limit decoded cursor data to 1 MiB before JSON serialization.
    const MAX_CURSOR_SIZE: i32 = 512;
    const RGBA_CHANNELS: usize = 4;

    let mut cd = data;
    if !(1..=MAX_CURSOR_SIZE).contains(&cd.width) || !(1..=MAX_CURSOR_SIZE).contains(&cd.height) {
        bail!("invalid source size {}x{}", cd.width, cd.height);
    }
    if !(0..cd.width).contains(&cd.hotx) || !(0..cd.height).contains(&cd.hoty) {
        bail!(
            "hotspot ({},{}) is outside the cursor image",
            cd.hotx,
            cd.hoty
        );
    }
    let expected = (cd.width as usize)
        .checked_mul(cd.height as usize)
        .and_then(|pixels| pixels.checked_mul(RGBA_CHANNELS))
        .ok_or_else(|| anyhow!("cursor RGBA size overflow"))?;
    let max_compressed_size = zstd::zstd_safe::compress_bound(expected);
    if cd.colors.len() > max_compressed_size {
        bail!(
            "compressed cursor data too large: {} bytes (limit {max_compressed_size})",
            cd.colors.len()
        );
    }
    let colors = zstd::bulk::decompress(&cd.colors, expected)?;
    if colors.len() != expected {
        bail!(
            "invalid RGBA length: expected {expected}, got {}",
            colors.len()
        );
    }
    cd.colors = colors.into();
    Ok(cd)
}

/// Gives the UI one id per shape for a peer that names shapes by handle, so the UI decodes and
/// keeps a shape once however many handles it arrives under. Nothing is dropped for the
/// connection: the peer sends a shape once and may select any handle it named again.
#[derive(Default)]
struct CursorDedupe {
    ids: HashMap<u64, u64>,
    // The shapes given to the UI, by content id: whether one that arrives needs decoding.
    decoded: HashSet<u64>,
}

impl CursorDedupe {
    /// Hashes the compressed colors: one peer compresses the same pixels to the same bytes, and
    /// a shape seen before is then never decompressed again.
    fn name(cd: &CursorData) -> u64 {
        crate::cursor_content_id(cd.width, cd.height, cd.hotx, cd.hoty, &cd.colors)
    }

    fn id(&self, peer_id: u64) -> u64 {
        self.ids.get(&peer_id).copied().unwrap_or(peer_id)
    }

    fn has_decoded(&self, id: u64) -> bool {
        self.decoded.contains(&id)
    }

    /// Names the handle once its shape decoded, now or before.
    fn record(&mut self, peer_id: u64, id: u64) {
        self.ids.insert(peer_id, id);
        self.decoded.insert(id);
    }

    /// Names a known handle by the content of a shape that did not decode, which is never
    /// marked decoded: the UI has no shape under it, now or when the handle is selected again,
    /// and the shape the handle named before is not shown in its place. A handle never seen is
    /// not filed, so what a peer sends that does not decode takes no room.
    fn rejected(&mut self, peer_id: u64, id: u64) -> u64 {
        if let Some(named) = self.ids.get_mut(&peer_id) {
            *named = id;
        }
        id
    }
}

struct RemoveJob {
    files: Vec<FileEntry>,
}

impl RemoveJob {
    fn new(files: Vec<FileEntry>) -> Self {
        Self { files }
    }
}

#[derive(Debug, Default)]
struct FpsControl {
    refresh_times: usize,
    last_refresh_instant: Option<Instant>,
    idle_counter: usize,
    inactive_counter: usize,
}

struct VideoThread {
    video_queue: Arc<RwLock<ArrayQueue<VideoFrame>>>,
    video_sender: MediaSender,
    decode_fps: Arc<RwLock<Option<usize>>>,
    frame_count: Arc<RwLock<usize>>,
    discard_queue: Arc<RwLock<bool>>,
    fps_control: FpsControl,
}

impl Drop for VideoThread {
    fn drop(&mut self) {
        // since channels are buffered, messages sent before the disconnect will still be properly received.
        *self.discard_queue.write().unwrap() = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flutter::FlutterHandler;

    /// A round's `Remote` over a loopback pair, before any login: what it sends to `peer`
    /// arrives at `far`.
    async fn remote_and_peer() -> (Remote<FlutterHandler>, Stream, Stream) {
        let listener = hbb_common::tcp::new_listener("127.0.0.1:0", false)
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (peer, accepted) = tokio::join!(
            hbb_common::socket_client::connect_tcp(addr.to_string(), 3000),
            listener.accept()
        );
        let (accepted, far_addr) = accepted.unwrap();
        let far = Stream::Tcp(hbb_common::tcp::FramedStream::from(accepted, far_addr));
        let (sender, receiver) = mpsc::unbounded_channel::<Data>();
        let remote = Remote::new(Session::<FlutterHandler>::default(), receiver, sender);
        (remote, peer.unwrap(), far)
    }

    async fn arrives(far: &mut Stream) -> bool {
        matches!(hbb_common::timeout(300, far.next()).await, Ok(Some(Ok(_))))
    }

    fn clipboard() -> Data {
        let mut msg = Message::new();
        msg.set_clipboard(Clipboard {
            content: b"copied while this login was pending".to_vec().into(),
            ..Default::default()
        });
        Data::Message(msg)
    }

    fn login_request() -> Data {
        let mut msg = Message::new();
        msg.set_login_request(LoginRequest::new());
        Data::Message(msg)
    }

    // A clipboard queued before this round's login stays here; what the login itself sends
    // through the same queue does not.
    #[tokio::test]
    async fn a_clipboard_queued_before_this_rounds_login_is_dropped() {
        let (mut remote, mut peer, mut far) = remote_and_peer().await;
        assert!(!remote.is_connected);
        assert!(remote.handle_msg_from_ui(clipboard(), &mut peer).await);
        assert!(
            !arrives(&mut far).await,
            "a clipboard went out before the login"
        );
        assert!(remote.handle_msg_from_ui(login_request(), &mut peer).await);
        assert!(arrives(&mut far).await, "the login was held back");

        remote.is_connected = true;
        assert!(remote.handle_msg_from_ui(clipboard(), &mut peer).await);
        assert!(
            arrives(&mut far).await,
            "a clipboard after the login was held back"
        );
    }
}

#[cfg(test)]
mod cursor_dedupe_tests {
    use super::*;

    fn shape(id: u64, hotx: i32, colors: &[u8]) -> CursorData {
        CursorData {
            id,
            hotx,
            width: 2,
            height: 2,
            colors: colors.to_vec().into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_shape_sent_under_a_new_handle_keeps_its_id() {
        let mut dedupe = CursorDedupe::default();
        let first = CursorDedupe::name(&shape(1, 0, b"arrow"));
        assert!(!dedupe.has_decoded(first));
        dedupe.record(1, first);

        let again = CursorDedupe::name(&shape(2, 0, b"arrow"));
        assert_eq!(again, first);
        assert!(dedupe.has_decoded(again));
        dedupe.record(2, again);
        assert_eq!(dedupe.id(2), first);
        assert_eq!(dedupe.id(1), first);
    }
}

#[cfg(test)]
mod kept_cursor_tests {
    use super::*;

    fn compressed(width: i32, height: i32) -> CursorData {
        let rgba = vec![7u8; (width * height * 4) as usize];
        CursorData {
            id: 1,
            width,
            height,
            colors: hbb_common::compress::compress(&rgba).into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_kept_shape_is_checked_again_when_it_is_read() {
        assert_eq!(kept_cursor_rgba(compressed(4, 4)).unwrap().colors.len(), 64);
        assert!(
            kept_cursor_rgba(compressed(513, 1)).is_err(),
            "over the size cap"
        );
        let mut bomb = compressed(4, 4);
        bomb.colors = hbb_common::compress::compress(&vec![0u8; 1 << 20]).into();
        assert!(kept_cursor_rgba(bomb).is_err(), "more pixels than its size");
    }
}
