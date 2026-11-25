//! linux2BSD: choosing and diagnosing environments for Linux programs.
//!
//! This is not a translator. FreeBSD's Linuxulator implements the Linux system
//! call interface in the kernel; linux2BSD inspects a program, finds a suitable
//! Linux userland, and explains what is missing.

pub mod database;
pub mod decision;
pub mod elf;
pub mod resolve;
pub mod runtime;

pub use database::{Advice, Database, Severity};
pub use decision::{Decision, Report, analyze};
pub use elf::{Abi, Arch, Binary, ElfError, Libc, ProgramKind, inspect, inspect_bytes};
pub use resolve::{Resolution, resolve_libraries};
pub use runtime::{Distribution, Linuxulator, Runtime, discover};
