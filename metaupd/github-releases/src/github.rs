//! GitHub Releases through the documented REST API.
//!
//! Uses `GET /repos/{owner}/{repo}/releases` and the documented fields
//! `tag_name`, `draft`, `assets[].name` and `assets[].browser_download_url`.

use std::path::Path;

use metaupd_metadata::MAX_METADATA_SIZE;
use serde::Deserialize;

use crate::fetch::Fetcher;
use crate::{Asset, Release, ReleaseSource, SourceError};

pub const OFFICIAL_REPOSITORY: &str = "tnuproject/meta";

#[derive(Debug)]
pub struct GithubSource<F> {
    repository: String,
    fetcher: F,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

impl<F: Fetcher> GithubSource<F> {
    pub fn official(fetcher: F) -> Self {
        Self {
            repository: OFFICIAL_REPOSITORY.to_owned(),
            fetcher,
        }
    }

    fn parse(&self, body: &[u8]) -> Result<Vec<Release>, SourceError> {
        let releases: Vec<ApiRelease> = serde_json::from_slice(body)
            .map_err(|error| SourceError::Malformed(error.to_string()))?;
        let prefix = format!("https://github.com/{}/releases/download/", self.repository);
        Ok(releases
            .into_iter()
            .filter(|release| !release.draft)
            .map(|release| Release {
                tag: release.tag_name,
                // Assets with unsafe names or foreign URLs are dropped; a
                // manifest that needs one then fails with a missing asset.
                assets: release
                    .assets
                    .into_iter()
                    .filter(|asset| asset.browser_download_url.starts_with(&prefix))
                    .filter_map(|asset| {
                        Some(Asset {
                            name: asset.name.parse().ok()?,
                            location: asset.browser_download_url,
                        })
                    })
                    .collect(),
            })
            .collect())
    }
}

impl<F: Fetcher> ReleaseSource for GithubSource<F> {
    fn describe(&self) -> String {
        format!("GitHub Releases of {}", self.repository)
    }

    fn releases(&self) -> Result<Vec<Release>, SourceError> {
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page=30",
            self.repository
        );
        self.parse(&self.fetcher.get(&url, MAX_METADATA_SIZE)?)
    }

    fn fetch(&self, release: &Release, asset: &str, limit: u64) -> Result<Vec<u8>, SourceError> {
        self.fetcher.get(&release.asset(asset)?.location, limit)
    }

    fn download(
        &self,
        release: &Release,
        asset: &str,
        destination: &Path,
        limit: u64,
    ) -> Result<(), SourceError> {
        self.fetcher
            .download(&release.asset(asset)?.location, destination, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Canned(&'static str);

    impl Fetcher for Canned {
        fn get(&self, _url: &str, _limit: u64) -> Result<Vec<u8>, SourceError> {
            Ok(self.0.as_bytes().to_vec())
        }

        fn download(&self, _: &str, _: &Path, _: u64) -> Result<(), SourceError> {
            Err(SourceError::Transport("not in this test".into()))
        }
    }

    #[test]
    fn skips_drafts_foreign_urls_and_unsafe_names() {
        let source = GithubSource::official(Canned(
            r#"[
              {"tag_name":"v26.1","draft":true,"assets":[]},
              {"tag_name":"v26.0","draft":false,"prerelease":false,"assets":[
                {"name":"ok.pkg","browser_download_url":"https://github.com/tnuproject/meta/releases/download/v26.0/ok.pkg"},
                {"name":"evil.pkg","browser_download_url":"https://example.org/evil.pkg"},
                {"name":"fork.pkg","browser_download_url":"https://github.com/other/meta/releases/download/v26.0/fork.pkg"},
                {"name":"../x","browser_download_url":"https://github.com/tnuproject/meta/releases/download/v26.0/x"}
              ]}
            ]"#,
        ));
        let releases = source.releases().unwrap();
        assert_eq!(releases.len(), 1);
        let names: Vec<_> = releases[0].assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["ok.pkg"]);
        assert!(matches!(
            releases[0].asset("evil.pkg"),
            Err(SourceError::MissingAsset { .. })
        ));
    }

    #[test]
    fn reports_malformed_responses() {
        let source = GithubSource::official(Canned(r#"{"message":"Not Found"}"#));
        assert!(matches!(source.releases(), Err(SourceError::Malformed(_))));
    }

    #[test]
    fn empty_release_list_is_not_an_error() {
        assert!(
            GithubSource::official(Canned("[]"))
                .releases()
                .unwrap()
                .is_empty()
        );
    }
}
