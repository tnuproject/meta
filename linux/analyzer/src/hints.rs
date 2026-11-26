//! Traces in a file of what a program might use.
//!
//! None of this shows that the program does use something: code may never run, and
//! text may be a message. Hints explain failures and break ties; they never rule a
//! runtime out.

use goblin::elf::{Elf, section_header::SHF_EXECINSTR};
use linux2bsd::elf::{Arch, Binary};
use meta_model::{Evidence, Requirement, Source};

use crate::evidence;

/// Functions of the C library that are thin wrappers of calls Linuxulator lacks.
const CALLS: &[&str] = &[
    "unshare",
    "setns",
    "pivot_root",
    "init_module",
    "finit_module",
    "delete_module",
    "bpf",
    "perf_event_open",
    "fanotify_init",
    "fanotify_mark",
    "pidfd_open",
    "pidfd_send_signal",
    "pidfd_getfd",
    "open_tree",
    "move_mount",
    "fsopen",
    "fsmount",
    "userfaultfd",
    "landlock_create_ruleset",
    "keyctl",
    "process_madvise",
];

/// Functions of other libraries that stand for a kernel feature.
const FUNCTIONS: &[(&str, &str)] = &[
    ("io_uring_queue_init", "kernel:io-uring"),
    ("io_uring_queue_init_params", "kernel:io-uring"),
    ("sd_notify", "init:systemd"),
    ("sd_bus_open_system", "init:systemd"),
];

const TEXTS: &[(&str, &str)] = &[
    ("/sys/fs/cgroup/unified", "kernel:cgroup-v1"),
    ("/sys/fs/cgroup/memory", "kernel:cgroup-v1"),
    ("/sys/fs/cgroup/cgroup.controllers", "kernel:cgroup-v2"),
    ("cgroup.subtree_control", "kernel:cgroup-v2"),
    ("/run/systemd/system", "init:systemd"),
    ("/run/systemd/private", "init:systemd"),
    ("/var/run/docker.sock", "container:docker"),
    ("/run/docker.sock", "container:docker"),
    ("/run/containerd/containerd.sock", "container:containerd"),
    ("/dev/kvm", "device:kvm"),
    ("/dev/net/tun", "device:tun"),
    ("/dev/fuse", "kernel:fuse"),
    ("/dev/dri/", "device:gpu"),
    ("/proc/self/ns/", "kernel:namespaces"),
    ("/proc/modules", "kernel:modules"),
];

/// Numbers of amd64 system calls that stand for a feature.
const NUMBERS: &[(u32, &str)] = &[
    (155, "syscall:pivot_root"),
    (175, "syscall:init_module"),
    (272, "syscall:unshare"),
    (298, "syscall:perf_event_open"),
    (308, "syscall:setns"),
    (313, "syscall:finit_module"),
    (321, "syscall:bpf"),
    (323, "syscall:userfaultfd"),
    (425, "syscall:io_uring_setup"),
    (430, "syscall:fsopen"),
    (434, "syscall:pidfd_open"),
    (435, "syscall:clone3"),
    (444, "syscall:landlock_create_ruleset"),
];

pub fn find(data: &[u8], binary: &Binary) -> Vec<(Requirement, Evidence)> {
    let mut found = Vec::new();
    if let Ok(elf) = Elf::parse(data) {
        imports(&elf, &mut found);
        // A dynamic program makes its calls through the C library. Scanning it
        // would find nothing but noise.
        if binary.arch == Arch::Amd64 && binary.needed.is_empty() {
            instructions(&elf, data, &mut found);
        }
    }
    texts(data, &mut found);
    found
}

fn hint(found: &mut Vec<(Requirement, Evidence)>, requirement: &str, detail: String) {
    if let Ok(requirement) = requirement.parse() {
        found.push((requirement, evidence(Source::Heuristic, detail)));
    }
}

fn imports(elf: &Elf<'_>, found: &mut Vec<(Requirement, Evidence)>) {
    for symbol in elf.dynsyms.iter().filter(goblin::elf::Sym::is_import) {
        let Some(name) = elf.dynstrtab.get_at(symbol.st_name) else {
            continue;
        };
        if CALLS.contains(&name) {
            hint(found, &format!("syscall:{name}"), format!("imports {name}"));
        }
        if let Some((_, requirement)) = FUNCTIONS.iter().find(|(function, _)| *function == name) {
            hint(found, requirement, format!("imports {name}"));
        }
    }
}

/// Looks for `mov eax, number; syscall` in statically linked programs.
fn instructions(elf: &Elf<'_>, data: &[u8], found: &mut Vec<(Requirement, Evidence)>) {
    let mut seen = Vec::new();
    for section in &elf.section_headers {
        if section.sh_flags & u64::from(SHF_EXECINSTR) == 0 {
            continue;
        }
        let Some(code) = section.file_range().and_then(|range| data.get(range)) else {
            continue;
        };
        for window in code.windows(7) {
            if window[0] != 0xb8 || window[5..] != [0x0f, 0x05] {
                continue;
            }
            let number = u32::from_le_bytes([window[1], window[2], window[3], window[4]]);
            if let Some((_, requirement)) = NUMBERS.iter().find(|(n, _)| *n == number)
                && !seen.contains(&number)
            {
                seen.push(number);
                let detail = format!("contains code for system call {number}");
                hint(found, requirement, detail);
            }
        }
    }
}

fn texts(data: &[u8], found: &mut Vec<(Requirement, Evidence)>) {
    // Every text starts with one of a few bytes, so most of the file is skipped.
    let mut seen = [false; TEXTS.len()];
    for (position, byte) in data.iter().enumerate() {
        if *byte != b'/' && *byte != b'c' {
            continue;
        }
        for (index, (text, requirement)) in TEXTS.iter().enumerate() {
            if !seen[index] && data[position..].starts_with(text.as_bytes()) {
                seen[index] = true;
                hint(found, requirement, format!("contains the text {text}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::tests::linux;

    #[test]
    fn texts_are_only_hints() {
        let found = find(b"xx/run/systemd/system\0/dev/kvm\0/dev/kvm", &linux(&[]));
        let names: Vec<String> = found.iter().map(|(r, _)| r.to_string()).collect();
        assert_eq!(names, ["init:systemd", "device:kvm"]);
        assert!(found.iter().all(|(_, e)| e.source == Source::Heuristic));
    }

    #[test]
    fn tables_are_valid() {
        for (_, requirement) in TEXTS.iter().chain(FUNCTIONS) {
            assert!(requirement.parse::<Requirement>().is_ok(), "{requirement}");
        }
        for (_, requirement) in NUMBERS {
            assert!(requirement.parse::<Requirement>().is_ok(), "{requirement}");
        }
        // Text search only looks at positions with these bytes.
        assert!(TEXTS.iter().all(|(text, _)| text.starts_with(['/', 'c'])));
    }

    #[test]
    fn truncated_files_are_harmless() {
        for length in 0..64 {
            let data = vec![0x7f; length];
            find(&data, &linux(&[]));
        }
    }
}
