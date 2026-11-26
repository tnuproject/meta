//! The adaptive execution system: chooses the least demanding runtime that has
//! everything a program is known to need, and explains the choice.
//!
//! [`decide()`] is a pure function of a profile, the registry and the runtimes that
//! exist; [`host`] finds those, and [`launch`] starts the program.

pub mod decide;
pub mod host;
pub mod launch;

pub use decide::{Candidate, Decision, Unmet, Verdict, decide};
pub use host::{Guest, Host};
