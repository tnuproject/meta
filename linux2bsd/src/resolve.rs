//! Shared library resolution inside a Linux userland.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::elf::{Abi, Arch, Binary, inspect};

const MAX_SYMLINKS: usize = 40;
/// Bounds the dependency walk on malformed input.
const MAX_LIBRARIES: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Resolution {
    /// Library name to its path inside the root.
    pub found: BTreeMap<String, String>,
    /// Missing library and the object that needs it.
    pub missing: BTreeMap<String, String>,
    /// Missing symbol version, as `library version`, and the object that needs it.
    pub missing_versions: BTreeMap<String, String>,
    /// The program's loader is absent from the root.
    pub interpreter_missing: bool,
}

impl Resolution {
    pub fn complete(&self) -> bool {
        self.missing.is_empty() && self.missing_versions.is_empty() && !self.interpreter_missing
    }
}

/// Resolves `path` as a process confined to `root` would, following symbolic
/// links. Absolute links and `..` stay inside the root. `None` if absent.
pub fn path_in_root(root: &Path, path: &str) -> Option<PathBuf> {
    let mut pending: VecDeque<String> = split(path);
    let mut inside = PathBuf::new();
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        if part == ".." {
            inside.pop();
            continue;
        }
        let candidate = inside.join(&part);
        let metadata = fs::symlink_metadata(root.join(&candidate)).ok()?;
        if metadata.file_type().is_symlink() {
            links += 1;
            if links > MAX_SYMLINKS {
                return None;
            }
            let target = fs::read_link(root.join(&candidate)).ok()?;
            let target = target.to_str()?;
            if target.starts_with('/') {
                inside = PathBuf::new();
            }
            for part in split(target).into_iter().rev() {
                pending.push_front(part);
            }
        } else {
            inside = candidate;
        }
    }
    Some(root.join(inside))
}

fn split(path: &str) -> VecDeque<String> {
    path.split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .map(str::to_owned)
        .collect()
}

/// Resolves the program's loader and all libraries it needs, transitively.
///
/// Symbol versions are checked too: a library that is present but too old is the
/// usual reason a program built for a newer distribution fails.
///
/// Follows the documented loader order: run paths, `ld.so.conf`, defaults. It
/// does not read `ld.so.cache`, `LD_LIBRARY_PATH` or hardware-specific
/// subdirectories, so an unusual setup can be reported as incomplete.
pub fn resolve_libraries(root: &Path, program: &Path, binary: &Binary) -> Resolution {
    let mut resolution = Resolution::default();
    if let Some(interpreter) = &binary.interpreter {
        resolution.interpreter_missing = path_in_root(root, interpreter).is_none();
    }
    let system_dirs = system_directories(root, binary.arch);
    let mut provided: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut wanted: Vec<(String, (String, String))> = Vec::new();
    let mut queue = VecDeque::new();
    let mut seen = BTreeSet::new();
    // A program outside the root is addressed by its host path.
    let origin = origin_of(root, program);
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    enqueue(&mut queue, &mut seen, binary, &name, origin.as_deref());
    wanted.extend(
        binary
            .needed_versions
            .iter()
            .map(|v| (name.clone(), v.clone())),
    );

    while let Some(request) = queue.pop_front() {
        if resolution.found.len() + resolution.missing.len() >= MAX_LIBRARIES {
            break;
        }
        // The vDSO is provided by the kernel, not by a file.
        if request.name.starts_with("linux-vdso") || request.name.starts_with("linux-gate") {
            continue;
        }
        let join = |dir: &String| format!("{}/{}", dir.trim_end_matches('/'), request.name);
        let located = if request.name.contains('/') {
            load_anywhere(root, &request.name, binary.arch).map(|b| (request.name.clone(), b))
        } else {
            let own = request.search.iter().map(join);
            let system = system_dirs.iter().map(join);
            own.map(|path| (load_anywhere(root, &path, binary.arch), path))
                .chain(system.map(|path| (load(root, &path, binary.arch), path)))
                .find_map(|(library, path)| Some((path, library?)))
        };
        match located {
            Some((path, library)) => {
                let origin = path.rsplit_once('/').map(|(dir, _)| dir.to_owned());
                enqueue(
                    &mut queue,
                    &mut seen,
                    &library,
                    &request.name,
                    origin.as_deref(),
                );
                let needs = library.needed_versions.into_iter();
                wanted.extend(needs.map(|version| (request.name.clone(), version)));
                provided.insert(request.name.clone(), library.defined_versions);
                resolution.found.insert(request.name, path);
            }
            None => {
                resolution.missing.insert(request.name, request.needed_by);
            }
        }
    }
    for (needed_by, (library, version)) in wanted {
        // A library without any version definitions satisfies every reference,
        // as it does for the dynamic loader. Missing ones are reported already.
        let Some(defined) = provided.get(&library) else {
            continue;
        };
        if !defined.is_empty() && !defined.contains(&version) {
            resolution
                .missing_versions
                .entry(format!("{library} {version}"))
                .or_insert(needed_by);
        }
    }
    resolution
}

