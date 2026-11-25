#![allow(clippy::unwrap_used)]

mod common;

use std::cell::Cell;
use std::fs;
use std::path::Path;

use common::{ARTIFACT, World, key, root_json, sign, staged_file};
use metaupd::history;
use metaupd::trust_store::TrustStoreError;
use metaupd::{CheckReport, EngineError};
use metaupd_github::{DirectorySource, Release, ReleaseSource, SourceError};
use metaupd_metadata::{Channel, MANIFEST_ASSET, MANIFEST_SIGNATURE_ASSET, Role};

fn check(world: &World) -> CheckReport {
    let source = DirectorySource::new(world.releases());
    world.engine(&source).check().unwrap()
}

fn offered(report: &CheckReport) -> Option<String> {
    report
        .offer
        .as_ref()
        .map(|offer| offer.manifest.manifest().version.to_string())
}

/// Asserts that nothing is offered and the only rejection mentions `reason`.
fn assert_rejected(world: &World, reason: &str) {
    let report = check(world);
    assert_eq!(offered(&report), None);
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert!(
        report.rejected[0].1.contains(reason),
        "{:?}",
        report.rejected
    );
}

#[test]
fn verified_release_is_offered_downloaded_and_planned() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let source = DirectorySource::new(world.releases());
    let engine = world.engine(&source);

    let report = engine.check().unwrap();
    assert_eq!(offered(&report).as_deref(), Some("26.0.1"));
    assert!(report.rejected.is_empty() && report.warnings.is_empty());

    engine.download().unwrap();
    let staged = engine.staged("26.0.1").unwrap();
    assert!(
        staged.incompatibilities.is_empty(),
        "{:?}",
        staged.incompatibilities
    );
    let plan = engine.plan(&staged);
    assert!(plan.requires_reboot);
    assert!(plan.not_rolled_back.is_empty());
    assert_eq!(engine.staged_versions().len(), 1);

    let records = history::load(&engine.paths).unwrap().records;
    assert_eq!(records.len(), 1);
    assert!(records[0].success);
}

#[test]
fn missing_anchor_fails_closed() {
    let world = World::without_anchor();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let source = DirectorySource::new(world.releases());
    let error = world.engine(&source).check().unwrap_err();
    assert!(
        matches!(error, EngineError::TrustStore(TrustStoreError::NoAnchor(_))),
        "{error}"
    );
}

#[test]
fn edited_anchor_is_refused() {
    let world = World::new();
    let path = world.paths().trust_dir.join("root.json");
    let text = fs::read_to_string(&path).unwrap().replace("2027", "2099");
    fs::write(&path, text).unwrap();
    let source = DirectorySource::new(world.releases());
    assert!(world.engine(&source).check().is_err());
}

#[test]
fn untrusted_key_is_rejected() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(99)], |_| {});
    assert_rejected(&world, "threshold not met");
}

#[test]
fn root_key_cannot_sign_releases() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(1)], |_| {});
    assert_rejected(&world, "threshold not met");
}

#[test]
fn signature_for_another_role_is_rejected() {
    let world = World::new();
    let directory = world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let bytes = fs::read(directory.join(MANIFEST_ASSET)).unwrap();
    // Correct key, wrong domain, relabelled to look like a release signature.
    let signature = String::from_utf8(sign(Role::Root, &bytes, &[&key(2)])).unwrap();
    let relabelled = signature.replace("\"role\":\"root\"", "\"role\":\"release\"");
    fs::write(directory.join(MANIFEST_SIGNATURE_ASSET), relabelled).unwrap();
    assert_rejected(&world, "threshold not met");
    fs::write(directory.join(MANIFEST_SIGNATURE_ASSET), signature).unwrap();
    assert_rejected(&world, "role root, expected release");
}

#[test]
fn tampered_manifest_is_rejected() {
    let world = World::new();
    let directory = world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let path = directory.join(MANIFEST_ASSET);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("26.0.1", "26.0.2");
    fs::write(&path, text).unwrap();
    assert_rejected(&world, "threshold not met");
}

