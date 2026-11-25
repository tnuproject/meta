//! Creating a userland from a distribution's signed archive.

use std::fs;
use std::path::{Path, PathBuf};

use meta_common::CommandRunner;
use meta_pkg_policy::{Bootstrap, Edition};
use sha2::{Digest, Sha256};

use crate::root::{JEXEC, RootError};

const FETCH: &str = "/usr/bin/fetch";
const GPG: &str = "/usr/local/bin/gpg";
const DEBOOTSTRAP: &str = "/usr/local/sbin/debootstrap";
const TAR: &str = "/usr/bin/tar";

/// Verified input for [`install`].
#[derive(Debug)]
pub enum Material {
    Keyring(PathBuf),
    Archive(PathBuf),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> RootError + '_ {
    move |source| RootError::Io {
        path: path.display().to_string(),
        source,
    }
}

fn text(path: &Path) -> Result<&str, RootError> {
    path.to_str()
        .ok_or_else(|| RootError::Bootstrap("path is not valid UTF-8".into()))
}

fn download(runner: &dyn CommandRunner, url: &str, path: &Path) -> Result<(), RootError> {
    if !url.starts_with("https://") {
        return Err(RootError::Bootstrap(format!("refusing {url}: not HTTPS")));
    }
    runner.stdout(FETCH, &["-q", "-T", "60", "-o", text(path)?, url])?;
    Ok(())
}

/// Downloads and verifies what the edition is built from.
pub fn fetch(
    runner: &dyn CommandRunner,
    edition: &Edition,
    work: &Path,
) -> Result<Material, RootError> {
    let result = fetch_verified(runner, edition, work);
    // gpg leaves helper processes behind, bound to its home directory.
    if let Ok(home) = text(&work.join("gnupg")) {
        let _ = runner.run(
            "/usr/local/bin/gpgconf",
            &["--homedir", home, "--kill", "all"],
        );
    }
    result
}

fn fetch_verified(
    runner: &dyn CommandRunner,
    edition: &Edition,
    work: &Path,
) -> Result<Material, RootError> {
    let home = work.join("gnupg");
    fs::create_dir(&home).map_err(io(&home))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).map_err(io(&home))?;
    }
    let home = text(&home)?.to_owned();
    match &edition.bootstrap {
        Bootstrap::Debootstrap { keys, .. } => {
            let mut keyring = Vec::new();
            for (index, key) in keys.iter().enumerate() {
                let armored = work.join(format!("key{index}.asc"));
                download(runner, &key.url, &armored)?;
                let data = fs::read(&armored).map_err(io(&armored))?;
                let digest = hex::encode(Sha256::digest(&data));
                if digest != key.sha256 {
                    return Err(RootError::Bootstrap(format!(
                        "{} has SHA-256 {digest}, expected {}",
                        key.url, key.sha256
                    )));
                }
                let binary = work.join(format!("key{index}.gpg"));
                let args = [
                    "--homedir",
                    &home,
                    "--batch",
                    "--dearmor",
                    "-o",
                    text(&binary)?,
                    text(&armored)?,
                ];
                runner.stdout(GPG, &args)?;
                keyring.extend(fs::read(&binary).map_err(io(&binary))?);
            }
            let path = work.join("keyring.gpg");
            fs::write(&path, keyring).map_err(io(&path))?;
            Ok(Material::Keyring(path))
        }
        Bootstrap::Archive {
            url,
            signature,
            signer,
            fingerprint,
        } => {
            let archive = work.join("archive");
            let detached = work.join("archive.sig");
            // The key is located by address but trusted only by fingerprint.
            runner.stdout(
                GPG,
                &[
                    "--homedir",
                    &home,
                    "--batch",
                    "--auto-key-locate",
                    "clear,wkd",
                    "--locate-external-keys",
                    signer,
                ],
            )?;
            download(runner, signature, &detached)?;
            download(runner, url, &archive)?;
            let status = runner.stdout(
                GPG,
                &[
                    "--homedir",
                    &home,
                    "--batch",
                    "--status-fd",
                    "1",
                    "--verify",
                    text(&detached)?,
                    text(&archive)?,
                ],
            )?;
            if !signed_by(&status, fingerprint) {
                return Err(RootError::Bootstrap(format!(
                    "{url} is not signed by {fingerprint}"
                )));
            }
            Ok(Material::Archive(archive))
        }
    }
}

