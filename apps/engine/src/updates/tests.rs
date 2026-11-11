use base64::{Engine, engine::general_purpose::STANDARD};
use minisign_verify::PublicKey;

use super::{
    http::Cancellation,
    manifest::{self, Artifact, Manifest, Platforms},
};

// Public RFC 8032 section 7.1 test vector 1, framed as Minisign with test key ID
// 5445535400000001. It is never used by production key selection. No secret or
// signing operation is needed by the tests.
const KEY: &str = "RWRURVNUAAAAAddamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea";
const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHB1YmxpYyBSRkMgODAzMiB0ZXN0IHZlY3RvcjsgbmV2ZXIgYSByZWxlYXNlIGtleQpSVVJVUlZOVUFBQUFBVGo1NEJCUHJHRmJnNk9VeGdFTmNyU2lreEVRMkFaNytBRlpzR3VUMU1mY1FKaFpCRFdZbmJXQXFIMVNJMlNVeDJjUlp1dWRvUXFDTFlodldIallVZ0U9CnRydXN0ZWQgY29tbWVudDogdmVyc2lvbjoxLjIuMwpkSXFjRXJoTWNybTZacnRoOWJ2T05Gd1NBR1d2MFBCS0x6SFR0RHNGbFpaWHJ4VWtpNllEeTBKczhGZVU4NnhiNTdROU40ajZLaGFsR1cwZUt5SHhEQT09Cg==";
const DATA: &[u8] = b"crosshair-rs synthetic package fixture\n";

pub(super) fn release() -> Manifest {
    Manifest {
        schema_version: 1,
        version: "1.2.3".into(),
        notes: "https://github.com/ohne-b/crosshair-rs/releases/tag/v1.2.3".into(),
        pub_date: "2026-11-11T12:00:00Z".into(),
        platforms: Platforms { windows: Artifact {
            url: "https://github.com/ohne-b/crosshair-rs/releases/download/v1.2.3/crosshair-rs-1.2.3-windows-x64.msix".into(),
            signature: SIGNATURE.into(),
        } },
    }
}

#[test]
fn streamed_signature_authenticates_bytes_and_version_comment() {
    let key = PublicKey::from_base64(KEY).unwrap();
    let release = release();
    let (size, hash) = manifest::verify(&mut &DATA[..], &release, &key, || Ok(())).unwrap();
    assert_eq!(size, DATA.len() as u64);
    assert_eq!(hash.len(), 64);
    assert!(manifest::verify(&mut b"changed".as_slice(), &release, &key, || Ok(())).is_err());
    assert!(
        manifest::verify(
            &mut &DATA[..],
            &release,
            &PublicKey::from_base64("RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3")
                .unwrap(),
            || Ok(())
        )
        .is_err()
    );

    let mut forged = release.clone();
    forged.version = "2.0.0".into();
    forged.notes = forged.notes.replace("1.2.3", "2.0.0");
    let artifact = &mut forged.platforms.windows;
    artifact.url = artifact.url.replace("1.2.3", "2.0.0");
    let signature = String::from_utf8(STANDARD.decode(SIGNATURE).unwrap())
        .unwrap()
        .replace("version:1.2.3", "version:2.0.0");
    artifact.signature = STANDARD.encode(signature);
    assert!(forged.validate().is_ok()); // Syntactic metadata is still untrusted.
    assert!(manifest::verify(&mut &DATA[..], &forged, &key, || Ok(())).is_err());
}

#[test]
fn cancellation_and_reader_failures_never_verify() {
    let cancellation = Cancellation::default();
    cancellation.cancel();
    assert!(
        manifest::verify(
            &mut &DATA[..],
            &release(),
            &PublicKey::from_base64(KEY).unwrap(),
            || cancellation.check()
        )
        .is_err()
    );
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fixture disk failure"))
        }
    }
    assert!(
        manifest::verify(
            &mut Broken,
            &release(),
            &PublicKey::from_base64(KEY).unwrap(),
            || Ok(())
        )
        .is_err()
    );
}