struct Request {
    name: String,
    needed_by: String,
    search: Vec<String>,
}

fn enqueue(
    queue: &mut VecDeque<Request>,
    seen: &mut BTreeSet<String>,
    binary: &Binary,
    name: &str,
    origin: Option<&str>,
) {
    let search: Vec<String> = binary
        .search_paths
        .iter()
        .filter_map(|path| expand_origin(path, origin))
        .filter(|path| path.starts_with('/'))
        .collect();
    for needed in &binary.needed {
        if seen.insert(needed.clone()) {
            queue.push_back(Request {
                name: needed.clone(),
                needed_by: name.to_owned(),
                search: search.clone(),
            });
        }
    }
}

/// Expands `$ORIGIN`; paths using it are dropped when the origin is unknown.
fn expand_origin(path: &str, origin: Option<&str>) -> Option<String> {
    if !path.contains("$ORIGIN") && !path.contains("${ORIGIN}") {
        return Some(path.to_owned());
    }
    let origin = origin?;
    Some(path.replace("${ORIGIN}", origin).replace("$ORIGIN", origin))
}

fn origin_of(root: &Path, program: &Path) -> Option<String> {
    let parent = program.parent()?;
    let inside = parent.strip_prefix(root).ok();
    let path = inside.unwrap_or(parent);
    let mut text = String::new();
    for part in path.components() {
        if let Component::Normal(part) = part {
            text.push('/');
            text.push_str(part.to_str()?);
        }
    }
    Some(text)
}

/// Opens a library in the root if it is a Linux library for the architecture.
fn load(root: &Path, path: &str, arch: Arch) -> Option<Binary> {
    path_in_root(root, path).and_then(|file| usable(&file, arch))
}

/// Like [`load`], but also accepts the same path on the host, where programs
/// outside the root keep their own libraries. System directories never fall
/// through: the host's libraries there are FreeBSD's.
fn load_anywhere(root: &Path, path: &str, arch: Arch) -> Option<Binary> {
    load(root, path, arch).or_else(|| usable(Path::new(path), arch))
}

fn usable(file: &Path, arch: Arch) -> Option<Binary> {
    if !file.is_file() {
        return None;
    }
    let binary = inspect(file).ok()?;
    let foreign = matches!(binary.abi, Abi::FreeBsd { .. } | Abi::OtherBsd { .. });
    (binary.arch == arch && !foreign).then_some(binary)
}

fn system_directories(root: &Path, arch: Arch) -> Vec<String> {
    let mut dirs = Vec::new();
    read_ld_so_conf(root, "/etc/ld.so.conf", 0, &mut dirs);
    let triplet = match arch {
        Arch::Amd64 => Some("x86_64-linux-gnu"),
        Arch::I386 => Some("i386-linux-gnu"),
        Arch::Aarch64 => Some("aarch64-linux-gnu"),
        Arch::Arm => Some("arm-linux-gnueabihf"),
        Arch::Riscv64 => Some("riscv64-linux-gnu"),
        Arch::Other(_) => None,
    };
    if let Some(triplet) = triplet {
        dirs.push(format!("/lib/{triplet}"));
        dirs.push(format!("/usr/lib/{triplet}"));
    }
    let defaults: &[&str] = match arch {
        Arch::Amd64 | Arch::Aarch64 | Arch::Riscv64 => {
            &["/lib64", "/usr/lib64", "/lib", "/usr/lib"]
        }
        _ => &["/lib", "/usr/lib", "/lib32", "/usr/lib32"],
    };
    dirs.extend(defaults.iter().map(|dir| (*dir).to_owned()));
    dirs
}

