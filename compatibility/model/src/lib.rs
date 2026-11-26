//! The shared compatibility model.
//!
//! Everything MDCF, MLCE and MAES decide is expressed in the same terms:
//! [`Requirement`]s that something has, [`Evidence`] for each of them, what a
//! [`RuntimeKind`] provides according to the [`Registry`], and a [`Graph`] that
//! ties applications, interfaces, drivers and firmware together.

pub mod evidence;
pub mod graph;
pub mod registry;
pub mod requirement;

pub use evidence::{Evidence, Source};
pub use graph::{Edge, Graph, Node, NodeKind, Relation};
pub use registry::{Interface, Provision, Registry, RuntimeKind};
pub use requirement::{Need, Requirement};
