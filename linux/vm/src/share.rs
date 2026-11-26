//! Which host directories may be shared.
//!
//! bhyve serves shares as the machine's owner, so a guest cannot make files that
//! belong to anyone else. It could still leave a set-user-ID program of the owner's
//! where another host user runs it. Only filesystems mounted `nosuid` are therefore
//! accepted.

use std::path::Path;

/// Directories that are never shared, nor anything that contains them.
const PROTECTED: &[&str] = &[
    "/bin",
    "/boot",
    "/compat",
    "/dev",
    "/etc",
    "/lib",
    "/libexec",
    "/proc",
    "/rescue",
    "/root",
    "/sbin",
    "/usr",
    "/var/backups",
    "/var/cron",
    "/var/db",
    "/var/log",
    "/var/mail",
    "/var/run",
];

fn within(path: &str, directory: &str) -> bool {
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Mount options of the filesystem holding `path`, from `mount -p`.
fn mount_options<'a>(path: &str, table: &'a str) -> Option<Vec<&'a str>> {
    table
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let point = *fields.get(1)?;
            let options = fields.get(3)?.split(',').collect();
            (point == "/" || within(path, point)).then_some((point.len(), options))
        })
        .max_by_key(|(length, _)| *length)
        .map(|(_, options)| options)
}

/// Decides from the path and the mount table alone.
pub fn check_path(path: &str, table: &str) -> Result<(), String> {
    if path == "/" {
        return Err("the whole filesystem cannot be shared".into());
    }
    if let Some(protected) = PROTECTED
        .iter()
        .find(|p| within(path, p) || within(p, path))
    {
        return Err(format!("it overlaps {protected}"));
    }
    match mount_options(path, table) {
        None => Err("its filesystem cannot be determined".into()),
        Some(options) if options.contains(&"nosuid") => Ok(()),
        Some(_) => {
            Err("its filesystem allows set-user-ID programs; use a dataset with setuid=off".into())
        }
    }
}

/// Also checks the directory itself: real, and owned by the machine's owner.
pub fn check(path: &str, owner_uid: u32, table: &str) -> Result<(), String> {
    check_path(path, table)?;
    let metadata = std::fs::symlink_metadata(Path::new(path)).map_err(|e| e.to_string())?;
    if !metadata.is_dir() {
        return Err("it is not a directory".into());
    }
    let real = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if real != Path::new(path) {
        return Err(format!(
            "it is reached through a link to {}",
            real.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != owner_uid {
            return Err("it belongs to another user".into());
        }
    }
    #[cfg(not(unix))]
    let _ = owner_uid;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "zroot/ROOT/default / zfs rw,nfsv4acls 0 0\n\
        zroot/home /home zfs rw 0 0\n\
        zroot/share /var/meta-linux/share/x zfs rw,nosuid,nfsv4acls 0 0\n";

    #[test]
    fn only_nosuid_filesystems_are_shared() {
        assert!(check_path("/var/meta-linux/share/x/sub", TABLE).is_ok());
        assert!(
            check_path("/home/alice", TABLE)
                .unwrap_err()
                .contains("set-user-ID")
        );
        // A sibling with the same beginning is on another filesystem.
        assert!(check_path("/var/meta-linux/share/xy", TABLE).is_err());
    }

    #[test]
    fn system_directories_are_never_shared() {
        for path in [
            "/",
            "/etc",
            "/etc/rc.d",
            "/usr/local",
            "/var",
            "/var/db/x",
            "/root",
            "/dev",
        ] {
            assert!(
                check_path(path, &TABLE.replace("rw", "rw,nosuid")).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn missing_tables_refuse() {
        assert!(check_path("/srv/x", "").is_err());
    }
}
