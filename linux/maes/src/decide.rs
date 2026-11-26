//! The choice.

use std::collections::BTreeMap;

use meta_analyzer::{Kind, Profile};
use meta_model::{
    Evidence, Graph, Need, NodeKind, Provision, Registry, Relation, Requirement, RuntimeKind,
    Source,
};
use serde::Serialize;

/// A place where programs can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub kind: RuntimeKind,
    /// Edition or machine, when there can be several of a kind.
    pub instance: Option<String>,
    /// Whether it can be used right now.
    pub ready: bool,
    /// What has to be done first when it is not ready.
    pub preparation: Option<String>,
    /// Reasons, independent of the requirements, why this program cannot run here.
    pub obstacles: Vec<String>,
    /// What is known about this instance in particular. Overrides the registry.
    pub facts: BTreeMap<String, (Provision, Evidence)>,
    /// Lowest Linux version this instance is known to be.
    pub kernel: Option<(u32, u32)>,
}

impl Candidate {
    pub fn new(kind: RuntimeKind) -> Self {
        Self {
            kind,
            instance: None,
            ready: true,
            preparation: None,
            obstacles: Vec::new(),
            facts: BTreeMap::new(),
            kernel: None,
        }
    }

    pub fn fact(&mut self, requirement: &str, provision: Provision, source: Source, detail: &str) {
        let evidence = Evidence::new(source, detail);
        self.facts
            .insert(requirement.to_owned(), (provision, evidence));
    }

    pub fn label(&self) -> String {
        match &self.instance {
            Some(instance) => format!("{} ({instance})", self.kind),
            None => self.kind.to_string(),
        }
    }

    fn provision(
        &self,
        registry: &Registry,
        requirement: &Requirement,
    ) -> (Provision, Vec<Evidence>) {
        if let Some((provision, evidence)) = self.facts.get(&requirement.to_string()) {
            return (*provision, vec![evidence.clone()]);
        }
        match (requirement, self.kernel) {
            (Requirement::KernelVersion(major, minor), Some(version)) => {
                let enough = version >= (*major, *minor);
                let detail = format!("it reports Linux {}.{}", version.0, version.1);
                let provision = if enough {
                    Provision::Provided
                } else {
                    Provision::Missing
                };
                (provision, vec![Evidence::new(Source::Observation, detail)])
            }
            _ => registry.provision(self.kind, requirement),
        }
    }
}

/// A requirement that a candidate does not certainly provide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unmet {
    pub requirement: Requirement,
    pub provision: Provision,
    /// Whether the need is more than a hint.
    pub firm: bool,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Everything known to be needed is there.
    Suitable,
    /// Nothing known to be needed is missing, but not everything is certain.
    Uncertain,
    /// Something needed is missing.
    Unsuitable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Assessed {
    pub candidate: Candidate,
    pub verdict: Verdict,
    pub unmet: Vec<Unmet>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    /// In order of preference.
    pub assessed: Vec<Assessed>,
    /// Index of the choice.
    pub selected: Option<usize>,
    pub explanation: Vec<String>,
    pub graph: Graph,
}

impl Decision {
    pub fn choice(&self) -> Option<&Assessed> {
        self.selected.and_then(|index| self.assessed.get(index))
    }
}

/// Restrictions the user puts on the choice. They never widen it.
#[derive(Debug, Clone, Default)]
pub struct Preference {
    pub runtime: Option<RuntimeKind>,
    pub instance: Option<String>,
    /// Treat hints as requirements.
    pub strict: bool,
}

impl Preference {
    fn admits(&self, candidate: &Candidate) -> bool {
        self.runtime.is_none_or(|kind| kind == candidate.kind)
            && (self.instance.is_none() || self.instance == candidate.instance)
    }
}

/// Whether the choice of a runtime depends on this at all. Libraries are checked
/// against a particular userland instead, which shows up as an obstacle.
fn relevant(need: &Need) -> bool {
    !matches!(
        need.requirement,
        Requirement::Arch(_)
            | Requirement::Abi(_)
            | Requirement::Library(_)
            | Requirement::SymbolVersion(_)
    )
}

