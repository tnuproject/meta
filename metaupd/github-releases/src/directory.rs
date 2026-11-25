//! A local directory laid out as `<directory>/<tag>/<asset>`.
//!
//! Used for offline updates and for testing. Verification is identical to the
//! online source.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::{Asset, Release, ReleaseSource, SourceError};

#[derive(Debug, Clone)]
pub struct DirectorySource {
    directory: PathBuf,
}

impl DirectorySource {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
}

impl ReleaseSource for DirectorySource {
    fn describe(&self) -> String {
        format!("local directory {}", self.directory.display())
    }

    fn releases(&self) -> Result<Vec<Release>, SourceError> {
        let mut releases = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Ok(tag) = entry.file_name().into_string() else {
                continue;
            };
            let mut assets = Vec::new();
            for file in fs::read_dir(entry.path())? {
                let file = file?;
                let name = file
                    .file_name()
                    .into_string()
                    .ok()
                    .and_then(|n| n.parse().ok());
                if let (true, Some(name)) = (file.file_type()?.is_file(), name) {
                    let location = file.path().to_string_lossy().into_owned();
                    assets.push(Asset { name, location });
                }
            }
            releases.push(Release { tag, assets });
        }
        releases.sort_by(|a, b| b.tag.cmp(&a.tag));
        Ok(releases)
    }

    fn fetch(&self, release: &Release, asset: &str, limit: u64) -> Result<Vec<u8>, SourceError> {
        let mut body = Vec::new();
        File::open(&release.asset(asset)?.location)?
            .take(limit.saturating_add(1))
            .read_to_end(&mut body)?;
        if body.len() as u64 > limit {
            return Err(SourceError::TooLarge {
                what: asset.to_owned(),
                limit,
            });
        }
        Ok(body)
    }

    fn download(
        &self,
        release: &Release,
        asset: &str,
        destination: &Path,
        limit: u64,
    ) -> Result<(), SourceError> {
        let mut input = File::open(&release.asset(asset)?.location)?.take(limit.saturating_add(1));
        let mut output = File::create(destination)?;
        if std::io::copy(&mut input, &mut output)? > limit {
            return Err(SourceError::TooLarge {
                what: asset.to_owned(),
                limit,
            });
        }
        output.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_reads_with_limits() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("v1")).unwrap();
        fs::write(dir.path().join("v1/data.bin"), b"hello").unwrap();
        let source = DirectorySource::new(dir.path());
        let releases = source.releases().unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(source.fetch(&releases[0], "data.bin", 5).unwrap(), b"hello");
        assert!(matches!(
            source.fetch(&releases[0], "data.bin", 4),
            Err(SourceError::TooLarge { .. })
        ));
        assert!(source.fetch(&releases[0], "nope", 4).is_err());
    }
}
