//! The node's model-server supervisor (see Cargo.toml). `core` is platform-neutral and is the
//! module the Mac node reuses; `windows_job` and `windows_service` are Windows only.

pub mod cli;
pub mod core;
#[cfg(unix)]
pub mod footprint;
#[cfg(unix)]
pub mod jobs;
pub mod probe;
pub mod process;
pub mod seatbelt;
#[cfg(windows)]
pub mod windows_job;
#[cfg(windows)]
pub mod windows_service;
