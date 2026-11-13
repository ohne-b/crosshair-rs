use std::io::Read;

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ENDPOINT: &str =
    "https://github.com/ohne-b/crosshair-rs/releases/latest/download/latest.json";
pub const PACKAGE_NAME: &str = "OhneB.crosshair-rs";
pub const MAX_MANIFEST: u64 = 64 * 1024;
pub const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const RELEASES: &str = "https://github.com/ohne-b/crosshair-rs/releases/";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub version: String,
    pub notes: String,
    pub pub_date: String,
    pub platforms: Platforms,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Platforms {
    #[serde(rename = "windows-x86_64-nsis")]
    pub windows: Artifact,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub signature: String,
    pub package_signature: String,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() as u64 <= MAX_MANIFEST,
            "Update metadata exceeds 64 KiB"
        );
        let value: Self = serde_json::from_slice(bytes).context("Invalid update metadata")?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<Version> {
        ensure!(
            self.schema_version == 1,
            "Unsupported update metadata version"
        );
        let version = release_version(&self.version)?;
        ensure!(
            self.notes
                == format!(
                    "[Check release notes on GitHub]({RELEASES}tag/v{})",
                    self.version
                ),
            "Unexpected release notes link"
        );
        ensure!(valid_date(&self.pub_date), "Invalid release date");
        let artifact = self.artifact();
        ensure!(
            artifact.url
                == format!(
                    "{RELEASES}download/v{0}/crosshair-rs-{0}-windows-x64-setup.exe",
                    self.version
                ),
            "Unexpected update asset URL"
        );
        for encoded in [&artifact.signature, &artifact.package_signature] {
            ensure!(encoded.len() <= 4096, "Update signature exceeds its limit");
            ensure!(
                signature(encoded)?.trusted_comment() == format!("version:{}", self.version),
                "Signature does not authenticate this release version"
            );
        }
        Ok(version)
    }

    pub fn artifact(&self) -> &Artifact {
        &self.platforms.windows
    }
}

pub fn release_version(value: &str) -> Result<Version> {
    ensure!(value.len() <= 128, "Version exceeds its limit");
    let version = Version::parse(value).context("Invalid release version")?;
    ensure!(
        version.build.is_empty() && version.to_string() == value,
        "Release versions must be canonical and cannot include build metadata"
    );
    ensure!(
        [version.major, version.minor, version.patch]
            .into_iter()
            .all(|part| part <= u16::MAX as u64),
        "Version exceeds Windows package limits"
    );
    if !version.pre.is_empty() {
        let (stage, number) = version
            .pre
            .as_str()
            .split_once('.')
            .context("Expected alpha.N, beta.N or rc.N")?;
        let number: u16 = number.parse().context("Invalid prerelease number")?;
        ensure!(
            matches!(stage, "alpha" | "beta" | "rc") && number < 20000,
            "Unsupported prerelease version"
        );
    }
    Ok(version)
}

pub fn mapped_version(version: &Version) -> u64 {
    let revision = if version.pre.is_empty() {
        65535
    } else {
        let (stage, number) = version
            .pre
            .as_str()
            .split_once('.')
            .expect("validated release version");
        let offset = match stage {
            "alpha" => 0,
            "beta" => 20000,
            "rc" => 40000,
            _ => unreachable!("validated release stage"),
        };
        offset + number.parse::<u64>().expect("validated release number")
    };
    (version.major << 48) | (version.minor << 32) | (version.patch << 16) | revision
}

pub fn product_version(package: u64) -> String {
    let base = format!(
        "{}.{}.{}",
        package >> 48,
        (package >> 32) & 65535,
        (package >> 16) & 65535
    );
    let revision = package & 65535;
    match revision {
        0..20000 => format!("{base}-alpha.{revision}"),
        20000..40000 => format!("{base}-beta.{}", revision - 20000),
        40000..60000 => format!("{base}-rc.{}", revision - 40000),
        _ => base,
    }
}

pub fn public_key() -> Result<PublicKey> {
    let encoded = option_env!("CROSSHAIR_UPDATE_PUBLIC_KEY")
        .filter(|key| !key.is_empty())
        .context(
            "Updates are not configured in this build: the release verification key is missing",
        )?;
    PublicKey::from_base64(encoded)
        .context("This build contains an invalid update verification key")
}

pub fn publisher() -> Result<&'static str> {
    option_env!("CROSSHAIR_PACKAGE_PUBLISHER")
        .filter(|value| !value.is_empty())
        .context("Updates are not configured in this build: the Windows publisher is missing")
}

