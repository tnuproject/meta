//! Builds signed release directories with throwaway keys.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signer, SigningKey};
use meta_common::command::FakeRunner;
use metaupd::Engine;
use metaupd::store::Paths;
use metaupd_github::ReleaseSource;
use metaupd_metadata::{
    MANIFEST_ASSET, MANIFEST_SIGNATURE_ASSET, PublicKey, Role, RoleSpec, RootMetadata, Timestamp,
};
use metaupd_verify::{Host, SignatureEntry, SignatureFile, key_id, sha256, signing_message};
use serde_json::{Value, json};

pub const NOW: &str = "2026-06-01T00:00:00Z";
pub const ARTIFACT: &[u8] = b"pretend package contents";

pub fn now() -> Timestamp {
    NOW.parse().unwrap()
}

/// Deterministic test key. Never used for anything but these tests.
pub fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub fn public(key: &SigningKey) -> PublicKey {
    PublicKey(key.verifying_key().to_bytes())
}

pub fn sign(role: Role, payload: &[u8], keys: &[&SigningKey]) -> Vec<u8> {
    let mut file = SignatureFile::new(role);
    for key in keys {
        let sig = key.sign(&signing_message(role, payload));
        file.signatures.push(SignatureEntry {
            keyid: key_id(&public(key)),
            sig: BASE64.encode(sig.to_bytes()),
        });
    }
    serde_json::to_vec(&file).unwrap()
}

pub fn root_json(
    version: u32,
    root: (&[&SigningKey], u32),
    release: (&[&SigningKey], u32),
) -> Vec<u8> {
    let spec = |(keys, threshold): (&[&SigningKey], u32)| RoleSpec {
        threshold,
        keyids: keys
            .iter()
            .map(|k| key_id(&public(k)))
            .collect::<BTreeSet<_>>(),
    };
    let metadata = RootMetadata {
        schema: 1,
        kind: "meta-root".into(),
        project: "META".into(),
        version,
        created: "2026-01-01T00:00:00Z".parse().unwrap(),
        expires: "2027-01-01T00:00:00Z".parse().unwrap(),
        keys: root
            .0
            .iter()
            .chain(release.0)
            .map(|k| (key_id(&public(k)), public(k)))
            .collect(),
        roles: BTreeMap::from([(Role::Root, spec(root)), (Role::Release, spec(release))]),
    };
    serde_json::to_vec(&metadata).unwrap()
}

pub struct World {
    pub dir: tempfile::TempDir,
    pub runner: FakeRunner,
    pub host: Host,
}

impl World {
    /// Root key 1, release key 2, both with threshold 1.
    pub fn new() -> Self {
        let world = Self::without_anchor();
        let root = root_json(1, (&[&key(1)], 1), (&[&key(2)], 1));
        world.install_anchor(&root, &sign(Role::Root, &root, &[&key(1)]));
        world
    }

    pub fn without_anchor() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("trust")).unwrap();
        fs::create_dir(dir.path().join("releases")).unwrap();
        Self {
            dir,
            runner: FakeRunner::new(),
            host: Host {
                arch: "amd64".into(),
                osreldate: Some(1_501_000),
                meta_version: Some("26.0".parse().unwrap()),
                metaupd_version: metaupd::metaupd_version(),
            },
        }
    }

    pub fn install_anchor(&self, root: &[u8], signatures: &[u8]) {
        fs::write(self.dir.path().join("trust/root.json"), root).unwrap();
        fs::write(self.dir.path().join("trust/root.sig.json"), signatures).unwrap();
    }

    pub fn paths(&self) -> Paths {
        Paths {
            state_dir: self.dir.path().join("state"),
            trust_dir: self.dir.path().join("trust"),
        }
    }

    pub fn releases(&self) -> PathBuf {
        self.dir.path().join("releases")
    }

    pub fn engine<'a>(&'a self, source: &'a dyn ReleaseSource) -> Engine<'a> {
        Engine {
            paths: self.paths(),
            source,
            runner: &self.runner,
            now: now(),
            host: self.host.clone(),
            edition: None,
        }
    }

    /// Writes release `version` signed by `keys`, after applying `change`.
    pub fn publish(
        &self,
        version: &str,
        sequence: u64,
        keys: &[&SigningKey],
        change: impl FnOnce(&mut Value),
    ) -> PathBuf {
        let directory = self.releases().join(format!("v{version}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("meta-base.pkg"), ARTIFACT).unwrap();
        fs::write(directory.join("CHANGELOG.md"), b"changes\n").unwrap();
        let mut manifest = manifest(version, sequence);
        change(&mut manifest);
        let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        fs::write(directory.join(MANIFEST_ASSET), &bytes).unwrap();
        fs::write(
            directory.join(MANIFEST_SIGNATURE_ASSET),
            sign(Role::Release, &bytes, keys),
        )
        .unwrap();
        directory
    }
}

pub fn manifest(version: &str, sequence: u64) -> Value {
    json!({
        "schema": 1,
        "kind": "meta-release-manifest",
        "project": "META",
        "version": version,
        "codename": "uglydemon",
        "channel": "stable",
        "sequence": sequence,
        "created": "2026-05-01T00:00:00Z",
        "expires": "2026-08-01T00:00:00Z",
        "compatibility": {
            "arch": ["amd64"],
            "min_meta_version": "26.0",
            "osreldate": {"min": 1_501_000, "max": 1_501_999},
            "min_metaupd_version": "26.0-alpha0"
        },
        "artifacts": [
            {
                "name": "meta-base.pkg",
                "category": "meta-system",
                "size": ARTIFACT.len(),
                "sha256": sha256(ARTIFACT),
                "apply": "pkg-in-boot-environment",
                "requires_reboot": true
            },
            {
                "name": "CHANGELOG.md",
                "category": "documentation",
                "size": 8,
                "sha256": sha256(b"changes\n"),
                "apply": "none"
            }
        ],
        "changelog": "CHANGELOG.md"
    })
}

pub fn staged_file(paths: &Paths, version: &str, name: &str) -> PathBuf {
    paths.staging().join(version).join(name)
}
