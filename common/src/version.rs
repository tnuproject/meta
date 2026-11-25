//! META version identity and ordering.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub const PROJECT: &str = "META";
pub const VERSION: &str = "26.0";
pub const CODENAME: &str = "uglydemon";
pub const TAGLINE: &str = "FreeBSD at its core. Linux compatibility by design.";
/// Development stage of this source tree. Nothing has been released yet.
pub const RELEASE_STAGE: &str = "pre-release development";

/// A META version such as `26.0`, `26.0.1` or `26.1-rc2`.
///
/// Pre-releases order before the release they precede:
/// `26.0-alpha1 < 26.0-beta1 < 26.0-rc1 < 26.0 < 26.0.1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MetaVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre: Option<PreRelease>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreRelease {
    pub stage: Stage,
    pub number: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Alpha,
    Beta,
    Rc,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid META version {0:?}: expected MAJOR.MINOR[.PATCH][-(alpha|beta|rc)N]")]
pub struct VersionParseError(String);

impl FromStr for MetaVersion {
    type Err = VersionParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let err = || VersionParseError(input.chars().take(64).collect());
        let (numbers, pre) = match input.split_once('-') {
            Some((numbers, pre)) => (numbers, Some(parse_pre(pre).ok_or_else(err)?)),
            None => (input, None),
        };
        let parts = numbers
            .split('.')
            .map(parse_number)
            .collect::<Option<Vec<u32>>>()
            .ok_or_else(err)?;
        let (major, minor, patch) = match parts.as_slice() {
            [major, minor] => (*major, *minor, 0),
            [major, minor, patch] => (*major, *minor, *patch),
            _ => return Err(err()),
        };
        Ok(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

/// Rejects signs, whitespace and leading zeros so each number has one spelling.
fn parse_number(text: &str) -> Option<u32> {
    let canonical = text == "0" || !text.starts_with('0');
    if text.is_empty() || !canonical || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_pre(text: &str) -> Option<PreRelease> {
    let (stage, rest) = [
        ("alpha", Stage::Alpha),
        ("beta", Stage::Beta),
        ("rc", Stage::Rc),
    ]
    .into_iter()
    .find_map(|(name, stage)| text.strip_prefix(name).map(|rest| (stage, rest)))?;
    Some(PreRelease {
        stage,
        number: parse_number(rest)?,
    })
}

impl fmt::Display for MetaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        if self.patch != 0 {
            write!(f, ".{}", self.patch)?;
        }
        if let Some(pre) = self.pre {
            let stage = match pre.stage {
                Stage::Alpha => "alpha",
                Stage::Beta => "beta",
                Stage::Rc => "rc",
            };
            write!(f, "-{stage}{}", pre.number)?;
        }
        Ok(())
    }
}

impl Ord for MetaVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre, other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(&b),
            })
    }
}

impl PartialOrd for MetaVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<String> for MetaVersion {
    type Error = VersionParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<MetaVersion> for String {
    fn from(value: MetaVersion) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> MetaVersion {
        text.parse().unwrap()
    }

    #[test]
    fn orders_prereleases_before_releases() {
        let ordered = [
            "25.9.9",
            "26.0-alpha1",
            "26.0-alpha10",
            "26.0-beta1",
            "26.0-rc1",
            "26.0",
            "26.0.1",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn round_trips_through_display() {
        for text in ["26.0", "26.0.1", "26.1-rc2", "0.0"] {
            assert_eq!(v(text).to_string(), text);
        }
        assert_eq!(v("26.0.0"), v("26.0"));
    }

    #[test]
    fn rejects_malformed_versions() {
        for text in [
            "",
            "26",
            "26.",
            "026.0",
            "26.0-",
            "26.0-rc",
            "26.0-rc01",
            "+26.0",
            "26.0.0.0",
            "26.0 ",
            "26.0-dev1",
            "99999999999.0",
        ] {
            assert!(
                text.parse::<MetaVersion>().is_err(),
                "{text:?} should be rejected"
            );
        }
    }

    #[test]
    fn current_version_constant_parses() {
        assert_eq!(v(VERSION).to_string(), VERSION);
    }
}
