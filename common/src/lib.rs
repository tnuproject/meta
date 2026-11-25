//! Types shared by META userspace components.

pub mod command;
pub mod platform;
pub mod status;
pub mod version;

pub use command::{CommandError, CommandOutput, CommandRunner, SystemRunner};
pub use platform::{Os, Platform};
pub use status::{Component, Status, components};
pub use version::{CODENAME, MetaVersion, PROJECT, RELEASE_STAGE, TAGLINE, VERSION};