fn signature(encoded: &str) -> Result<Signature> {
    let decoded = STANDARD
        .decode(encoded)
        .context("Invalid update signature encoding")?;
    Signature::decode(std::str::from_utf8(&decoded)?).context("Invalid Minisign signature")
}

pub fn verify(
    reader: &mut impl Read,
    manifest: &Manifest,
    key: &PublicKey,
    cancelled: impl FnMut() -> Result<()>,
) -> Result<(u64, String)> {
    manifest.validate()?;
    verify_bytes(
        reader,
        &manifest.artifact().package_signature,
        key,
        cancelled,
    )
}

pub fn verify_setup(
    reader: &mut impl Read,
    manifest: &Manifest,
    key: &PublicKey,
    cancelled: impl FnMut() -> Result<()>,
) -> Result<(u64, String)> {
    manifest.validate()?;
    verify_bytes(reader, &manifest.artifact().signature, key, cancelled)
}

fn verify_bytes(
    reader: &mut impl Read,
    encoded: &str,
    key: &PublicKey,
    mut cancelled: impl FnMut() -> Result<()>,
) -> Result<(u64, String)> {
    let signature = signature(encoded)?;
    // Release signatures must use Minisign's prehashed mode. This verifier
    // authenticates both the streamed bytes and the trusted version comment.
    let mut verifier = key
        .verify_stream(&signature)
        .context("The release requires a prehashed Minisign signature")?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled()?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .context("Package size overflow")?;
        ensure!(size <= MAX_PACKAGE, "Update package exceeds 512 MiB");
        verifier.update(&buffer[..count]);
        hash.update(&buffer[..count]);
    }
    ensure!(size > 0, "Update package is empty");
    verifier
        .finalize()
        .context("The update's Minisign signature is not valid")?;
    Ok((size, format!("{:x}", hash.finalize())))
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || !bytes.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
    {
        return false;
    }
    let year: u32 = value[..4].parse().unwrap();
    let month: usize = value[5..7].parse().unwrap();
    let day: u32 = value[8..10].parse().unwrap();
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    (1..=12).contains(&month)
        && day > 0
        && day <= lengths[month - 1]
        && &value[11..13] <= "23"
        && &value[14..16] <= "59"
        && &value[17..19] <= "59"
}

/// Only these constant hosts ever reach WinHTTP. Paths are kept separate from
/// hosts so URI normalization cannot turn untrusted path text into an origin.
pub fn request_target(url: &str, initial: &str, redirect: bool) -> Result<(&'static str, String)> {
    ensure!(
        url.len() <= 16 * 1024
            && url.is_ascii()
            && !url
                .bytes()
                .any(|b| b <= b' ' || b == 127 || b == b'\\' || b == b'#'),
        "Invalid update URL"
    );
    if url == initial {
        if let Some(path) = super::discovery::api_target(initial) {
            ensure!(!redirect, "GitHub API redirects are not accepted");
            return Ok(("api.github.com", path));
        }
        let asset = initial
            .strip_prefix(&format!("{RELEASES}download/v"))
            .and_then(|path| path.split_once('/'))
            .is_some_and(|(version, file)| {
                release_version(version).is_ok()
                    && (file == "latest.json"
                        || file == format!("crosshair-rs-{version}-windows-x64-setup.exe"))
            });
        ensure!(
            initial == ENDPOINT || asset,
            "Unexpected initial update origin"
        );
        return Ok(("github.com", url["https://github.com".len()..].into()));
    }
    ensure!(redirect, "Unexpected update URL");
    ensure!(
        super::discovery::api_target(initial).is_none(),
        "GitHub API redirects are not accepted"
    );
    if initial == ENDPOINT
        && let Some(path) = url.strip_prefix(&format!("{RELEASES}download/v"))
    {
        let tag = path
            .strip_suffix("/latest.json")
            .context("Unexpected metadata redirect")?;
        release_version(tag)?;
        return Ok(("github.com", url["https://github.com".len()..].into()));
    }
    for host in [
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
    ] {
        if let Some(path) =
            url.strip_prefix(&format!("https://{host}/github-production-release-asset/"))
        {
            ensure!(!path.is_empty(), "Empty GitHub release redirect");
            return Ok((host, format!("/github-production-release-asset/{path}")));
        }
    }
    anyhow::bail!("Update redirected outside the allowed GitHub asset origins")
}
