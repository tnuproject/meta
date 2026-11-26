//! Requirements that people wrote down.

use std::path::Path;

use meta_model::{Evidence, Requirement, Source};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    program: Vec<Program>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Program {
    names: Vec<String>,
    requires: Vec<Requirement>,
    reason: String,
    reference: String,
}

/// A list of programs whose documentation states what they need.
#[derive(Debug, Clone)]
pub struct Known(Vec<Program>);

impl Known {
    pub fn builtin() -> Self {
        Self::parse(include_str!("../data/programs.toml"))
            .expect("the embedded list is validated by the test suite")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
        if file.schema != 1 {
            return Err(format!("unsupported schema {}", file.schema));
        }
        for program in &file.program {
            if program.names.is_empty() || program.requires.is_empty() {
                return Err("an entry names no program or requires nothing".into());
            }
            if !program.reference.starts_with("https://") {
                return Err(format!("{}: no reference", program.names[0]));
            }
        }
        Ok(Self(file.program))
    }

    /// Needs of the program called `name`. A name is weak evidence, and a script
    /// with the same name may well do something else.
    pub fn needs(&self, name: &str, script: bool) -> Vec<(Requirement, Evidence)> {
        let source = if script {
            Source::Heuristic
        } else {
            Source::Database
        };
        self.0
            .iter()
            .filter(|program| program.names.iter().any(|n| n == name))
            .flat_map(|program| {
                program.requires.iter().map(move |requirement| {
                    let detail = format!("{name}: {}", program.reason);
                    let evidence = Evidence::new(source, detail).at(&*program.reference);
                    (requirement.clone(), evidence)
                })
            })
            .collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    schema: u32,
    requires: Vec<Requirement>,
}

/// Requirements in `PROGRAM.requirements.toml`. They can only add to what is
/// needed, so a program cannot talk its way into a less isolated runtime.
pub fn declared(program: &Path) -> Result<Vec<(Requirement, Evidence)>, String> {
    let mut name = program.as_os_str().to_owned();
    name.push(".requirements.toml");
    let path = Path::new(&name);
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let declaration: Declaration =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if declaration.schema != 1 {
        return Err(format!("{}: unsupported schema", path.display()));
    }
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(declaration
        .requires
        .into_iter()
        .map(|r| {
            (
                r,
                Evidence::new(Source::Declared, format!("declared in {file}")),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_is_valid() {
        let known = Known::builtin();
        let needs = known.needs("dockerd", false);
        assert!(
            needs
                .iter()
                .any(|(r, _)| r.to_string() == "kernel:namespaces")
        );
        assert!(
            needs
                .iter()
                .all(|(_, e)| e.source == Source::Database && e.reference.is_some())
        );
        assert!(
            known
                .needs("dockerd", true)
                .iter()
                .all(|(_, e)| e.source == Source::Heuristic)
        );
        assert!(known.needs("ls", false).is_empty());
    }

    #[test]
    fn declarations_are_read_and_mistakes_reported() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("app");
        assert!(declared(&program).unwrap().is_empty());
        let file = dir.path().join("app.requirements.toml");
        std::fs::write(&file, "schema = 1\nrequires = [\"kernel:cgroup-v2\"]\n").unwrap();
        assert_eq!(
            declared(&program).unwrap()[0].0.to_string(),
            "kernel:cgroup-v2"
        );
        for bad in [
            "schema = 2\nrequires = []\n",
            "schema = 1\nrequires = [\"x\"]\n",
            "x",
        ] {
            std::fs::write(&file, bad).unwrap();
            assert!(declared(&program).is_err(), "{bad}");
        }
    }
}