pub fn decide(
    profile: &Profile,
    registry: &Registry,
    candidates: Vec<Candidate>,
    preference: &Preference,
) -> Decision {
    let mut assessed: Vec<Assessed> = candidates
        .into_iter()
        .map(|candidate| assess(profile, registry, candidate, preference))
        .collect();
    // Within a verdict, the least demanding runtime and then a ready one. Sorting
    // is stable, so instances keep the order they were found in.
    assessed.sort_by_key(|a| (a.candidate.kind, !a.candidate.ready));
    let pick = |verdict| {
        assessed
            .iter()
            .position(|a| a.verdict == verdict && preference.admits(&a.candidate))
    };
    let selected = pick(Verdict::Suitable).or_else(|| pick(Verdict::Uncertain));
    let explanation = explain(profile, &assessed, selected, preference);
    let graph = graph(profile, &assessed, selected);
    Decision {
        assessed,
        selected,
        explanation,
        graph,
    }
}

fn assess(
    profile: &Profile,
    registry: &Registry,
    candidate: Candidate,
    preference: &Preference,
) -> Assessed {
    let mut reasons = candidate.obstacles.clone();
    let native = candidate.kind == RuntimeKind::Native;
    match profile.kind {
        Kind::FreeBsd if !native => reasons.push("it is a FreeBSD program".into()),
        Kind::Linux if native => reasons.push("it is a Linux program".into()),
        Kind::Other => reasons.push("it is not a program META can run".into()),
        _ => {}
    }
    let mut unmet = Vec::new();
    for need in profile.needs.iter().filter(|need| relevant(need)) {
        let (provision, evidence) = candidate.provision(registry, &need.requirement);
        if provision == Provision::Provided {
            continue;
        }
        let firm = need.is_firm() || preference.strict;
        if firm && matches!(provision, Provision::Missing) {
            reasons.push(format!("{} is not available", need.requirement));
        }
        unmet.push(Unmet {
            requirement: need.requirement.clone(),
            provision,
            firm,
            evidence: need.evidence.iter().cloned().chain(evidence).collect(),
        });
    }
    let verdict = if !reasons.is_empty() {
        Verdict::Unsuitable
    } else if unmet
        .iter()
        .any(|u| u.firm && u.provision != Provision::Partial)
    {
        Verdict::Uncertain
    } else {
        Verdict::Suitable
    };
    Assessed {
        candidate,
        verdict,
        unmet,
        reasons,
    }
}

fn explain(
    profile: &Profile,
    assessed: &[Assessed],
    selected: Option<usize>,
    preference: &Preference,
) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(index) = selected else {
        lines.push("No runtime can be used:".into());
        for a in assessed {
            let reason = if preference.admits(&a.candidate) {
                a.reasons.join("; ")
            } else {
                "excluded by the options given".into()
            };
            lines.push(format!("  {}: {reason}", a.candidate.label()));
        }
        if !assessed
            .iter()
            .any(|a| a.candidate.kind == RuntimeKind::VirtualMachine)
            && profile.kind != Kind::FreeBsd
        {
            lines.push(
                "  a virtual machine could provide more: sudo meta-linux vm create NAME".into(),
            );
        }
        return lines;
    };
    let choice = &assessed[index];
    lines.push(format!("{} is used.", choice.candidate.label()));
    if choice.candidate.kind == RuntimeKind::VirtualMachine {
        let firm: Vec<String> = profile
            .needs
            .iter()
            .filter(|n| relevant(n) && (n.is_firm() || preference.strict))
            .map(|n| n.requirement.to_string())
            .collect();
        if !firm.is_empty() {
            lines.push(format!("  needed: {}", firm.join(", ")));
        }
    }
    for a in &assessed[..index] {
        let reason = if preference.admits(&a.candidate) {
            a.reasons.join("; ")
        } else {
            "excluded by the options given".into()
        };
        let reason = if reason.is_empty() {
            "less certain".into()
        } else {
            reason
        };
        lines.push(format!("  not {}: {reason}", a.candidate.label()));
    }
    for unmet in &choice.unmet {
        let what = match unmet.provision {
            Provision::Missing => "is not available there",
            Provision::Partial => "is only partly available there",
            Provision::Configurable => "has to be set up there",
            Provision::Unknown => "may not be available there",
            Provision::Provided => continue,
        };
        let who = if unmet.firm { "needs" } else { "might use" };
        lines.push(format!(
            "  the program {who} {}, which {what}",
            unmet.requirement
        ));
    }
    if let Some(preparation) = &choice.candidate.preparation {
        lines.push(format!("  first: {preparation}"));
    }
    let missing = |a: &Assessed| {
        a.unmet
            .iter()
            .filter(|u| u.provision == Provision::Missing)
            .count()
    };
    let better = assessed[index + 1..]
        .iter()
        .find(|a| a.verdict != Verdict::Unsuitable && missing(a) < missing(choice));
    if let Some(better) = better {
        lines.push(format!(
            "  if it fails, try {} with --runtime {}",
            better.candidate.label(),
            better.candidate.kind
        ));
    }
    lines
}

