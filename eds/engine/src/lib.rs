//! META Driver Compatibility Framework.
//!
//! Evaluates drivers for detected hardware, explains the verdict, manages firmware
//! and driver components through inspectable plans, and reports what hardware can
//! actually do. Everything that only looks is free of side effects; everything that
//! changes the system is a [`lifecycle::Plan`] that has to be executed explicitly.

pub mod capability;
pub mod evaluate;
pub mod firmware;
pub mod host;
pub mod lifecycle;
pub mod linuxkpi;
pub mod predict;

pub use capability::{Capability, CapabilityState};
pub use evaluate::{Assessment, Check, Outcome, Verdict, assess, assess_device};
pub use firmware::{FirmwareReport, FirmwareState};
pub use host::HostFacts;
pub use lifecycle::{Operation, Plan, PlanError};
pub use linuxkpi::{InterfaceState, KpiRegistry};
pub use predict::{Prediction, predict};
