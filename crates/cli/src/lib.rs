//! Cross-platform CLI primitives shared by the manager and display runtime.
pub mod arguments;
pub mod commands;
pub mod desktop;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod output;
