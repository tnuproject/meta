//! Linux compatibility roots.
//!
//! A root is a ZFS dataset holding a Linux distribution's userland, run as a
//! jail. The host's package manager never writes into it, and the distribution's
//! package manager runs only inside it.

pub mod bootstrap;
pub mod mounts;
pub mod root;

pub use root::{Root, RootError, State};
