//! The compatibility graph.
//!
//! Nodes are applications, requirements, runtimes, devices, drivers and firmware;
//! edges carry the evidence for the relation. The graph answers "why" questions
//! and is the interchange format between MDCF and MLCE.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::evidence::Evidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeKind {
    Application,
    Requirement,
    Runtime,
    Capability,
    Device,
    Driver,
    KernelApi,
    LinuxKpi,
    Firmware,
    Userspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Relation {
    Requires,
    Provides,
    /// The source cannot provide the target.
    Lacks,
    DrivenBy,
    Selected,
    Rejected,
    ConflictsWith,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub relation: Relation,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node, keeping the first one with an identifier.
    pub fn node(&mut self, id: impl Into<String>, kind: NodeKind, label: impl Into<String>) {
        let id = id.into();
        self.nodes.entry(id.clone()).or_insert_with(|| Node {
            id,
            kind,
            label: label.into(),
        });
    }

    /// Adds an edge. Both nodes must exist; an edge to nowhere would be silently
    /// meaningless.
    pub fn edge(
        &mut self,
        from: &str,
        to: &str,
        relation: Relation,
        evidence: Vec<Evidence>,
    ) -> Result<(), String> {
        for id in [from, to] {
            if !self.nodes.contains_key(id) {
                return Err(format!("no node {id:?}"));
            }
        }
        let edge = Edge {
            from: from.to_owned(),
            to: to.to_owned(),
            relation,
            evidence,
        };
        if !self.edges.contains(&edge) {
            self.edges.push(edge);
        }
        Ok(())
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn from<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Edge> {
        self.edges.iter().filter(move |edge| edge.from == id)
    }

    /// Everything `id` requires, directly or through other requirements.
    pub fn requirements(&self, id: &str) -> BTreeSet<&str> {
        let mut found = BTreeSet::new();
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            for edge in self.edges.iter().filter(|e| e.from == current) {
                let follows = matches!(edge.relation, Relation::Requires | Relation::DrivenBy);
                if follows && found.insert(edge.to.as_str()) {
                    pending.push(&edge.to);
                }
            }
        }
        found
    }

    /// Requirements of `id` that nothing in the graph provides.
    pub fn unmet(&self, id: &str) -> Vec<&Node> {
        self.requirements(id)
            .into_iter()
            .filter(|target| {
                !self
                    .edges
                    .iter()
                    .any(|e| e.to == *target && e.relation == Relation::Provides)
            })
            .filter_map(|target| self.nodes.get(target))
            .filter(|node| node.kind == NodeKind::Requirement)
            .collect()
    }

    /// Graphviz form, for people.
    pub fn to_dot(&self) -> String {
        let escape = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
        let mut dot = String::from("digraph compatibility {\n  rankdir=LR;\n");
        for node in self.nodes.values() {
            dot.push_str(&format!(
                "  \"{}\" [label=\"{}\"];\n",
                escape(&node.id),
                escape(&node.label)
            ));
        }
        for edge in &self.edges {
            let relation = format!("{:?}", edge.relation).to_lowercase();
            dot.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{relation}\"];\n",
                escape(&edge.from),
                escape(&edge.to)
            ));
        }
        dot.push_str("}\n");
        dot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Graph {
        let mut graph = Graph::new();
        graph.node("app", NodeKind::Application, "app");
        graph.node("kernel:namespaces", NodeKind::Requirement, "namespaces");
        graph.node("abi:linux", NodeKind::Requirement, "linux");
        graph.node("vm", NodeKind::Runtime, "vm \"debian\"");
        graph
            .edge("app", "kernel:namespaces", Relation::Requires, vec![])
            .unwrap();
        graph
            .edge("app", "abi:linux", Relation::Requires, vec![])
            .unwrap();
        graph
            .edge("vm", "abi:linux", Relation::Provides, vec![])
            .unwrap();
        graph
    }

    #[test]
    fn unmet_requirements_are_found() {
        let mut graph = sample();
        let unmet: Vec<_> = graph.unmet("app").iter().map(|n| n.id.clone()).collect();
        assert_eq!(unmet, ["kernel:namespaces"]);
        graph
            .edge("vm", "kernel:namespaces", Relation::Provides, vec![])
            .unwrap();
        assert!(graph.unmet("app").is_empty());
    }

    #[test]
    fn cycles_terminate() {
        let mut graph = sample();
        graph
            .edge("abi:linux", "app", Relation::Requires, vec![])
            .unwrap();
        assert!(graph.requirements("app").contains("app"));
    }

    #[test]
    fn edges_need_both_ends_and_are_not_duplicated() {
        let mut graph = sample();
        assert!(
            graph
                .edge("app", "nowhere", Relation::Requires, vec![])
                .is_err()
        );
        let before = graph.edges().len();
        graph
            .edge("vm", "abi:linux", Relation::Provides, vec![])
            .unwrap();
        assert_eq!(graph.edges().len(), before);
    }

    #[test]
    fn json_round_trips_and_dot_is_escaped() {
        let graph = sample();
        let json = serde_json::to_string(&graph).unwrap();
        assert_eq!(serde_json::from_str::<Graph>(&json).unwrap(), graph);
        assert!(graph.to_dot().contains("vm \\\"debian\\\""));
    }
}
