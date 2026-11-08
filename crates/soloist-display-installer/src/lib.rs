//! Shared installer package. Native lifecycle adapters are isolated from the Display runtime.
#[cfg(any(windows, target_os = "macos"))]
pub mod desktop_lifecycle;
