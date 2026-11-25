//! The node's model-server supervisor (see Cargo.toml). `core` is platform-neutral and shared by
//! the Windows and Mac nodes; `windows_job` and `windows_service` are Windows only, `macos_guard`
//! macOS only. `macos_launchd` and `macos_power` are pure (the LaunchDaemon plist, the `launchctl`
//! and `pmset` arguments) and compile everywhere, so their tests run on Linux.

pub mod cli;
pub mod core;
#[cfg(target_os = "macos")]
pub mod macos_guard;
pub mod macos_launchd;
pub mod macos_power;
pub mod probe;
pub mod process;
#[cfg(windows)]
pub mod windows_job;
#[cfg(windows)]
pub mod windows_service;
