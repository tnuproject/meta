//! Guest images, pinned by digest.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use meta_common::CommandRunner;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

use crate::config::{VmError, valid_name};

const FETCH: &str = "/usr/bin/fetch";
pub const QEMU_IMG: &str = "/usr/local/bin/qemu-img";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub id: String,
    pub name: String,
    pub distribution: String,
    /// A qcow2 disk image.
    pub url: String,
    /// `sha256:` or `sha512:` and lowercase hexadecimal.
    pub digest: String,
    /// How the digest was obtained.
    pub provenance: String,
    /// Control group versions the distribution's init system can use.
    pub cgroups: Vec<crate::config::Cgroups>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Images {
    schema: u32,
    image: Vec<Image>,
}

impl Images {
    pub fn builtin() -> Self {
        Self::parse(include_str!("../../images.toml"))
            .expect("the embedded list is validated by the test suite")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let images: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        if images.schema != 1 {
            return Err(format!("unsupported schema {}", images.schema));
        }
        for image in &images.image {
            if !valid_name(&image.id) {
                return Err(format!("invalid identifier {:?}", image.id));
            }
            if !image.url.starts_with("https://") {
                return Err(format!("{}: not HTTPS", image.id));
            }
            if parse_digest(&image.digest).is_none() {
                return Err(format!("{}: invalid digest", image.id));
            }
        }
        Ok(images)
    }

    pub fn all(&self) -> &[Image] {
        &self.image
    }

    pub fn get(&self, id: &str) -> Option<&Image> {
        self.image.iter().find(|image| image.id == id)
    }
}

enum Expected {
    Sha256(String),
    Sha512(String),
}

fn parse_digest(text: &str) -> Option<Expected> {
    let (kind, value) = text.split_once(':')?;
    let lower = value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    match (kind, value.len(), lower) {
        ("sha256", 64, true) => Some(Expected::Sha256(value.to_owned())),
        ("sha512", 128, true) => Some(Expected::Sha512(value.to_owned())),
        _ => None,
    }
}

/// Checks a file against a digest from the list.
pub fn verify(path: &Path, digest: &str) -> Result<bool, std::io::Error> {
    fn hash<D: Digest>(path: &Path) -> Result<String, std::io::Error> {
        let mut file = File::open(path)?;
        let mut hasher = D::new();
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                return Ok(hex::encode(hasher.finalize()));
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(match parse_digest(digest) {
        Some(Expected::Sha256(expected)) => hash::<Sha256>(path)? == expected,
        Some(Expected::Sha512(expected)) => hash::<Sha512>(path)? == expected,
        None => false,
    })
}

impl Image {
    /// The raw disk image in `directory`, downloaded and verified if necessary.
    ///
    /// The raw file exists only after its source has been verified, and is moved
    /// into place whole.
    pub fn prepare(
        &self,
        directory: &Path,
        runner: &dyn CommandRunner,
    ) -> Result<PathBuf, VmError> {
        let raw = directory.join(format!("{}.raw", self.id));
        if raw.is_file() {
            return Ok(raw);
        }
        let error = |reason: String| VmError::Image {
            image: self.id.clone(),
            reason,
        };
        fs::create_dir_all(directory).map_err(|source| VmError::Io {
            path: directory.display().to_string(),
            source,
        })?;
        let download = directory.join(format!("{}.download", self.id));
        let text = |path: &Path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| error("path is not UTF-8".into()))
        };
        if !download.is_file() || !verify(&download, &self.digest).unwrap_or(false) {
            let target = text(&download)?;
            runner.inherit(FETCH, &["-q", "-T", "60", "-o", &target, &self.url])?;
        }
        if !verify(&download, &self.digest).map_err(|e| error(e.to_string()))? {
            let _ = fs::remove_file(&download);
            return Err(error("the download does not match its digest".into()));
        }
        let partial = directory.join(format!("{}.raw.new", self.id));
        let (source, target) = (text(&download)?, text(&partial)?);
        if !Path::new(QEMU_IMG).exists() {
            return Err(error("converting it needs: pkg install qemu-tools".into()));
        }
        runner.stdout(
            QEMU_IMG,
            &["convert", "-f", "qcow2", "-O", "raw", &source, &target],
        )?;
        fs::rename(&partial, &raw).map_err(|e| error(e.to_string()))?;
        let _ = fs::remove_file(&download);
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_is_valid() {
        let images = Images::builtin();
        assert!(images.get("debian-12").is_some() && images.get("arch").is_some());
        assert!(images.get("nope").is_none());
    }

    #[test]
    fn entries_need_https_and_a_digest() {
        let entry = |url: &str, digest: &str| {
            format!(
                "schema = 1\n[[image]]\nid = \"x\"\nname = \"X\"\ndistribution = \"x\"\nurl = \"{url}\"\n\
                 digest = \"{digest}\"\nprovenance = \"p\"\ncgroups = [\"v2\"]\n"
            )
        };
        let good = format!("sha256:{}", "0".repeat(64));
        assert!(Images::parse(&entry("https://x", &good)).is_ok());
        assert!(Images::parse(&entry("http://x", &good)).is_err());
        for digest in [
            "",
            "md5:00",
            "sha256:zz",
            &format!("sha256:{}", "A".repeat(64)),
        ] {
            assert!(
                Images::parse(&entry("https://x", digest)).is_err(),
                "{digest}"
            );
        }
    }

    #[test]
    fn digests_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        fs::write(&path, b"abc").unwrap();
        let abc = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&path, abc).unwrap());
        fs::write(&path, b"abd").unwrap();
        assert!(!verify(&path, abc).unwrap());
    }

    #[test]
    fn a_wrong_download_is_deleted_and_not_used() {
        use meta_common::{CommandError, CommandOutput};
        struct Evil;
        impl CommandRunner for Evil {
            fn run(&self, _: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
                fs::write(args[4], b"malicious").unwrap();
                Ok(CommandOutput {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let image = Images::builtin().get("debian-12").unwrap().clone();
        assert!(matches!(
            image.prepare(dir.path(), &Evil),
            Err(VmError::Image { .. })
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
