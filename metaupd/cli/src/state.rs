//! Persistent client state.

use std::collections::BTreeMap;

use metaupd_metadata::{Channel, Timestamp};
use metaupd_verify::HighWater;
use serde::{Deserialize, Serialize};

use crate::apply::Pending;
use crate::store::{Paths, StoreError, read_limited, write_atomic};

const STATE_SCHEMA: u32 = 1;
const STATE_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema: u32,
    pub channel: Channel,
    /// Highest root version ever trusted; the chain on disk must reach it.
    pub root_version: u32,
    /// Newest manifest seen per publishing channel.
    pub high_water: BTreeMap<Channel, HighWater>,
    pub last_check: Option<Timestamp>,
    /// Update activated for one boot and not yet confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA,
            channel: Channel::Stable,
            root_version: 0,
            high_water: BTreeMap::new(),
            last_check: None,
            pending: None,
        }
    }
}

impl State {
    /// Loads the state. Corruption is an error, never silently reset, because
    /// resetting it would discard rollback protection.
    pub fn load(paths: &Paths) -> Result<Self, StoreError> {
        let path = paths.state_dir.join("state.json");
        let Some(data) = read_limited(&path, STATE_LIMIT)? else {
            return Ok(Self::default());
        };
        let corrupt = |reason: String| StoreError::Corrupt {
            path: path.clone(),
            reason,
        };
        let state: Self = serde_json::from_slice(&data).map_err(|e| corrupt(e.to_string()))?;
        if state.schema != STATE_SCHEMA {
            return Err(corrupt(format!("unsupported schema {}", state.schema)));
        }
        Ok(state)
    }

    pub fn save(&self, paths: &Paths) -> Result<(), StoreError> {
        let path = paths.state_dir.join("state.json");
        let data = serde_json::to_vec_pretty(self).map_err(|e| StoreError::Corrupt {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        write_atomic(&path, &data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_state_is_not_reset() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            state_dir: dir.path().into(),
            trust_dir: dir.path().into(),
        };
        assert_eq!(State::load(&paths).unwrap(), State::default());
        std::fs::write(dir.path().join("state.json"), b"{}").unwrap();
        assert!(matches!(
            State::load(&paths),
            Err(StoreError::Corrupt { .. })
        ));
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            state_dir: dir.path().into(),
            trust_dir: dir.path().into(),
        };
        let state = State {
            channel: Channel::Testing,
            root_version: 3,
            ..State::default()
        };
        state.save(&paths).unwrap();
        assert_eq!(State::load(&paths).unwrap(), state);
    }
}