fn read_ld_so_conf(root: &Path, path: &str, depth: usize, dirs: &mut Vec<String>) {
    if depth > 4 {
        return;
    }
    let Some(text) = path_in_root(root, path).and_then(|p| fs::read_to_string(p).ok()) else {
        return;
    };
    for line in text.lines().map(str::trim) {
        if let Some(pattern) = line.strip_prefix("include ") {
            let pattern = pattern.trim();
            // Only the `directory/*.conf` form distributions use is supported.
            let Some((dir, "*.conf")) = pattern.rsplit_once('/') else {
                continue;
            };
            let dir = if dir.starts_with('/') {
                dir.to_owned()
            } else {
                format!("/etc/{dir}")
            };
            let Some(entries) = path_in_root(root, &dir).and_then(|p| p.read_dir().ok()) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| name.ends_with(".conf"))
                .collect();
            names.sort();
            for name in names {
                read_ld_so_conf(root, &format!("{dir}/{name}"), depth + 1, dirs);
            }
        } else if line.starts_with('/') {
            dirs.push(line.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_expansion() {
        assert_eq!(
            expand_origin("$ORIGIN/../lib", Some("/opt/x/bin")).unwrap(),
            "/opt/x/bin/../lib"
        );
        assert_eq!(expand_origin("${ORIGIN}", Some("/a")).unwrap(), "/a");
        assert_eq!(expand_origin("$ORIGIN", None), None);
        assert_eq!(expand_origin("/usr/lib", None).unwrap(), "/usr/lib");
    }

    #[test]
    fn plain_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("usr/lib")).unwrap();
        fs::write(dir.path().join("usr/lib/x"), b"").unwrap();
        assert_eq!(
            path_in_root(dir.path(), "/usr/./lib//x").unwrap(),
            dir.path().join("usr/lib/x")
        );
        assert!(path_in_root(dir.path(), "/usr/lib/y").is_none());
        // `..` cannot leave the root.
        assert_eq!(
            path_in_root(dir.path(), "/../../usr/lib/x").unwrap(),
            dir.path().join("usr/lib/x")
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_stay_inside_the_root() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("usr/lib")).unwrap();
        fs::write(dir.path().join("usr/lib/ld.so"), b"").unwrap();
        symlink("usr/lib", dir.path().join("lib")).unwrap();
        symlink("/usr/lib/ld.so", dir.path().join("absolute")).unwrap();
        symlink("../../../../usr/lib/ld.so", dir.path().join("climbing")).unwrap();
        symlink("loop", dir.path().join("loop")).unwrap();
        let expected = dir.path().join("usr/lib/ld.so");
        for path in ["/lib/ld.so", "/absolute", "/climbing"] {
            assert_eq!(
                path_in_root(dir.path(), path),
                Some(expected.clone()),
                "{path}"
            );
        }
        assert_eq!(path_in_root(dir.path(), "/loop"), None);
    }

    #[test]
    fn ld_so_conf_includes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("etc/ld.so.conf.d")).unwrap();
        fs::write(
            dir.path().join("etc/ld.so.conf"),
            "include ld.so.conf.d/*.conf\n/opt/lib\n",
        )
        .unwrap();
        fs::write(dir.path().join("etc/ld.so.conf.d/b.conf"), "# c\n/b\n").unwrap();
        fs::write(dir.path().join("etc/ld.so.conf.d/a.conf"), "/a\n").unwrap();
        let mut dirs = Vec::new();
        read_ld_so_conf(dir.path(), "/etc/ld.so.conf", 0, &mut dirs);
        assert_eq!(dirs, ["/a", "/b", "/opt/lib"]);
    }
}