#[test]
fn missing_signature_is_rejected() {
    let world = World::new();
    let directory = world.publish("26.0.1", 2, &[&key(2)], |_| {});
    fs::remove_file(directory.join(MANIFEST_SIGNATURE_ASSET)).unwrap();
    assert_rejected(&world, "no asset meta-manifest.sig.json");
}

#[test]
fn one_key_signing_twice_does_not_meet_a_threshold_of_two() {
    let world = World::without_anchor();
    let root = root_json(1, (&[&key(1)], 1), (&[&key(2), &key(3)], 2));
    world.install_anchor(&root, &sign(Role::Root, &root, &[&key(1)]));
    world.publish("26.0.1", 2, &[&key(2), &key(2)], |_| {});
    assert_rejected(&world, "1 of 2");
    world.publish("26.0.1", 2, &[&key(2), &key(3)], |_| {});
    assert_eq!(offered(&check(&world)).as_deref(), Some("26.0.1"));
}

#[test]
fn expired_and_future_manifests_are_rejected() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |m| {
        m["expires"] = "2026-05-02T00:00:00Z".into()
    });
    assert_rejected(&world, "expired");
    world.publish("26.0.1", 2, &[&key(2)], |m| {
        m["created"] = "2026-07-01T00:00:00Z".into()
    });
    assert_rejected(&world, "in the future");
}

#[test]
fn expired_root_rejects_everything() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |m| {
        m["expires"] = "2028-01-01T00:00:00Z".into()
    });
    let source = DirectorySource::new(world.releases());
    let mut engine = world.engine(&source);
    engine.now = "2027-06-01T00:00:00Z".parse().unwrap();
    let report = engine.check().unwrap();
    assert!(report.offer.is_none());
    assert!(
        report.rejected[0]
            .1
            .contains("root metadata version 1 expired")
    );
}

#[test]
fn older_or_equal_versions_are_not_offered() {
    let world = World::new();
    world.publish("26.0", 1, &[&key(2)], |_| {});
    assert_eq!(offered(&check(&world)), None);
}

#[test]
fn replayed_older_sequence_is_refused_and_reported() {
    let world = World::new();
    let newer = world.publish("26.0.2", 3, &[&key(2)], |_| {});
    assert_eq!(offered(&check(&world)).as_deref(), Some("26.0.2"));
    // The source stops offering 26.0.2 and replays a validly signed older one.
    fs::remove_dir_all(newer).unwrap();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let report = check(&world);
    assert_eq!(offered(&report), None);
    assert!(
        report.warnings.iter().any(|w| w.contains("withheld")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn equivocation_is_rejected() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    assert!(check(&world).offer.is_some());
    world.publish("26.0.1", 2, &[&key(2)], |m| {
        m["expires"] = "2026-09-01T00:00:00Z".into()
    });
    assert_rejected(&world, "seen before with different contents");
}

#[test]
fn two_manifests_with_one_sequence_are_both_rejected() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    world.publish("26.0.2", 2, &[&key(2)], |_| {});
    let report = check(&world);
    assert_eq!(offered(&report), None);
    assert_eq!(report.rejected.len(), 2);
}

#[test]
fn channels_filter_offers() {
    let world = World::new();
    world.publish("26.1-beta1", 1, &[&key(2)], |m| {
        m["channel"] = "testing".into()
    });
    assert_eq!(offered(&check(&world)), None);
    let source = DirectorySource::new(world.releases());
    world.engine(&source).set_channel(Channel::Testing).unwrap();
    assert_eq!(offered(&check(&world)).as_deref(), Some("26.1-beta1"));
}

#[test]
fn corrupt_artifact_is_not_staged() {
    let world = World::new();
    let directory = world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let mut corrupt = ARTIFACT.to_vec();
    corrupt[0] ^= 1;
    fs::write(directory.join("meta-base.pkg"), corrupt).unwrap();
    let source = DirectorySource::new(world.releases());
    let engine = world.engine(&source);
    assert!(matches!(engine.download(), Err(EngineError::Hash(_))));
    let path = staged_file(&engine.paths, "26.0.1", "meta-base.pkg");
    assert!(!path.exists());
    assert!(!staged_file(&engine.paths, "26.0.1", "meta-base.pkg.partial").exists());
    assert!(matches!(
        engine.staged("26.0.1"),
        Err(EngineError::NotStaged(_))
    ));
    assert!(!history::load(&engine.paths).unwrap().records[0].success);
}

#[test]
fn oversized_artifact_is_refused() {
    let world = World::new();
    let directory = world.publish("26.0.1", 2, &[&key(2)], |_| {});
    fs::write(directory.join("meta-base.pkg"), vec![0u8; 1 << 20]).unwrap();
    let source = DirectorySource::new(world.releases());
    assert!(matches!(
        world.engine(&source).download(),
        Err(EngineError::Source(SourceError::TooLarge { .. }))
    ));
}

#[test]
fn tampering_after_download_is_detected() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let source = DirectorySource::new(world.releases());
    let engine = world.engine(&source);
    engine.download().unwrap();
    let path = staged_file(&engine.paths, "26.0.1", "meta-base.pkg");
    let mut data = fs::read(&path).unwrap();
    data[0] ^= 1;
    fs::write(&path, data).unwrap();
    assert!(matches!(engine.staged("26.0.1"), Err(EngineError::Hash(_))));
    // Downloading again repairs it.
    engine.download().unwrap();
    engine.staged("26.0.1").unwrap();
}

