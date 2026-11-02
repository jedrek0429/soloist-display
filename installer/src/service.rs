#[cfg(target_os = "linux")]
#[path = "service/linux.rs"]
mod native;
#[cfg(target_os = "macos")]
#[path = "service/macos.rs"]
mod native;
#[cfg(windows)]
#[path = "service/windows.rs"]
mod native;

pub use native::*;