/// Whether gpg's status output reports a valid signature by the primary key.
///
/// `VALIDSIG` ends with the fingerprint of the primary key; `GOODSIG` alone says
/// nothing about which key was meant.
fn signed_by(status: &str, fingerprint: &str) -> bool {
    status
        .lines()
        .filter_map(|line| line.strip_prefix("[GNUPG:] VALIDSIG "))
        .any(|fields| fields.split_whitespace().next_back() == Some(fingerprint))
}

pub fn install(
    runner: &dyn CommandRunner,
    edition: &Edition,
    material: &Material,
) -> Result<(), RootError> {
    match (&edition.bootstrap, material) {
        (Bootstrap::Debootstrap { suite, mirror, .. }, Material::Keyring(keyring)) => {
            let option = format!("--keyring={}", text(keyring)?);
            let args: [&str; 6] = [
                "--variant=minbase",
                "--force-check-gpg",
                &option,
                suite,
                &edition.root,
                mirror,
            ];
            let path = verifier_path(keyring.parent().unwrap_or(Path::new("/")))?;
            let mut command = vec![path.as_str(), DEBOOTSTRAP];
            command.extend(args.iter().copied());
            runner.inherit("/usr/bin/env", &command)?;
        }
        (Bootstrap::Archive { .. }, Material::Archive(archive)) => {
            let args = [
                "-x",
                "-p",
                "--numeric-owner",
                // FreeBSD has no equivalent of Linux file capabilities.
                "--no-xattrs",
                "--strip-components",
                "1",
                "-f",
                text(archive)?,
                "-C",
                &edition.root,
            ];
            runner.stdout(TAR, &args)?;
        }
        _ => return Err(RootError::Bootstrap("unexpected material".into())),
    }
    Ok(())
}

/// `PATH` with a directory providing `gpgv2`.
///
/// FreeBSD's debootstrap insists on that name, but its gnupg package installs the
/// program as `gpgv`. Without it, debootstrap refuses to check signatures.
fn verifier_path(work: &Path) -> Result<String, RootError> {
    let directory = work.join("bin");
    fs::create_dir_all(&directory).map_err(io(&directory))?;
    #[cfg(unix)]
    {
        let link = directory.join("gpgv2");
        std::os::unix::fs::symlink("/usr/local/bin/gpgv", &link).map_err(io(&link))?;
    }
    let directory = text(&directory)?;
    Ok(format!(
        "PATH={directory}:/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin"
    ))
}

/// Marks the root and keeps kernels, boot loaders and init systems out of it.
pub fn protect(edition: &Edition) -> Result<(), RootError> {
    let root = Path::new(&edition.root);
    let write = |inside: &str, contents: &str| {
        let path = root.join(inside);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        fs::write(&path, contents).map_err(io(&path))
    };
    write(
        "etc/meta-compat-release",
        &format!(
            "META_VERSION={}\nMETA_EDITION={}\n",
            meta_common::VERSION,
            edition.id
        ),
    )?;
    match &edition.bootstrap {
        Bootstrap::Debootstrap { suite, mirror, .. } => {
            write(
                "etc/apt/preferences.d/meta-compat",
                include_str!("../../debian/preferences"),
            )?;
            write(
                "etc/apt/sources.list",
                &format!("deb {mirror} {suite} main\n"),
            )?;
        }
        Bootstrap::Archive { .. } => {
            write(
                "etc/pacman.d/hooks/00-meta-compat.hook",
                include_str!("../../arch/meta-compat.hook"),
            )?;
            write(
                "etc/pacman.d/mirrorlist",
                "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n",
            )?;
            let path = root.join("etc/pacman.conf");
            let configuration = fs::read_to_string(&path).map_err(io(&path))?;
            fs::write(&path, adapt_pacman(&configuration)).map_err(io(&path))?;
        }
    }
    Ok(())
}

