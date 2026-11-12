use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use windows::{
    ApplicationModel::Package,
    Management::Deployment::PackageManager,
    System::ProcessorArchitecture,
    Win32::{Storage::Packaging::Appx::*, System::Com::*, UI::Shell::SHCreateStreamOnFileEx},
    core::{HSTRING, Interface, PCWSTR, PWSTR},
};

use super::manifest::{self, Manifest, PACKAGE_NAME};

const FRAMEWORK: &str = "Microsoft.VCLibs.140.00";
const MICROSOFT: &str =
    "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US";
const FRAMEWORK_MINIMUM: u64 = (14_u64 << 48) | (33519_u64 << 16);
const WINDOWS_MINIMUM: u64 = (10_u64 << 48) | (22000_u64 << 16);

pub struct Apartment;
impl Apartment {
    pub fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub version: u64,
    pub family: String,
}

pub fn installed() -> Result<Option<Installed>> {
    find(PACKAGE_NAME, manifest::publisher()?)
}

fn find(name: &str, publisher: &str) -> Result<Option<Installed>> {
    let packages = PackageManager::new()?.FindPackagesByUserSecurityIdNamePublisher(
        &HSTRING::new(),
        &name.into(),
        &publisher.into(),
    )?;
    let mut found: Option<Installed> = None;
    for package in packages {
        let id = package.Id()?;
        if id.Architecture()? != ProcessorArchitecture::X64 || package.IsResourcePackage()? {
            continue;
        }
        let version = id.Version()?;
        let version = (u64::from(version.Major) << 48)
            | (u64::from(version.Minor) << 32)
            | (u64::from(version.Build) << 16)
            | u64::from(version.Revision);
        let status = package.Status()?;
        ensure!(
            !status.DeploymentInProgress()? && !status.Servicing()?,
            "Windows is still updating {name}; wait, then reopen the app"
        );
        ensure!(
            status.VerifyIsOK()?,
            "Installed package {name} needs repair"
        );
        if found.as_ref().is_none_or(|old| old.version < version) {
            found = Some(Installed {
                version,
                family: id.FamilyName()?.to_string(),
            });
        }
    }
    Ok(found)
}

pub fn current() -> Result<Installed> {
    let package = Package::Current().context("Install the MSIX to use in-app updates")?;
    let id = package.Id()?;
    ensure!(
        id.Name()? == PACKAGE_NAME && id.Publisher()? == manifest::publisher()?,
        "This installation is not on the configured stable update channel"
    );
    let result = installed()?.context("The application package is not registered for this user")?;
    ensure!(
        result.version
            == manifest::mapped_version(&manifest::release_version(
                crosshair_core::protocol::PRODUCT_VERSION
            )?),
        "The running and installed versions differ; restart before updating"
    );
    Ok(result)
}

/// This read handle denies writes and deletion until deployment completes.
pub fn locked_file(path: &Path) -> Result<File> {
    use std::os::windows::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.file_attributes() & 0x400 == 0,
        "The update must be a regular local file"
    );
    OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .context("Could not lock the verified update package")
}

/// Package identity is checked after Minisign authenticates the locked download.
pub fn validate(path: &Path, manifest: &Manifest) -> Result<String> {
    let version = manifest.validate()?;
    let _locked = locked_file(path)?;
    let path = wide_path(path);
    unsafe {
        let stream = SHCreateStreamOnFileEx(
            PCWSTR(path.as_ptr()),
            (STGM_READ | STGM_SHARE_DENY_WRITE).0,
            0,
            false,
            None,
        )?;
        let factory: IAppxFactory = CoCreateInstance(&AppxFactory, None, CLSCTX_INPROC_SERVER)?;
        let reader = factory.CreatePackageReader(&stream)?;
        let package_manifest = reader.GetManifest()?;
        let identity = package_manifest.GetPackageId()?;
        ensure!(
            take_string(identity.GetName()?)? == PACKAGE_NAME,
            "The update package has the wrong identity"
        );
        ensure!(
            take_string(identity.GetPublisher()?)? == manifest::publisher()?,
            "The update package has the wrong publisher"
        );
        ensure!(
            identity.GetArchitecture()? == APPX_PACKAGE_ARCHITECTURE_X64,
            "The update package is not x64"
        );
        ensure!(
            identity.GetVersion()? == manifest::mapped_version(&version),
            "The signed release version does not match the Windows package"
        );

        let families = package_manifest
            .cast::<IAppxManifestReader3>()?
            .GetTargetDeviceFamilies()?;
        ensure!(
            families.GetHasCurrent()?.as_bool(),
            "The update has no Windows desktop requirement"
        );
        let family = families.GetCurrent()?;
        ensure!(
            take_string(family.GetName()?)? == "Windows.Desktop"
                && family.GetMinVersion()? == WINDOWS_MINIMUM
                && !families.MoveNext()?.as_bool(),
            "This update changes Windows requirements; use the release's manual installer"
        );

        let dependencies = package_manifest.GetPackageDependencies()?;
        ensure!(
            dependencies.GetHasCurrent()?.as_bool(),
            "The update is missing its framework dependency"
        );
        let dependency = dependencies.GetCurrent()?;
        ensure!(
            take_string(dependency.GetName()?)? == FRAMEWORK
                && take_string(dependency.GetPublisher()?)? == MICROSOFT
                && dependency.GetMinVersion()? == FRAMEWORK_MINIMUM
                && !dependencies.MoveNext()?.as_bool(),
            "This update changes framework requirements; use the release's manual installer"
        );
        ensure!(
            find(FRAMEWORK, MICROSOFT)?.is_some_and(|package| package.version >= FRAMEWORK_MINIMUM),
            "Microsoft VCLibs x64 is missing or outdated; repair it before installing this update"
        );
        take_string(identity.GetPackageFamilyName()?)
    }
}

unsafe fn take_string(value: PWSTR) -> Result<String> {
    unsafe {
        let result = value.to_string();
        CoTaskMemFree(Some(value.0.cast()));
        Ok(result?)
    }
}

pub fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
