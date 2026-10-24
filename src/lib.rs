mod audio_resampler;
mod keyboard;
/// cbindgen:ignore
pub mod platform;
pub use platform::{
    clip_cursor, get_cursor, get_cursor_data, get_cursor_pos, get_focused_display,
    set_cursor_pos,
};
/// cbindgen:ignore
mod server;
pub use self::server::*;
mod client;
mod lan;
/// cbindgen:ignore
pub mod common;
pub mod ipc;
mod version;
pub use version::*;
mod bridge_generated;
pub mod flutter;
pub mod flutter_ffi;
use common::*;
mod auth_2fa;
mod clipboard;
pub mod core_main;
mod lang;

mod tray;

mod ui_cm_interface;
mod ui_interface;
mod ui_session_interface;


pub mod clipboard_file;


