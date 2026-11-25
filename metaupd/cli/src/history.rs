//! Append-only update history.
//!
//! Each record carries the digest of the previous line, so edits, reordering
//! and deletions in the middle are detectable. The chain is not keyed: root can
//! rewrite all of it, and truncation of the newest records is not detectable.

use std::fs::OpenOptions;
use std::io::Write;

use metaupd_metadata::{Sha256Digest, Timestamp};
use metaupd_verify::sha256;
use serde::{Deserialize, Serialize};

use crate::store::{Paths, StoreError, io_at, read_limited};

const HISTORY_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Channel,
    Download,
    RootRotation,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub time: Timestamp,
    pub action: Action,
    pub success: bool,
    pub detail: String,
    /// Digest of the previous line, or of the empty string for the first record.
    pub previous: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub records: Vec<Record>,
    last_digest: Sha256Digest,
}

pub fn load(paths: &Paths) -> Result<History, StoreError> {
    let path = paths.history();
    let data = read_limited(&path, HISTORY_LIMIT)?.unwrap_or_default();
    let corrupt = |reason: String| StoreError::Corrupt {
        path: path.clone(),
        reason,
    };
    let text = std::str::from_utf8(&data).map_err(|e| corrupt(e.to_string()))?;
    let mut history = History {
        records: Vec::new(),
        last_digest: sha256(b""),
    };
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let record: Record =
            serde_json::from_str(line).map_err(|e| corrupt(format!("line {number}: {e}")))?;
        if record.previous != history.last_digest {
            return Err(corrupt(format!(
                "line {number} does not follow the record before it"
            )));
        }
        history.last_digest = sha256(line.as_bytes());
        history.records.push(record);
    }
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(corrupt("the last record is incomplete".into()));
    }
    Ok(history)
}

pub fn append(
    paths: &Paths,
    time: Timestamp,
    action: Action,
    success: bool,
    detail: impl Into<String>,
) -> Result<(), StoreError> {
    let history = load(paths)?;
    let path = paths.history();
    let record = Record {
        time,
        action,
        success,
        detail: detail.into(),
        previous: history.last_digest,
    };
    let mut line = serde_json::to_string(&record).map_err(|e| StoreError::Corrupt {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io_at(&path))?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(io_at(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            state_dir: dir.path().into(),
            trust_dir: dir.path().into(),
        };
        let time = "2026-01-01T00:00:00Z".parse().unwrap();
        for detail in ["one", "two\nlines", "three"] {
            append(&paths, time, Action::Download, true, detail).unwrap();
        }
        (dir, paths)
    }

    #[test]
    fn appends_and_loads() {
        let (_dir, paths) = sample();
        let history = load(&paths).unwrap();
        assert_eq!(history.records.len(), 3);
        assert_eq!(history.records[1].detail, "two\nlines");
    }

    #[test]
    fn detects_edits_and_deletions() {
        let (_dir, paths) = sample();
        let text = std::fs::read_to_string(paths.history()).unwrap();
        std::fs::write(paths.history(), text.replace("one", "0ne")).unwrap();
        assert!(load(&paths).is_err());
        let without_second: String = text
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| format!("{l}\n"))
            .collect();
        std::fs::write(paths.history(), without_second).unwrap();
        assert!(load(&paths).is_err());
        std::fs::write(paths.history(), text.trim_end()).unwrap();
        assert!(load(&paths).is_err());
    }
}