/// Pacman sandboxes downloads with Linux facilities Linuxulator does not have,
/// and checks free space through mount tables a jail does not show.
fn adapt_pacman(configuration: &str) -> String {
    let mut result = String::new();
    for line in configuration.lines() {
        let option = line.trim_start_matches('#').trim();
        match option {
            "DisableSandbox" | "CheckSpace" | "DownloadUser = alpm" => continue,
            _ => {}
        }
        result.push_str(line);
        result.push('\n');
        if line.trim() == "[options]" {
            result.push_str("DisableSandbox\n");
        }
    }
    result
}

fn inside(runner: &dyn CommandRunner, jail: &str, command: &[&str]) -> Result<(), RootError> {
    let mut args = vec![
        jail,
        "/usr/bin/env",
        "-i",
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin",
        "HOME=/root",
        "LC_ALL=C",
        "DEBIAN_FRONTEND=noninteractive",
    ];
    args.extend(command);
    runner.inherit(JEXEC, &args)?;
    Ok(())
}

/// First steps that need the root to be running.
pub fn finish(runner: &dyn CommandRunner, edition: &Edition, jail: &str) -> Result<(), RootError> {
    match &edition.bootstrap {
        Bootstrap::Debootstrap { .. } => {
            inside(runner, jail, &["apt-get", "-q", "update"])?;
        }
        Bootstrap::Archive { .. } => {
            inside(runner, jail, &["pacman-key", "--init"])?;
            inside(runner, jail, &["pacman-key", "--populate"])?;
            inside(runner, jail, &["pacman", "-Sy", "--noconfirm"])?;
        }
    }
    Ok(())
}

pub fn update(runner: &dyn CommandRunner, edition: &Edition, jail: &str) -> Result<(), RootError> {
    match &edition.bootstrap {
        Bootstrap::Debootstrap { .. } => {
            inside(runner, jail, &["apt-get", "-q", "update"])?;
            inside(runner, jail, &["apt-get", "-q", "-y", "dist-upgrade"])
        }
        Bootstrap::Archive { .. } => inside(
            runner,
            jail,
            &["pacman", "-Syu", "--noconfirm", "--noprogressbar"],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPR: &str = "3E80CA1A8B89F69CBA57D98A76A5EF9054449A5C";

    #[test]
    fn only_the_primary_fingerprint_counts() {
        let status = format!(
            "[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG 76A5EF9054449A5C Name\n\
             [GNUPG:] VALIDSIG {FPR} 2026-11-01 1 0 4 0 22 10 00 {FPR}\n"
        );
        assert!(signed_by(&status, FPR));
        assert!(!signed_by(&status, &FPR.replace('3', "4")));
        assert!(!signed_by("[GNUPG:] GOODSIG 76A5EF9054449A5C Name\n", FPR));
        // A user id containing the text must not be mistaken for a status line.
        assert!(!signed_by(
            &format!("[GNUPG:] GOODSIG X VALIDSIG {FPR}\n"),
            FPR
        ));
    }

    #[test]
    fn pacman_configuration_is_adapted_once() {
        let text =
            "# comment\n[options]\nCheckSpace\n#DisableSandbox\nDownloadUser = alpm\n[core]\n";
        let adapted = adapt_pacman(text);
        assert_eq!(adapted, "# comment\n[options]\nDisableSandbox\n[core]\n");
        assert_eq!(adapt_pacman(&adapted), adapted);
    }
}
