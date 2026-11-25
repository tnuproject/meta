//! Release-engineering tool: generates and signs META update metadata.
//!
//! Runs on a release engineer's machine or a protected CI job, never on client
//! systems.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::{Parser, Subcommand, ValueEnum};
use ed25519_dalek::{Signer, SigningKey};
use meta_common::{MetaVersion, PROJECT};
use metaupd_metadata::manifest::{MANIFEST_KIND, MANIFEST_SCHEMA};
use metaupd_metadata::root::{ROOT_KIND, ROOT_SCHEMA};
use metaupd_metadata::{
    Apply, Artifact, AssetName, Category, Channel, Compatibility, Edition, MANIFEST_ASSET,
    MANIFEST_SIGNATURE_ASSET, Manifest, Migration, PublicKey, Role, RoleSpec, RootMetadata,
    Timestamp,
};
use metaupd_verify::{
    HashingReader, SignatureEntry, SignatureFile, TrustedRoot, key_id, signing_message, verify_file,
};
use serde::Deserialize;
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const SECRET_PREFIX: &str = "meta-ed25519-secret-v1:";

#[derive(Parser)]
#[command(
    name = "metaupd-release",
    about = "Generate and sign META release metadata"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a signing key outside any Git working tree
    Keygen {
        /// File to create; must not exist
        #[arg(long)]
        out: PathBuf,
    },
    /// Build root metadata from a configuration file
    Root {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build a release manifest for the artifacts in a directory
    Manifest {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        artifacts: PathBuf,
    },
    /// Write SHA256SUMS for a release directory
    Checksums {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Add a signature to a document
    Sign {
        #[arg(long)]
        role: RoleArg,
        /// Document to sign
        #[arg(long)]
        file: PathBuf,
        /// Signature file to create or extend
        #[arg(long)]
        signatures: PathBuf,
        /// File holding the secret key
        #[arg(long, required_unless_present = "key_env", conflicts_with = "key_env")]
        key_file: Option<PathBuf>,
        /// Environment variable holding the secret key
        #[arg(long)]
        key_env: Option<String>,
    },
    /// Verify a release directory as a client would
    Verify {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        root_signatures: PathBuf,
        #[arg(long)]
        dir: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum RoleArg {
    Root,
    Release,
}

impl From<RoleArg> for Role {
    fn from(value: RoleArg) -> Self {
        match value {
            RoleArg::Root => Role::Root,
            RoleArg::Release => Role::Release,
        }
    }
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Keygen { out } => keygen(&out),
        Command::Root { config, out } => root(&config, &out),
        Command::Manifest { config, artifacts } => manifest(&config, &artifacts),
        Command::Checksums { dir } => checksums(&dir),
        Command::Sign {
            role,
            file,
            signatures,
            key_file,
            key_env,
        } => load_key(key_file.as_deref(), key_env.as_deref())
            .and_then(|key| sign(role.into(), &file, &signatures, &key)),
        Command::Verify {
            root,
            root_signatures,
            dir,
        } => verify(&root, &root_signatures, &dir),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("metaupd-release: {error}");
            ExitCode::FAILURE
        }
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn write(path: &Path, data: &[u8]) -> Result<()> {
    fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn pretty_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    Ok(data)
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = String::from_utf8(read(path)?)?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// True when `path` would live inside a Git working tree.
fn inside_git(path: &Path) -> Result<bool> {
    let absolute = std::path::absolute(path)?;
    Ok(absolute
        .ancestors()
        .skip(1)
        .any(|dir| dir.join(".git").exists()))
}

fn keygen(out: &Path) -> Result<()> {
    if inside_git(out)? {
        return Err("refusing to create a secret key inside a Git working tree".into());
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::fill(seed.as_mut_slice()).map_err(|e| format!("no randomness available: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    let text = Zeroizing::new(format!(
        "{SECRET_PREFIX}{}\n",
        BASE64.encode(seed.as_slice())
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(out)
        .map_err(|e| format!("{}: {e}", out.display()))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    let public = PublicKey(key.verifying_key().to_bytes());
    println!("public key  {}", String::from(public));
    println!("key id      {}", key_id(&public));
    eprintln!(
        "The secret key in {} is not encrypted. Keep it offline.",
        out.display()
    );
    Ok(())
}

fn load_key(file: Option<&Path>, env: Option<&str>) -> Result<SigningKey> {
    let text = Zeroizing::new(match (file, env) {
        (Some(path), _) => String::from_utf8(read(path)?).map_err(|_| "key file is not text")?,
        (None, Some(name)) => {
            std::env::var(name).map_err(|_| format!("environment variable {name} is not set"))?
        }
        (None, None) => return Err("no key given".into()),
    });
    let encoded = text
        .trim()
        .strip_prefix(SECRET_PREFIX)
        .ok_or("not a META secret key")?;
    let seed = Zeroizing::new(BASE64.decode(encoded).map_err(|_| "malformed secret key")?);
    let seed: Zeroizing<[u8; 32]> = Zeroizing::new(
        seed.as_slice()
            .try_into()
            .map_err(|_| "malformed secret key")?,
    );
    Ok(SigningKey::from_bytes(&seed))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RootConfig {
    version: u32,
    expires: Timestamp,
    roles: BTreeMap<Role, RoleConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleConfig {
    threshold: u32,
    /// Public keys in lowercase hexadecimal.
    keys: Vec<PublicKey>,
}

fn root(config: &Path, out: &Path) -> Result<()> {
    let config: RootConfig = read_toml(config)?;
    let mut keys = BTreeMap::new();
    let mut roles = BTreeMap::new();
    for (role, spec) in config.roles {
        let mut keyids = BTreeSet::new();
        for key in spec.keys {
            keys.insert(key_id(&key), key);
            keyids.insert(key_id(&key));
        }
        roles.insert(
            role,
            RoleSpec {
                threshold: spec.threshold,
                keyids,
            },
        );
    }
    let metadata = RootMetadata {
        schema: ROOT_SCHEMA,
        kind: ROOT_KIND.to_owned(),
        project: PROJECT.to_owned(),
        version: config.version,
        created: Timestamp::now(),
        expires: config.expires,
        keys,
        roles,
    };
    metadata.validate()?;
    write(out, &pretty_json(&metadata)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseConfig {
    version: MetaVersion,
    codename: String,
    channel: Channel,
    sequence: u64,
    valid_days: i64,
    changelog: AssetName,
    compatibility: Compatibility,
    #[serde(default, rename = "artifact")]
    artifacts: Vec<ArtifactConfig>,
    #[serde(default, rename = "migration")]
    migrations: Vec<Migration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactConfig {
    name: AssetName,
    category: Category,
    apply: Apply,
    #[serde(default)]
    requires_reboot: bool,
    #[serde(default)]
    editions: BTreeSet<Edition>,
}

/// Files in a release directory that are not artifacts.
fn is_metadata(name: &str) -> bool {
    name.starts_with("meta-manifest.") || name.starts_with("meta-root-") || name == "SHA256SUMS"
}

fn release_files(dir: &Path) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "file name is not UTF-8")?;
        if !entry.file_type()?.is_file() {
            return Err(format!("{name} is not a regular file").into());
        }
        names.insert(name);
    }
    Ok(names)
}

fn manifest(config: &Path, dir: &Path) -> Result<()> {
    let config: ReleaseConfig = read_toml(config)?;
    let mut unlisted: BTreeSet<String> = release_files(dir)?
        .into_iter()
        .filter(|name| !is_metadata(name))
        .collect();
    let mut artifacts = Vec::new();
    for item in config.artifacts {
        let path = dir.join(item.name.as_str());
        let file = fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (sha256, size) = HashingReader::new(file).finish()?;
        unlisted.remove(item.name.as_str());
        artifacts.push(Artifact {
            name: item.name,
            category: item.category,
            size,
            sha256,
            apply: item.apply,
            requires_reboot: item.requires_reboot,
            editions: item.editions,
        });
    }
    if !unlisted.is_empty() {
        let names: Vec<_> = unlisted.into_iter().collect();
        return Err(format!(
            "files not described by the configuration: {}",
            names.join(", ")
        )
        .into());
    }
    let created = Timestamp::now();
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA,
        kind: MANIFEST_KIND.to_owned(),
        project: PROJECT.to_owned(),
        version: config.version,
        codename: config.codename,
        channel: config.channel,
        sequence: config.sequence,
        created,
        expires: created.plus_days(config.valid_days),
        compatibility: config.compatibility,
        artifacts,
        changelog: config.changelog,
        migrations: config.migrations,
    };
    manifest.validate()?;
    write(&dir.join(MANIFEST_ASSET), &pretty_json(&manifest)?)
}

fn checksums(dir: &Path) -> Result<()> {
    let mut text = String::new();
    for name in release_files(dir)?
        .into_iter()
        .filter(|name| name != "SHA256SUMS")
    {
        let (digest, _) = HashingReader::new(fs::File::open(dir.join(&name))?).finish()?;
        text.push_str(&format!("{digest}  {name}\n"));
    }
    write(&dir.join("SHA256SUMS"), text.as_bytes())
}

fn sign(role: Role, file: &Path, signatures: &Path, key: &SigningKey) -> Result<()> {
    let payload = read(file)?;
    // Refuses to sign anything a client would reject.
    match role {
        Role::Root => RootMetadata::parse(&payload).map(drop)?,
        Role::Release => Manifest::parse(&payload).map(drop)?,
    }
    let mut document = if signatures.exists() {
        SignatureFile::parse(&read(signatures)?)?
    } else {
        SignatureFile::new(role)
    };
    if document.role != role {
        return Err(format!(
            "{} holds {} signatures",
            signatures.display(),
            document.role
        )
        .into());
    }
    let keyid = key_id(&PublicKey(key.verifying_key().to_bytes()));
    let signature = key.sign(&signing_message(role, &payload));
    document.signatures.retain(|entry| entry.keyid != keyid);
    document.signatures.push(SignatureEntry {
        keyid,
        sig: BASE64.encode(signature.to_bytes()),
    });
    write(signatures, &pretty_json(&document)?)
}

fn verify(root: &Path, root_signatures: &Path, dir: &Path) -> Result<()> {
    let root = TrustedRoot::from_anchor(
        &read(root)?,
        &SignatureFile::parse(&read(root_signatures)?)?,
    )?;
    let signatures = SignatureFile::parse(&read(&dir.join(MANIFEST_SIGNATURE_ASSET))?)?;
    let verified = root.verify_manifest(
        &read(&dir.join(MANIFEST_ASSET))?,
        &signatures,
        Timestamp::now(),
    )?;
    let manifest = verified.manifest();
    for artifact in &manifest.artifacts {
        verify_file(&dir.join(artifact.name.as_str()), artifact)?;
    }
    let known: BTreeSet<&str> = manifest.artifacts.iter().map(|a| a.name.as_str()).collect();
    for name in release_files(dir)? {
        if !is_metadata(&name) && !known.contains(name.as_str()) {
            return Err(format!("{name} is not covered by the manifest").into());
        }
    }
    println!(
        "META {} ({}, sequence {}): signatures and {} artifacts verified",
        manifest.version,
        manifest.channel,
        manifest.sequence,
        manifest.artifacts.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_refuses_git_working_trees() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let nested = dir.path().join("a");
        fs::create_dir(&nested).unwrap();
        assert!(keygen(&nested.join("k")).is_err());
        assert!(!nested.join("k").exists());
    }

    #[test]
    fn keygen_round_trips_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k");
        keygen(&path).unwrap();
        load_key(Some(&path), None).unwrap();
        assert!(keygen(&path).is_err());
    }

    #[test]
    fn malformed_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k");
        for text in [
            "",
            "abc",
            "meta-ed25519-secret-v1:AAAA",
            "meta-ed25519-secret-v1:***",
        ] {
            fs::write(&path, text).unwrap();
            assert!(load_key(Some(&path), None).is_err(), "{text:?}");
        }
    }
}
