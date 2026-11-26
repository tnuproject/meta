//! Preparing, installing, enabling, disabling and removing driver components.
//!
//! Every operation is first a [`Plan`]: exact commands, what each is for, and whether
//! it can be undone. Nothing here unloads a module that is in use, replaces a kernel
//! module, edits files other than through sysrc(8), or weakens a protection.

use std::collections::BTreeSet;

use meta_common::{CommandError, CommandRunner};
use meta_eds_db::{Entry, Support, Validation};
use serde::Serialize;

use crate::evaluate::{Assessment, Verdict};
use crate::firmware::valid_package;
use crate::host::{HostFacts, PKG};

const KLDLOAD: &str = "/sbin/kldload";
const SYSRC: &str = "/usr/sbin/sysrc";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    /// Download without installing.
    Prepare,
    Install,
    /// Load now and at every boot.
    Enable,
    /// Stop loading at boot. Never unloads.
    Disable,
    /// Remove packages META's entry names.
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub description: String,
    pub command: Vec<String>,
    /// How to undo it, when that is possible.
    pub undo: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub operation: Operation,
    pub entry: String,
    pub steps: Vec<Step>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("{0} is not compatible with this system: {1}")]
    Incompatible(String, String),
    #[error("{0} has not been tested by META; repeat with --accept-untested to continue")]
    Untested(String),
    #[error("{0} names an invalid package or module")]
    InvalidName(String),
    #[error("removing {package} would also remove {}", others.join(", "))]
    StillNeeded {
        package: String,
        others: Vec<String>,
    },
    #[error("{0} is handled by the kernel itself; there is nothing to do")]
    Nothing(String),
}

pub struct Request<'a> {
    pub operation: Operation,
    pub entry: &'a Entry,
    pub assessment: Option<&'a Assessment>,
    pub accept_untested: bool,
}

impl std::fmt::Debug for Request<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("operation", &self.operation)
            .field("entry", &self.entry.id)
            .finish()
    }
}