/// Writes half of each artifact and fails, once; then appends the rest.
struct Interrupted {
    inner: DirectorySource,
    failures: Cell<u32>,
}

impl ReleaseSource for Interrupted {
    fn describe(&self) -> String {
        "interrupted".into()
    }

    fn releases(&self) -> Result<Vec<Release>, SourceError> {
        self.inner.releases()
    }

    fn fetch(&self, release: &Release, asset: &str, limit: u64) -> Result<Vec<u8>, SourceError> {
        self.inner.fetch(release, asset, limit)
    }

    fn download(
        &self,
        release: &Release,
        asset: &str,
        destination: &Path,
        limit: u64,
    ) -> Result<(), SourceError> {
        let data = self.inner.fetch(release, asset, limit)?;
        let have = fs::read(destination).unwrap_or_default();
        if self.failures.get() > 0 {
            self.failures.set(self.failures.get() - 1);
            fs::write(destination, &data[..data.len() / 2])?;
            return Err(SourceError::Transport("connection reset".into()));
        }
        fs::write(destination, [have.as_slice(), &data[have.len()..]].concat())?;
        Ok(())
    }
}

#[test]
fn interrupted_download_resumes() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let source = Interrupted {
        inner: DirectorySource::new(world.releases()),
        failures: Cell::new(1),
    };
    let engine = world.engine(&source);
    assert!(engine.download().is_err());
    assert!(staged_file(&engine.paths, "26.0.1", "meta-base.pkg.partial").exists());
    assert!(engine.staged("26.0.1").is_err());
    engine.download().unwrap();
    engine.staged("26.0.1").unwrap();
}

#[test]
fn corrupt_partial_is_discarded() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let source = Interrupted {
        inner: DirectorySource::new(world.releases()),
        failures: Cell::new(1),
    };
    let engine = world.engine(&source);
    assert!(engine.download().is_err());
    let partial = staged_file(&engine.paths, "26.0.1", "meta-base.pkg.partial");
    fs::write(&partial, b"garbage").unwrap();
    assert!(engine.download().is_err());
    assert!(!partial.exists());
    engine.download().unwrap();
}

/// Publishes root version 2 with the given keys and signers.
fn publish_root2(world: &World, root: &[u8], signers: &[&ed25519_dalek::SigningKey]) {
    let directory = world.releases().join("v26.0.1");
    fs::write(directory.join("meta-root-2.json"), root).unwrap();
    fs::write(
        directory.join("meta-root-2.sig.json"),
        sign(Role::Root, root, signers),
    )
    .unwrap();
}