/// Every node is added before its edges, and every edge has evidence.
const COMPLETE: &str = "nodes and evidence exist";

fn graph(profile: &Profile, assessed: &[Assessed], selected: Option<usize>) -> Graph {
    let mut graph = Graph::new();
    let program = profile.program.display().to_string();
    graph.node("program", NodeKind::Application, program);
    for need in profile.needs.iter().filter(|n| relevant(n)) {
        let id = need.requirement.to_string();
        graph.node(id.clone(), NodeKind::Requirement, id.clone());
        graph
            .edge("program", &id, Relation::Requires, need.evidence.clone())
            .expect(COMPLETE);
    }
    for (index, a) in assessed.iter().enumerate() {
        let id = a.candidate.label();
        graph.node(id.clone(), NodeKind::Runtime, id.clone());
        for unmet in &a.unmet {
            let relation = match unmet.provision {
                Provision::Missing => Relation::Lacks,
                _ => Relation::Provides,
            };
            let requirement = unmet.requirement.to_string();
            graph
                .edge(&id, &requirement, relation, unmet.evidence.clone())
                .expect(COMPLETE);
        }
        let relation = if Some(index) == selected {
            Relation::Selected
        } else if a.verdict == Verdict::Unsuitable {
            Relation::Rejected
        } else {
            continue;
        };
        let mut evidence: Vec<Evidence> = a
            .reasons
            .iter()
            .map(|reason| Evidence::new(Source::StaticAnalysis, reason.clone()))
            .collect();
        if evidence.is_empty() {
            let detail = "nothing known to be needed is missing";
            evidence.push(Evidence::new(Source::StaticAnalysis, detail));
        }
        graph
            .edge("program", &id, relation, evidence)
            .expect(COMPLETE);
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(kind: Kind, needs: &[(&str, Source)]) -> Profile {
        Profile {
            program: "/x/app".into(),
            kind,
            interpreter: None,
            binary: None,
            needs: needs
                .iter()
                .map(|(text, source)| {
                    Need::new(text.parse().unwrap(), Evidence::new(*source, "test"))
                })
                .collect(),
            limitations: Vec::new(),
        }
    }

    fn all() -> Vec<Candidate> {
        // Not in order of preference, which must not depend on it.
        let mut vm = Candidate::new(RuntimeKind::VirtualMachine);
        vm.instance = Some("debian".into());
        vm.kernel = Some((6, 1));
        let mut linuxulator = Candidate::new(RuntimeKind::Linuxulator);
        linuxulator.kernel = Some((5, 15));
        let mut userspace = Candidate::new(RuntimeKind::Userspace);
        userspace.kernel = Some((5, 15));
        vec![
            vm,
            userspace,
            linuxulator,
            Candidate::new(RuntimeKind::Native),
        ]
    }

    fn chosen(profile: &Profile, candidates: Vec<Candidate>) -> Option<RuntimeKind> {
        decide(
            profile,
            &Registry::builtin(),
            candidates,
            &Preference::default(),
        )
        .choice()
        .map(|a| a.candidate.kind)
    }

    const FIRM: Source = Source::Declared;
    const HINT: Source = Source::Heuristic;

    #[test]
    fn least_demanding_runtime_wins() {
        assert_eq!(
            chosen(&profile(Kind::FreeBsd, &[]), all()),
            Some(RuntimeKind::Native)
        );
        assert_eq!(
            chosen(&profile(Kind::Linux, &[]), all()),
            Some(RuntimeKind::Linuxulator)
        );
        assert_eq!(chosen(&profile(Kind::Other, &[]), all()), None);
    }

    #[test]
    fn kernel_features_lead_to_the_machine() {
        for need in [
            "kernel:namespaces",
            "kernel:cgroup-v2",
            "init:systemd",
            "syscall:bpf",
        ] {
            let decision = chosen(&profile(Kind::Linux, &[(need, FIRM)]), all());
            assert_eq!(decision, Some(RuntimeKind::VirtualMachine), "{need}");
        }
    }

    #[test]
    fn hints_never_rule_a_runtime_out() {
        let hinted = profile(Kind::Linux, &[("kernel:namespaces", HINT)]);
        let decision = decide(&hinted, &Registry::builtin(), all(), &Preference::default());
        assert_eq!(
            decision.choice().unwrap().candidate.kind,
            RuntimeKind::Linuxulator
        );
        let text = decision.explanation.join("\n");
        assert!(text.contains("might use kernel:namespaces"), "{text}");
        assert!(
            text.contains("if it fails, try virtual-machine (debian)"),
            "{text}"
        );
        let strict = Preference {
            strict: true,
            ..Preference::default()
        };
        let decision = decide(&hinted, &Registry::builtin(), all(), &strict);
        assert_eq!(
            decision.choice().unwrap().candidate.kind,
            RuntimeKind::VirtualMachine
        );
    }

    #[test]
    fn kernel_versions_are_compared() {
        let new = profile(Kind::Linux, &[("kernel-version:6.0", FIRM)]);
        assert_eq!(chosen(&new, all()), Some(RuntimeKind::VirtualMachine));
        let newer = profile(Kind::Linux, &[("kernel-version:6.19", FIRM)]);
        assert_eq!(chosen(&newer, all()), None);
        let old = profile(Kind::Linux, &[("kernel-version:3.2", FIRM)]);
        assert_eq!(chosen(&old, all()), Some(RuntimeKind::Linuxulator));
    }

    #[test]
    fn instances_with_the_feature_win_over_those_that_could_have_it() {
        let needs = profile(Kind::Linux, &[("kernel:cgroup-v1", FIRM)]);
        let mut v1 = Candidate::new(RuntimeKind::VirtualMachine);
        v1.instance = Some("old".into());
        v1.fact(
            "kernel:cgroup-v1",
            Provision::Provided,
            Source::Observation,
            "configured",
        );
        let mut v2 = Candidate::new(RuntimeKind::VirtualMachine);
        v2.instance = Some("new".into());
        v2.fact(
            "kernel:cgroup-v1",
            Provision::Missing,
            Source::Observation,
            "configured",
        );
        let decision = decide(
            &needs,
            &Registry::builtin(),
            vec![v2, v1],
            &Preference::default(),
        );
        assert_eq!(
            decision.choice().unwrap().candidate.instance.as_deref(),
            Some("old")
        );
    }

    #[test]
    fn preferences_restrict_but_do_not_override() {
        let needs = profile(Kind::Linux, &[("init:systemd", FIRM)]);
        let preference = Preference {
            runtime: Some(RuntimeKind::Linuxulator),
            ..Preference::default()
        };
        let decision = decide(&needs, &Registry::builtin(), all(), &preference);
        assert!(decision.choice().is_none());
        let plain = profile(Kind::Linux, &[]);
        let preference = Preference {
            runtime: Some(RuntimeKind::VirtualMachine),
            ..Preference::default()
        };
        let decision = decide(&plain, &Registry::builtin(), all(), &preference);
        assert_eq!(
            decision.choice().unwrap().candidate.kind,
            RuntimeKind::VirtualMachine
        );
    }

    #[test]
    fn obstacles_reject_and_are_explained() {
        let mut candidates = all();
        for candidate in &mut candidates {
            candidate.obstacles.push("libfoo.so.1 is missing".into());
        }
        let decision = decide(
            &profile(Kind::Linux, &[]),
            &Registry::builtin(),
            candidates,
            &Preference::default(),
        );
        assert!(decision.choice().is_none());
        assert!(
            decision
                .explanation
                .join("\n")
                .contains("libfoo.so.1 is missing")
        );
        assert!(decision.graph.to_dot().contains("program"));
    }

    #[test]
    fn ready_instances_come_first() {
        let mut stopped = Candidate::new(RuntimeKind::VirtualMachine);
        stopped.instance = Some("a".into());
        stopped.ready = false;
        stopped.preparation = Some("start it".into());
        let mut running = Candidate::new(RuntimeKind::VirtualMachine);
        running.instance = Some("b".into());
        let decision = decide(
            &profile(Kind::Linux, &[("init:systemd", FIRM)]),
            &Registry::builtin(),
            vec![stopped, running],
            &Preference::default(),
        );
        assert_eq!(
            decision.choice().unwrap().candidate.instance.as_deref(),
            Some("b")
        );
    }
}
