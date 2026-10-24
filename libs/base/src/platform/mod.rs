pub mod windows;

#[cfg(not(debug_assertions))]
use hbb_common::{config::Config, log};
#[cfg(not(debug_assertions))]
use std::process::exit;

#[cfg(not(debug_assertions))]
static mut GLOBAL_CALLBACK: Option<Box<dyn Fn()>> = None;

#[cfg(not(debug_assertions))]
extern "C" fn breakdown_signal_handler(sig: i32) {
    let mut stack = vec![];
    backtrace::trace(|frame| {
        backtrace::resolve_frame(frame, |symbol| {
            if let Some(name) = symbol.name() {
                stack.push(name.to_string());
            }
        });
        true // keep going to the next frame
    });
    if stack.iter().any(|s| {
        s.contains(&"nouveau_pushbuf_kick")
            || s.to_lowercase().contains("nvidia")
            || s.contains("gdk_window_end_draw_frame")
            || s.contains("glGetString")
    }) {
        Config::set_option("allow-always-software-render".to_string(), "Y".to_string());
        log::info!("Always use software rendering will be set.");
    }
    if stack.iter().any(|s| {
        s.to_lowercase().contains("nvidia")
            || s.to_lowercase().contains("amf")
            || s.to_lowercase().contains("mfx")
            || s.contains("cuProfilerStop")
    }) {
        Config::set_option("enable-hwcodec".to_string(), "N".to_string());
        log::info!("Perhaps hwcodec causing the crash, disable it first");
    }
    log::error!(
        "Got signal {} and exit. stack:\n{}",
        sig,
        stack.join("\n").to_string()
    );
    unsafe {
        #[allow(static_mut_refs)]
        if let Some(callback) = &GLOBAL_CALLBACK {
            callback()
        }
    }
    exit(0);
}

#[cfg(not(debug_assertions))]
pub fn register_breakdown_handler<T>(callback: T)
where
    T: Fn() + 'static,
{
    unsafe {
        GLOBAL_CALLBACK = Some(Box::new(callback));
        libc::signal(libc::SIGSEGV, breakdown_signal_handler as _);
    }
}