#[test]
fn metadata_rejects_oversize_unknown_schema_and_relabeled_versions() {
    assert!(Manifest::parse(&vec![b' '; manifest::MAX_MANIFEST as usize + 1]).is_err());
    let mut value = serde_json::to_value(release()).unwrap();
    value["unknown"] = true.into();
    assert!(Manifest::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut value = release();
    value.schema_version = 2;
    assert!(value.validate().is_err());
    let mut value = release();
    value.version = "1.2.4".into();
    assert!(value.validate().is_err());
    let mut value = release();
    value.notes.push_str("?html=true");
    assert!(value.validate().is_err());
    let mut value = release();
    value.platforms.windows.url.push_str("?other=true");
    assert!(value.validate().is_err());
    let mut value = release();
    value.pub_date = "2026-99-11T12:00:00Z".into();
    assert!(value.validate().is_err());
    let mut value = release();
    value.pub_date = "2025-02-29T12:00:00Z".into();
    assert!(value.validate().is_err());
    value.pub_date = "2024-02-29T12:00:00Z".into();
    assert!(value.validate().is_ok());
    let text = serde_json::to_string(&release()).unwrap();
    let duplicate = text.replace(
        "\"schemaVersion\":1",
        "\"schemaVersion\":1,\"schemaVersion\":1",
    );
    assert!(Manifest::parse(duplicate.as_bytes()).is_err());
    let artifact = serde_json::to_string(release().artifact()).unwrap();
    let duplicate = text.replace(
        &format!("\"windows-x86_64-msix\":{artifact}"),
        &format!("\"windows-x86_64-msix\":{artifact},\"windows-x86_64-msix\":{artifact}"),
    );
    assert!(Manifest::parse(duplicate.as_bytes()).is_err());
}

#[test]
fn versions_use_semver_and_fit_windows_without_downgrading() {
    for invalid in [
        "v1.2.3",
        "1.2",
        "01.2.3",
        "1.2.3-rc.1",
        "1.2.3+meta",
        "65536.0.0",
    ] {
        assert!(manifest::stable_version(invalid).is_err(), "{invalid}");
    }
    let old = manifest::stable_version("1.9.9").unwrap();
    let new = manifest::stable_version("1.10.0").unwrap();
    assert!(new > old);
    assert!(manifest::mapped_version(&new) > manifest::mapped_version(&old));
    assert_eq!(manifest::mapped_version(&new), (1 << 48) | (10 << 32));
}

#[test]
fn redirects_cannot_change_origin_scheme_port_repository_or_credentials() {
    let initial = manifest::ENDPOINT;
    assert!(manifest::request_target(initial, initial, false).is_ok());
    assert!(
        manifest::request_target(
            "https://github.com/ohne-b/crosshair-rs/releases/download/v1.2.3/latest.json",
            initial,
            true
        )
        .is_ok()
    );
    assert!(manifest::request_target("https://release-assets.githubusercontent.com/github-production-release-asset/12/34?sig=fixture", initial, true).is_ok());
    for url in [
        "http://github.com/ohne-b/crosshair-rs/releases/latest/download/latest.json",
        "https://github.com.evil.test/ohne-b/crosshair-rs/releases/latest/download/latest.json",
        "https://user@release-assets.githubusercontent.com/github-production-release-asset/1/2",
        "https://release-assets.githubusercontent.com:444/github-production-release-asset/1/2",
        "https://release-assets.githubusercontent.com\\@evil.test/github-production-release-asset/1/2",
        "https://github.com/other/crosshair-rs/releases/download/v1.2.3/latest.json",
        "https://github.com/ohne-b/crosshair-rs/releases/download/v1.2.3/../latest.json",
        "https://github.com/ohne-b/crosshair-rs/releases/download/v1.2.3%2flatest.json",
        "https://release-assets.githubusercontent.com/github-production-release-asset/1/2#fragment",
    ] {
        assert!(
            manifest::request_target(url, initial, true).is_err(),
            "{url}"
        );
    }
}

#[test]
fn package_lock_prevents_replacement_and_corrupt_package_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fixture.msix");
    std::fs::write(&path, DATA).unwrap();
    let file = super::package::locked_file(&path).unwrap();
    assert!(std::fs::write(&path, b"replacement").is_err());
    assert!(std::fs::remove_file(&path).is_err());
    let _apartment = super::package::Apartment::new().unwrap();
    assert!(super::package::validate(&path, &file, &release()).is_err());
}

#[test]
fn missing_release_key_fails_closed_before_network_work() {
    if option_env!("CROSSHAIR_UPDATE_PUBLIC_KEY").is_none() {
        assert!(
            manifest::public_key()
                .unwrap_err()
                .to_string()
                .contains("not configured")
        );
        let directory = tempfile::tempdir().unwrap();
        let updates = super::Updates::new(directory.path().to_owned(), |_| {}).unwrap();
        updates.check().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while updates.status().phase == crosshair_core::protocol::UpdatePhase::Checking
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            updates.status().phase,
            crosshair_core::protocol::UpdatePhase::Failed
        );
        assert!(
            updates
                .status()
                .error
                .unwrap()
                .contains("verification key is missing")
        );
    }
}
