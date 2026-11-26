//! Managed Linux virtual machines.
//!
//! A machine runs an unmodified Linux distribution, with its own kernel and init
//! system, under FreeBSD's bhyve. This is the only place where namespaces, cgroups,
//! systemd and Linux kernel modules exist on META.
//!
//! Everything bhyve, ssh and makefs are given is built as a vector of validated
//! arguments. Nothing is passed through a shell on the host.

pub mod audit;
pub mod bhyve;
pub mod config;
pub mod confine;
pub mod forward;
pub mod guest;
pub mod images;
pub mod manager;
pub mod quote;
pub mod seed;
pub mod share;

pub use config::{Cgroups, Forward, Network, Share, VmConfig, VmError};
pub use images::{Image, Images};
pub use manager::{Manager, VmState, VmStatus};