fn command(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// Builds a plan. `dependents` are installed packages that need the entry's packages.
pub fn plan(
    request: &Request<'_>,
    host: &HostFacts,
    dependents: &BTreeSet<String>,
) -> Result<Plan, PlanError> {
    let entry = request.entry;
    let adds = matches!(
        request.operation,
        Operation::Prepare | Operation::Install | Operation::Enable
    );
    if adds {
        check_allowed(request)?;
    }
    for name in entry
        .packages
        .iter()
        .chain(&entry.firmware)
        .chain(&entry.modules)
    {
        if !valid_package(name) {
            return Err(PlanError::InvalidName(name.clone()));
        }
    }
    let mut steps = Vec::new();
    let mut notes = Vec::new();
    // Firmware is packaged too; modules of the same name are not packages.
    let packages: Vec<&String> = entry.packages.iter().chain(&entry.firmware).collect();
    match request.operation {
        Operation::Prepare | Operation::Install => {
            for package in packages.iter().filter(|p| !host.packages.contains(**p)) {
                if request.operation == Operation::Prepare {
                    steps.push(Step {
                        description: format!("download {package} and what it needs"),
                        command: command(&[PKG, "fetch", "-y", "-d", package]),
                        undo: None,
                    });
                } else {
                    steps.push(Step {
                        description: format!("install {package}"),
                        command: command(&[PKG, "install", "-y", package]),
                        undo: Some(command(&[PKG, "delete", "-y", package])),
                    });
                }
            }
        }
        Operation::Enable => {
            for module in &entry.modules {
                if !host.has_module(module) {
                    notes.push(format!("{module} has to be installed first"));
                    continue;
                }
                let setting = format!("kld_list+={module}");
                let undo = format!("kld_list-={module}");
                steps.push(Step {
                    description: format!("load {module} at every boot"),
                    command: command(&[SYSRC, &setting]),
                    undo: Some(command(&[SYSRC, &undo])),
                });
                if !host.is_loaded(module) {
                    steps.push(Step {
                        description: format!("load {module} now"),
                        command: command(&[KLDLOAD, "-n", module]),
                        // Unloading a driver in use can take the machine down.
                        undo: None,
                    });
                }
            }
        }
        Operation::Disable => {
            for module in &entry.modules {
                let setting = format!("kld_list-={module}");
                let undo = format!("kld_list+={module}");
                steps.push(Step {
                    description: format!("stop loading {module} at boot"),
                    command: command(&[SYSRC, &setting]),
                    undo: Some(command(&[SYSRC, &undo])),
                });
                if host.is_loaded(module) {
                    notes.push(format!("{module} stays loaded until the next restart"));
                }
            }
        }
        Operation::Remove => {
            if !dependents.is_empty() {
                return Err(PlanError::StillNeeded {
                    package: entry.packages.join(", "),
                    others: dependents.iter().cloned().collect(),
                });
            }
            for package in packages.iter().filter(|p| host.packages.contains(**p)) {
                steps.push(Step {
                    description: format!("remove {package}"),
                    command: command(&[PKG, "delete", "-y", package]),
                    undo: Some(command(&[PKG, "install", "-y", package])),
                });
            }
            for module in entry.modules.iter().filter(|m| host.is_loaded(m)) {
                notes.push(format!("{module} stays loaded until the next restart"));
            }
        }
    }
    if steps.is_empty() && notes.is_empty() {
        return Err(PlanError::Nothing(entry.id.clone()));
    }
    Ok(Plan {
        operation: request.operation,
        entry: entry.id.clone(),
        steps,
        notes,
    })
}

fn check_allowed(request: &Request<'_>) -> Result<(), PlanError> {
    let entry = request.entry;
    if entry.support == Support::Unsupported {
        return Err(PlanError::Incompatible(
            entry.id.clone(),
            "known not to work".into(),
        ));
    }
    if let Some(assessment) = request.assessment
        && assessment.verdict == Verdict::Incompatible
    {
        return Err(PlanError::Incompatible(
            entry.id.clone(),
            assessment.reason.clone(),
        ));
    }
    if entry.validation != Validation::Validated && !request.accept_untested {
        return Err(PlanError::Untested(entry.id.clone()));
    }
    Ok(())
}

/// Installed packages outside the entry that need its packages.
pub fn dependents(
    runner: &dyn CommandRunner,
    entry: &Entry,
) -> Result<BTreeSet<String>, CommandError> {
    let mut found = BTreeSet::new();
    for package in entry.packages.iter().filter(|p| valid_package(p)) {
        let output = runner.run(PKG, &["query", "%rn", package])?;
        found.extend(output.stdout.lines().map(str::to_owned));
    }
    Ok(found
        .into_iter()
        .filter(|name| !entry.packages.contains(name))
        .collect())
}

/// Carries a plan out, stopping at the first failure.
pub fn execute(
    plan: &Plan,
    runner: &dyn CommandRunner,
    progress: &mut dyn FnMut(&Step),
) -> Result<(), CommandError> {
    for step in &plan.steps {
        progress(step);
        let Some((program, arguments)) = step.command.split_first() else {
            continue;
        };
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        runner.inherit(program, &arguments)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::CommandOutput;
    use meta_eds_db::Database;
    use std::cell::RefCell;

    fn entry(validation: &str, extra: &str) -> Entry {
        let tested = if validation == "validated" {
            "[[entry.tested_on]]\nrelease = \"15.1\"\nmachine = \"m\"\ndate = \"d\"\nresult = \"r\"\n"
        } else {
            ""
        };
        let text = format!(
            "schema = 2\n[[entry]]\nid = \"x\"\nname = \"X\"\ncategory = \"gpu\"\nsupport = \"native\"\n\
             validation = \"{validation}\"\nsource = \"https://example.org\"\n{extra}\n{tested}"
        );
        Database::parse(&[("t", &text)]).unwrap().entries()[0].clone()
    }

    fn host(packages: &[&str], loaded: &[&str], available: &[&str]) -> HostFacts {
        let set = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
        HostFacts {
            packages: set(packages),
            loaded: set(loaded),
            available: set(available),
            ..HostFacts::default()
        }
    }

    fn make(operation: Operation, entry: &Entry, host: &HostFacts) -> Result<Plan, PlanError> {
        let request = Request {
            operation,
            entry,
            assessment: None,
            accept_untested: false,
        };
        plan(&request, host, &BTreeSet::new())
    }

    const FULL: &str = "packages = [\"p\"]\nmodules = [\"m\"]";

    #[test]
    fn untested_components_need_acceptance_to_add_but_not_to_remove() {
        let untested = entry("documented", FULL);
        let empty = host(&[], &[], &["m"]);
        for operation in [Operation::Prepare, Operation::Install, Operation::Enable] {
            assert!(matches!(
                make(operation, &untested, &empty),
                Err(PlanError::Untested(_))
            ));
        }
        let request = Request {
            operation: Operation::Install,
            entry: &untested,
            assessment: None,
            accept_untested: true,
        };
        assert_eq!(
            plan(&request, &empty, &BTreeSet::new())
                .unwrap()
                .steps
                .len(),
            1
        );
        assert!(make(Operation::Remove, &untested, &host(&["p"], &[], &[])).is_ok());
    }

    #[test]
    fn nothing_ever_unloads_a_module() {
        let tested = entry("validated", FULL);
        let busy = host(&["p"], &["m"], &["m"]);
        for operation in [Operation::Enable, Operation::Disable, Operation::Remove] {
            let plan = make(operation, &tested, &busy).unwrap();
            let text = format!("{plan:?}");
            assert!(!text.contains("kldunload"), "{text}");
        }
        let disable = make(Operation::Disable, &tested, &busy).unwrap();
        assert!(disable.notes[0].contains("stays loaded"));
    }

    #[test]
    fn plans_name_exact_commands_and_their_reverse() {
        let plan = make(
            Operation::Enable,
            &entry("validated", FULL),
            &host(&[], &[], &["m"]),
        )
        .unwrap();
        assert_eq!(plan.steps[0].command, ["/usr/sbin/sysrc", "kld_list+=m"]);
        assert_eq!(plan.steps[0].undo.as_ref().unwrap()[1], "kld_list-=m");
        assert_eq!(plan.steps[1].command, ["/sbin/kldload", "-n", "m"]);
        assert!(plan.steps[1].undo.is_none());
    }

    #[test]
    fn removal_stops_when_something_else_needs_it() {
        let request = Request {
            operation: Operation::Remove,
            entry: &entry("validated", FULL),
            assessment: None,
            accept_untested: false,
        };
        let others = BTreeSet::from(["xorg".to_owned()]);
        let error = plan(&request, &host(&["p"], &[], &[]), &others).unwrap_err();
        assert!(matches!(error, PlanError::StillNeeded { .. }), "{error}");
    }

    #[test]
    fn hostile_names_never_reach_a_command() {
        for extra in [
            "packages = [\"-y\"]",
            "modules = [\"a;b\"]",
            "firmware = [\"../x\"]",
        ] {
            let result = make(
                Operation::Install,
                &entry("validated", extra),
                &host(&[], &[], &[]),
            );
            assert!(matches!(result, Err(PlanError::InvalidName(_))), "{extra}");
        }
    }

    #[test]
    fn incompatible_drivers_are_refused_even_when_accepted() {
        let unsupported = entry("documented", "");
        let mut unsupported = unsupported;
        unsupported.support = Support::Unsupported;
        let request = Request {
            operation: Operation::Install,
            entry: &unsupported,
            assessment: None,
            accept_untested: true,
        };
        let error = plan(&request, &host(&[], &[], &[]), &BTreeSet::new()).unwrap_err();
        assert!(matches!(error, PlanError::Incompatible(..)));
    }

    struct Failing(RefCell<Vec<String>>);

    impl CommandRunner for Failing {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
            self.0
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn execution_stops_at_the_first_failure() {
        let plan = make(
            Operation::Enable,
            &entry("validated", FULL),
            &host(&[], &[], &["m"]),
        )
        .unwrap();
        let runner = Failing(RefCell::default());
        assert!(execute(&plan, &runner, &mut |_| {}).is_err());
        assert_eq!(runner.0.borrow().len(), 1);
    }
}