#[test]
fn rotation_replaces_and_revokes_release_keys() {
    let world = World::new();
    // Key 2 is revoked, key 4 replaces it, and the root key moves from 1 to 5.
    let root2 = root_json(2, (&[&key(5)], 1), (&[&key(4)], 1));
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    publish_root2(&world, &root2, &[&key(1), &key(5)]);
    assert_rejected(&world, "threshold not met");

    world.publish("26.0.1", 2, &[&key(4)], |_| {});
    assert_eq!(offered(&check(&world)).as_deref(), Some("26.0.1"));

    // The rotated chain is required from now on, even if its files disappear
    // and the source stops serving the rotation.
    let directory = world.releases().join("v26.0.1");
    fs::remove_file(directory.join("meta-root-2.json")).unwrap();
    assert_eq!(offered(&check(&world)).as_deref(), Some("26.0.1"));
    fs::remove_file(world.paths().root_chain().join("meta-root-2.json")).unwrap();
    let source = DirectorySource::new(world.releases());
    let error = world.engine(&source).check().unwrap_err();
    assert!(matches!(
        error,
        EngineError::TrustStore(TrustStoreError::ChainTruncated { .. })
    ));
}

#[test]
fn rotation_needs_old_and_new_root_keys() {
    let root2 = root_json(2, (&[&key(5)], 1), (&[&key(4)], 1));
    for signers in [vec![key(5)], vec![key(1)], vec![key(2), key(4)]] {
        let world = World::new();
        world.publish("26.0.1", 2, &[&key(2)], |_| {});
        publish_root2(&world, &root2, &signers.iter().collect::<Vec<_>>());
        let report = check(&world);
        // The old root stays in force.
        assert_eq!(offered(&report).as_deref(), Some("26.0.1"));
        assert!(report.warnings[0].contains("ignored meta-root-2.json"));
    }
}

#[test]
fn rotation_cannot_skip_versions() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |_| {});
    let root3 = root_json(3, (&[&key(5)], 1), (&[&key(4)], 1));
    publish_root2(&world, &root3, &[&key(1), &key(5)]);
    let report = check(&world);
    assert!(
        report.warnings[0].contains("must go from version 1 to 2"),
        "{:?}",
        report.warnings
    );
}

#[test]
fn incompatible_hosts_are_told_why() {
    let mut world = World::new();
    world.host.arch = "riscv64".into();
    world.host.osreldate = Some(1_400_000);
    world.publish("26.0.1", 2, &[&key(2)], |m| {
        m["compatibility"]["min_metaupd_version"] = "27.0".into();
    });
    let report = check(&world);
    let reasons: Vec<String> = report
        .offer
        .unwrap()
        .incompatibilities
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(reasons.len(), 3, "{reasons:?}");
}

#[test]
fn artifacts_outside_the_boot_environment_are_flagged() {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], |m| {
        m["artifacts"][0]["apply"] = "efi-system-partition".into();
    });
    let source = DirectorySource::new(world.releases());
    let engine = world.engine(&source);
    engine.download().unwrap();
    let plan = engine.plan(&engine.staged("26.0.1").unwrap());
    assert_eq!(plan.not_rolled_back, ["meta-base.pkg"]);
    assert!(plan.steps.iter().any(|step| !step.undone_by_rollback));
}

#[test]
fn version_argument_cannot_escape_staging() {
    let world = World::new();
    let source = DirectorySource::new(world.releases());
    for version in ["..", "../..", "/etc", "26.0/../.."] {
        let error = world.engine(&source).staged(version).unwrap_err();
        assert!(
            matches!(error, EngineError::InvalidVersion(_)),
            "{version}: {error}"
        );
    }
}

#[test]
fn empty_source_offers_nothing() {
    let world = World::new();
    let report = check(&world);
    assert!(report.offer.is_none() && report.rejected.is_empty());
}
