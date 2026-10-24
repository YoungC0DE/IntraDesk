use super::*;
use crate::clipboard::clipboard_listener::{self, ClipboardEvent};
pub use crate::clipboard::{ClipboardContext, ClipboardSide};
pub use crate::clipboard::{CLIPBOARD_INTERVAL as INTERVAL, CLIPBOARD_NAME as NAME};
use crate::ipc::{self, ClipboardFile, ClipboardNonFile, Data};
use std::{
    io,
    sync::mpsc::{channel, RecvTimeoutError},
    time::Duration,
};
use tokio::runtime::Runtime;


struct Handler {
    ctx: Option<ClipboardContext>,
    stream: Option<ipc::ConnectionTmpl<parity_tokio_ipc::ConnectionClient>>,
    rt: Option<Runtime>,
}


pub fn new(name: String) -> GenericService {
    let svc = EmptyExtraFieldService::new(name, false);
    GenericService::run(&svc.clone(), run);
    svc.sp
}

fn run(sp: EmptyExtraFieldService) -> ResultType<()> {

    let (tx_cb_result, rx_cb_result) = channel();
    let ctx = Some(ClipboardContext::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?);
    clipboard_listener::subscribe(sp.name(), tx_cb_result)?;
    let mut handler = Handler {
        ctx,
        stream: None,
        rt: None,
    };

    while sp.ok() {
        match rx_cb_result.recv_timeout(Duration::from_millis(INTERVAL)) {
            Ok(ClipboardEvent::Changed) => {
                if let Some(msg) = handler.get_clipboard_msg() {
                    sp.send(msg);
                }
            }
            // Host startup state must not supersede a client's explicit initial snapshot.
            Ok(ClipboardEvent::Stop) => {
                log::debug!("Clipboard listener stopped");
                break;
            }
            Ok(ClipboardEvent::StopWithError(err)) => {
                bail!("Clipboard listener stopped with error: {}", err);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                log::error!("Clipboard listener disconnected");
                break;
            }
        }
    }

    clipboard_listener::unsubscribe(&sp.name());

    Ok(())
}





impl Handler {

    fn get_clipboard_msg(&mut self) -> Option<Message> {
        if crate::common::is_server() && crate::platform::is_root() {
            match self.read_clipboard_from_cm_ipc() {
                Err(e) => {
                    log::error!("Failed to read clipboard from cm: {}", e);
                }
                Ok(data) => {
                    // Skip sending empty clipboard data.
                    // Maybe there's something wrong reading the clipboard data in cm, but no error msg is returned.
                    // The clipboard data should not be empty, the last line will try again to get the clipboard data.
                    if !data.is_empty() {
                        let mut msg = Message::new();
                        let multi_clipboards = MultiClipboards {
                            clipboards: data
                                .into_iter()
                                .map(|c| Clipboard {
                                    compress: c.compress,
                                    content: c.content,
                                    width: c.width,
                                    height: c.height,
                                    format: ClipboardFormat::from_i32(c.format)
                                        .unwrap_or(ClipboardFormat::Text)
                                        .into(),
                                    special_name: c.special_name,
                                    ..Default::default()
                                })
                                .collect(),
                            ..Default::default()
                        };
                        msg.set_multi_clipboards(multi_clipboards);
                        return Some(msg);
                    }
                }
            }
        }

        {
            crate::clipboard::check_clipboard(&mut self.ctx, ClipboardSide::Host, false)
        }
    }

    // Read clipboard data from cm using ipc.
    //
    // We cannot use `#[tokio::main(flavor = "current_thread")]` here,
    // because the auto-managed tokio runtime (async context) will be dropped after the call.
    // The next call will create a new runtime, which will cause the previous stream to be unusable.
    // So we need to manage the tokio runtime manually.
    fn read_clipboard_from_cm_ipc(&mut self) -> ResultType<Vec<ClipboardNonFile>> {
        if self.rt.is_none() {
            self.rt = Some(Runtime::new()?);
        }
        let Some(rt) = &self.rt else {
            // unreachable!
            bail!("failed to get tokio runtime");
        };
        let mut is_sent = false;
        if let Some(stream) = &mut self.stream {
            // If previous stream is still alive, reuse it.
            // If the previous stream is dead, `is_sent` will trigger reconnect.
            is_sent = match rt.block_on(stream.send(&Data::ClipboardNonFile(None))) {
                Ok(_) => true,
                Err(e) => {
                    log::debug!("Failed to send to cm: {}", e);
                    false
                }
            };
        }
        if !is_sent {
            let mut stream = rt.block_on(crate::ipc::connect(100, "_cm"))?;
            rt.block_on(stream.send(&Data::ClipboardNonFile(None)))?;
            self.stream = Some(stream);
        }

        if let Some(stream) = &mut self.stream {
            loop {
                match rt.block_on(stream.next_timeout(800))? {
                    Some(Data::ClipboardNonFile(Some((err, mut contents)))) => {
                        if !err.is_empty() {
                            bail!("{}", err);
                        } else {
                            if contents.iter().any(|c| c.next_raw) {
                                // Wrap the future with a `Timeout` in an async block to avoid panic.
                                // We cannot use `rt.block_on(timeout(1000, stream.next_raw()))` here, because it causes panic:
                                // thread '<unnamed>' panicked at D:\Projects\rust\rustdesk\libs\hbb_common\src\lib.rs:98:5:
                                // there is no reactor running, must be called from the context of a Tokio 1.x runtime
                                // note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
                                match rt.block_on(async { timeout(1000, stream.next_raw()).await })
                                {
                                    Ok(Ok(mut data)) => {
                                        for c in &mut contents {
                                            if c.next_raw {
                                                // No need to check the length because sum(content_len) == data.len().
                                                c.content = data.split_to(c.content_len).into();
                                            }
                                        }
                                    }
                                    Ok(Err(e)) => {
                                        // reset by peer
                                        self.stream = None;
                                        bail!("failed to get raw clipboard data: {}", e);
                                    }
                                    Err(e) => {
                                        // Reconnect to avoid the next raw data remaining in the buffer.
                                        self.stream = None;
                                        log::debug!("Failed to get raw clipboard data: {}", e);
                                    }
                                }
                            }
                            return Ok(contents);
                        }
                    }
                    Some(Data::ClipboardFile(ClipboardFile::MonitorReady)) => {
                        // ClipboardFile::MonitorReady is the first message sent by cm.
                    }
                    _ => {
                        bail!("failed to get clipboard data from cm");
                    }
                }
            }
        }
        // unreachable!
        bail!("failed to get clipboard data from cm");
    }
}


