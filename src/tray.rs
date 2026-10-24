use crate::client::translate;
use crate::ipc::Data;
use hbb_common::tokio;
use hbb_common::{allow_err, log};
use base::config::keys;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn start_tray() {
    if crate::ui_interface::get_builtin_option(keys::OPTION_HIDE_TRAY) == "Y" {
        {
            return;
        }
    }


    allow_err!(make_tray());
}

fn make_tray() -> hbb_common::ResultType<()> {
    // https://github.com/tauri-apps/tray-icon/blob/dev/examples/tao.rs
    use hbb_common::anyhow::Context;
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        TrayIcon, TrayIconBuilder, TrayIconEvent as TrayEvent,
    };

    // Duplicated tray icons kept piling up through the blind spots of
    // `check_process("--tray", ..)`. https://github.com/rustdesk/rustdesk/issues/15689
    if !crate::platform::windows::try_lock_tray_single_instance() {
        log::info!("Another tray process is already running in this session, exit");
        return Ok(());
    }

    let icon;
    {
        icon = include_bytes!("../res/tray-icon.ico");
    }

    let (icon_rgba, icon_width, icon_height) = {
        let image = load_icon_from_asset()
            .unwrap_or(image::load_from_memory(icon).context("Failed to open icon path")?)
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    let icon = tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .context("Failed to open icon")?;

    let event_loop = EventLoopBuilder::new().build();

    let tray_menu = Menu::new();
    let open_i = MenuItem::new(translate("Open".to_owned()), true, None);
    tray_menu.append_items(&[&open_i]).ok();
    let tooltip = |count: usize| {
        if count == 0 {
            format!(
                "{} {}",
                crate::get_app_name(),
                translate("Service is running".to_owned()),
            )
        } else {
            format!(
                "{} - {}\n{}",
                crate::get_app_name(),
                translate("Ready".to_owned()),
                translate("{".to_string() + &format!("{count}") + "} sessions"),
            )
        }
    };
    let mut _tray_icon: Arc<Mutex<Option<TrayIcon>>> = Default::default();

    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayEvent::receiver();
    let (ipc_sender, ipc_receiver) = std::sync::mpsc::channel::<Data>();

    let open_func = move || {
        {
            // Do not use "start uni link" way, it may not work on some Windows, and pop out error
            // dialog, I found on one user's desktop, but no idea why, Windows is shit.
            // Use `run_me` instead.
            // `allow_multiple_instances` in `flutter/windows/runner/main.cpp` allows only one instance without args.
            crate::run_me::<&str>(vec![]).ok();
        }
    };

    std::thread::spawn(move || {
        start_query_session_count(ipc_sender.clone());
    });
    let mut last_click = std::time::Instant::now();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        );

        if let tao::event::Event::NewEvents(tao::event::StartCause::Init) = event {
            // for fixing https://github.com/rustdesk/rustdesk/discussions/10210#discussioncomment-14600745
            // so we start tray, but not to show it
            if crate::ui_interface::get_builtin_option(keys::OPTION_HIDE_TRAY) == "Y" {
                return;
            }
            // We create the icon once the event loop is actually running
            // to prevent issues like https://github.com/tauri-apps/tray-icon/issues/90
            let mut builder = TrayIconBuilder::new()
                .with_id(crate::get_app_name().to_lowercase())
                .with_menu(Box::new(tray_menu.clone()))
                .with_tooltip(tooltip(0))
                .with_icon(icon.clone());
            {
                // Required since tray-icon 0.17
                // Fixes #15215, #15222, #15410
                builder = builder.with_menu_on_left_click(false);
            }
            let tray = builder.build();
            match tray {
                Ok(tray) => _tray_icon = Arc::new(Mutex::new(Some(tray))),
                Err(err) => {
                    log::error!("Failed to create tray icon: {}", err);
                }
            };

            // We have to request a redraw here to have the icon actually show up.
            // Tao only exposes a redraw method on the Window so we use core-foundation directly.
        }

        if let Ok(event) = menu_channel.try_recv() {
            if event.id == open_i.id() {
                open_func();
            }
        }

        if let Ok(_event) = tray_channel.try_recv() {
            match _event {
                TrayEvent::Click {
                    button,
                    button_state,
                    ..
                } => {
                    if button == tray_icon::MouseButton::Left
                        && button_state == tray_icon::MouseButtonState::Up
                    {
                        if last_click.elapsed() < std::time::Duration::from_secs(1) {
                            return;
                        }
                        open_func();
                        last_click = std::time::Instant::now();
                    }
                }
                _ => {}
            }
        }

        if let Ok(data) = ipc_receiver.try_recv() {
            match data {
                Data::ControlledSessionCount(count) => {
                    _tray_icon
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|t| t.set_tooltip(Some(tooltip(count))));
                }
                _ => {}
            }
        }
    });
}

#[tokio::main(flavor = "current_thread")]
async fn start_query_session_count(sender: std::sync::mpsc::Sender<Data>) {
    let mut last_count = 0;
    loop {
        if let Ok(mut c) = crate::ipc::connect(1000, "").await {
            let mut timer = crate::rustdesk_interval(tokio::time::interval(Duration::from_secs(1)));
            loop {
                tokio::select! {
                    res = c.next() => {
                        match res {
                            Err(err) => {
                                log::error!("ipc connection closed: {}", err);
                                break;
                            }

                            Ok(Some(Data::ControlledSessionCount(count))) => {
                                if count != last_count {
                                    last_count = count;
                                    sender.send(Data::ControlledSessionCount(count)).ok();
                                }
                            }
                            _ => {}
                        }
                    }

                    _ = timer.tick() => {
                        c.send(&Data::ControlledSessionCount(0)).await.ok();
                    }
                }
            }
        }
        hbb_common::sleep(1.).await;
    }
}

fn load_icon_from_asset() -> Option<image::DynamicImage> {
    let Some(path) = std::env::current_exe().map_or(None, |x| x.parent().map(|x| x.to_path_buf()))
    else {
        return None;
    };
    let path = path.join(r"data\flutter_assets\assets\icon.png");
    if path.exists() {
        if let Ok(image) = image::open(path) {
            return Some(image);
        }
    }
    None
}
