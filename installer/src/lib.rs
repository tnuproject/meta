//! META installer: choices, plans, and carrying plans out.
//!
//! The interface in [`app`] only produces a plan. [`execute`] is the one module
//! that writes to disks, and it is run by a separate program.

pub mod app;
pub mod catalog;
pub mod config;
pub mod disks;
pub mod execute;
pub mod plan;
pub mod screens;
pub mod ui;
