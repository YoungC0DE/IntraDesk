pub use windows::*;

pub mod windows;

pub mod win_device;

use base::message_proto::CursorData;
#[cfg(not(not(target_pointer_width = "64")))]
use hbb_common::sysinfo::System;
use hbb_common::{sysinfo::Pid, ResultType};

pub const SERVICE_INTERVAL: u64 = 300;

pub fn is_xfce() -> bool {
    {
        return false;
    }
}

pub fn breakdown_callback() {
    crate::input_service::release_device_modifiers();
}

pub fn change_resolution(name: &str, width: usize, height: usize) -> ResultType<()> {
    let cur_resolution = current_resolution(name)?;
    // For MacOS
    // to-do: Make sure the following comparison works.
    // For Linux
    // Just run "xrandr", dpi may not be taken into consideration.
    // For Windows
    // dmPelsWidth and dmPelsHeight is the same to width and height
    // Because this process is running in dpi awareness mode.
    if cur_resolution.width as usize == width && cur_resolution.height as usize == height {
        return Ok(());
    }
    hbb_common::log::warn!("Change resolution of '{}' to ({},{})", name, width, height);
    change_resolution_directly(name, width, height)
}

// Android




pub fn get_wakelock(_display: bool) -> WakeLock {
    hbb_common::log::info!("new wakelock, require display on: {_display}");
    // display: keep screen on
    // idle: keep cpu on
    // sleep: prevent system from sleeping, even manually
    return crate::platform::WakeLock::new(_display, true, false);
}


// Note: This method is inefficient on Windows. It will get all the processes.
// It should only be called when performance is not critical.
// If we wanted to get the command line ourselves, there would be a lot of new code.
#[allow(dead_code)]
fn get_pids_of_process_with_args<S1: AsRef<str>, S2: AsRef<str>>(
    name: S1,
    args: &[S2],
) -> Vec<Pid> {
    // This function does not work when the process is 32-bit and the OS is 64-bit Windows,
    // `process.cmd()` always returns [] in this case.
    // So we use `windows::get_pids_with_args_by_wmic()` instead.
    #[cfg(not(target_pointer_width = "64"))]
    {
        return windows::get_pids_with_args_by_wmic(name, args);
    }
    #[cfg(not(not(target_pointer_width = "64")))]
    {
        let name = name.as_ref().to_lowercase();
        let system = System::new_all();
        system
            .processes()
            .iter()
            .filter(|(_, process)| {
                process.name().to_lowercase() == name
                    && process.cmd().len() == args.len() + 1
                    && args.iter().enumerate().all(|(i, arg)| {
                        process.cmd()[i + 1].to_lowercase() == arg.as_ref().to_lowercase()
                    })
            })
            .map(|(&pid, _)| pid)
            .collect()
    }
}

// Note: This method is inefficient on Windows. It will get all the processes.
// It should only be called when performance is not critical.
pub fn get_pids_of_process_with_first_arg<S1: AsRef<str>, S2: AsRef<str>>(
    name: S1,
    arg: S2,
) -> Vec<Pid> {
    // This function does not work when the process is 32-bit and the OS is 64-bit Windows,
    // `process.cmd()` always returns [] in this case.
    // So we use `windows::get_pids_with_first_arg_by_wmic()` instead.
    #[cfg(not(target_pointer_width = "64"))]
    {
        return windows::get_pids_with_first_arg_by_wmic(name, arg);
    }
    #[cfg(not(not(target_pointer_width = "64")))]
    {
        let name = name.as_ref().to_lowercase();
        let system = System::new_all();
        system
            .processes()
            .iter()
            .filter(|(_, process)| {
                process.name().to_lowercase() == name
                    && process.cmd().len() >= 2
                    && process.cmd()[1].to_lowercase() == arg.as_ref().to_lowercase()
            })
            .map(|(&pid, _)| pid)
            .collect()
    }
}
