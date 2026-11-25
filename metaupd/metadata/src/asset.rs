//! Release asset names.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A release asset file name that is safe to use as a single path component.
///
/// Manifests name assets, never URLs or paths, so signed metadata cannot
/// direct a client to another host or outside the staging directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AssetName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "unsafe asset name {0:?}: use 1-128 ASCII letters, digits, '.', '_', '+' or '-', \
     not starting with '.' or '-'"
)]
pub struct AssetNameError(String);

impl AssetName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AssetName {
    type Err = AssetNameError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-');
        let valid = (1..=128).contains(&text.len())
            && text.bytes().all(allowed)
            && !text.starts_with(['.', '-'])
            && !text.ends_with(".partial");
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(AssetNameError(text.chars().take(128).collect()))
        }
    }
}

impl fmt::Display for AssetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for AssetName {
    type Error = AssetNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<AssetName> for String {
    fn from(value: AssetName) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_and_special_names() {
        for name in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "/etc/passwd",
            ".hidden",
            "-rf",
            "a b",
            "a\0b",
            "C:x",
            "x.partial",
            "é",
        ] {
            assert!(name.parse::<AssetName>().is_err(), "{name:?}");
        }
        assert!("a".repeat(129).parse::<AssetName>().is_err());
    }

    #[test]
    fn accepts_ordinary_names() {
        for name in [
            "meta-base-26.0-amd64.pkg",
            "CHANGELOG.md",
            "libstdc++_1.txz",
        ] {
            assert!(name.parse::<AssetName>().is_ok(), "{name:?}");
        }
    }
}
