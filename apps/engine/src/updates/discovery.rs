use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use semver::Version;
use serde::Deserialize;

use super::{
    http,
    manifest::{self, Manifest},
};

pub const API: &str = "https://api.github.com/repos/ohne-b/crosshair-rs/releases";
const MAX_API: u64 = 2 * 1024 * 1024;
pub const MAX_PAGES: usize = 10;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
}

fn latest_in_page(bytes: &[u8], latest: &mut Option<Version>) -> Result<bool> {
    let releases: Vec<Release> =
        serde_json::from_slice(bytes).context("Invalid GitHub release list")?;
    ensure!(
        releases.len() <= 100,
        "GitHub release page exceeds its limit"
    );
    let more = releases.len() == 100;
    for release in releases {
        if release.draft
            || !release
                .assets
                .iter()
                .any(|asset| asset.name == "latest.json")
        {
            continue;
        }
        if let Some(tag) = release.tag_name.strip_prefix('v')
            && let Ok(version) = manifest::release_version(tag)
            && latest.as_ref().is_none_or(|old| version > *old)
        {
            *latest = Some(version);
        }
    }
    Ok(more)
}

pub fn check(
    allow_prereleases: bool,
    cancellation: &http::Cancellation,
) -> Result<Option<Manifest>> {
    let started = Instant::now();
    discover(allow_prereleases, |url: &str, limit| -> Result<Vec<u8>> {
        let remaining = Duration::from_secs(20).saturating_sub(started.elapsed());
        ensure!(!remaining.is_zero(), "Update check timed out");
        let mut bytes = Vec::new();
        http::download(url, &mut bytes, limit, remaining, cancellation, |_, _| {})?;
        Ok(bytes)
    })
}

fn discover(
    allow_prereleases: bool,
    mut fetch: impl FnMut(&str, u64) -> Result<Vec<u8>>,
) -> Result<Option<Manifest>> {
    if !allow_prereleases {
        let bytes = match fetch(manifest::ENDPOINT, manifest::MAX_MANIFEST) {
            Ok(bytes) => bytes,
            Err(error) if http::is_not_found(&error) => {
                // A missing stable release is normal; a missing asset on an existing release isn't.
                match fetch(&format!("{API}/latest"), MAX_API) {
                    Err(missing) if http::is_not_found(&missing) => return Ok(None),
                    Err(other) => return Err(other),
                    Ok(_) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        return checked_manifest(&bytes, None, false).map(Some);
    }

    let mut latest = None;
    for page in 1..=MAX_PAGES {
        let bytes = fetch(&format!("{API}?per_page=100&page={page}"), MAX_API)?;
        if !latest_in_page(&bytes, &mut latest)? {
            break;
        }
        ensure!(
            page < MAX_PAGES,
            "GitHub release history exceeds the update check limit"
        );
    }
    let Some(version) = latest else {
        return Ok(None);
    };
    let url =
        format!("https://github.com/ohne-b/crosshair-rs/releases/download/v{version}/latest.json");
    checked_manifest(&fetch(&url, manifest::MAX_MANIFEST)?, Some(&version), true).map(Some)
}

pub(super) fn checked_manifest(
    bytes: &[u8],
    expected: Option<&Version>,
    allow_prereleases: bool,
) -> Result<Manifest> {
    let manifest = Manifest::parse(bytes)?;
    let version = manifest.validate()?;
    ensure!(
        allow_prereleases || version.pre.is_empty(),
        "The stable update endpoint returned a prerelease"
    );
    ensure!(
        expected.is_none_or(|expected| expected == &version),
        "Update metadata does not match the selected release"
    );
    Ok(manifest)
}

pub fn api_target(url: &str) -> Option<String> {
    if url == format!("{API}/latest")
        || (1..=MAX_PAGES).any(|page| url == format!("{API}?per_page=100&page={page}"))
    {
        Some(url["https://api.github.com".len()..].into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_stable_release_is_normal_but_missing_assets_and_api_failures_are_errors() {
        let mut calls = Vec::new();
        let result = discover(false, |url, _| {
            calls.push(url.to_owned());
            Err(http::HttpStatus(404).into())
        })
        .unwrap();
        assert!(result.is_none());
        assert_eq!(
            calls,
            [manifest::ENDPOINT.to_owned(), format!("{API}/latest")]
        );
        for api_status in [200, 403, 500] {
            assert!(
                discover(false, |url, _| {
                    if url == manifest::ENDPOINT {
                        Err(http::HttpStatus(404).into())
                    } else if api_status == 200 {
                        Ok(b"{}".to_vec())
                    } else {
                        Err(http::HttpStatus(api_status).into())
                    }
                })
                .is_err()
            );
        }
    }

    #[test]
    fn release_pages_are_bounded_and_empty_history_is_normal() {
        assert!(discover(true, |_, _| Ok(b"[]".to_vec())).unwrap().is_none());
        let entries =
            vec![serde_json::json!({"tag_name":"unrelated","draft":false,"assets":[]}); 100];
        let full = serde_json::to_vec(&entries).unwrap();
        let mut calls = 0;
        assert!(
            discover(true, |url, limit| {
                calls += 1;
                assert_eq!(url, format!("{API}?per_page=100&page={calls}"));
                assert_eq!(limit, MAX_API);
                Ok(full.clone())
            })
            .is_err()
        );
        assert_eq!(calls, MAX_PAGES);
    }

    #[test]
    fn discovery_uses_semver_and_ignores_drafts_invalid_tags_and_non_app_releases() {
        let page = serde_json::json!([
            {"tag_name":"v0.1.0-alpha.9", "draft":false,"assets":[{"name":"latest.json"}]},
            {"tag_name":"v0.1.0-alpha.10","draft":false,"assets":[{"name":"latest.json"}]},
            {"tag_name":"v9.0.0","draft":true,"assets":[{"name":"latest.json"}]},
            {"tag_name":"v8.0.0","draft":false,"assets":[]},
            {"tag_name":"v7.0.0/../x","draft":false,"assets":[{"name":"latest.json"}]}
        ]);
        let mut latest = None;
        assert!(!latest_in_page(&serde_json::to_vec(&page).unwrap(), &mut latest).unwrap());
        assert_eq!(latest.as_ref().unwrap().to_string(), "0.1.0-alpha.10");
        for version in ["0.1.0-beta.1", "0.1.0-rc.1", "0.1.0"] {
            let page = serde_json::json!([{"tag_name":format!("v{version}"),"draft":false,"assets":[{"name":"latest.json"}]}]);
            latest_in_page(&serde_json::to_vec(&page).unwrap(), &mut latest).unwrap();
            assert_eq!(latest.as_ref().unwrap().to_string(), version);
        }
        assert!(latest_in_page(b"{}", &mut latest).is_err());
    }
}
