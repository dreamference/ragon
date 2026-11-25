//! The node's model-server supervisor (see Cargo.toml). `core` is platform-neutral and is the
//! module the Mac node reuses; `windows_job` and `windows_service` are Windows only.

pub mod advertise;
pub mod cli;
pub mod core;
pub mod firewall;
pub mod probe;
pub mod process;
#[cfg(windows)]
pub mod windows_job;
#[cfg(windows)]
pub mod windows_service;
