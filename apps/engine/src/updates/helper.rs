use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    mem::size_of,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use crosshair_core::Id;
use crosshair_platform::identity::{Identity, owned, raw, wide};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows::{
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Com::*, Diagnostics::ToolHelp::*, LibraryLoader::SetDllDirectoryW, Threading::*},
        UI::Shell::*,
    },
    core::{PCWSTR, PWSTR},
};

use super::{
    Ready,
    manifest::{self, Manifest},
    package,
};

// WinBase.h constants not included in the Win32 metadata projection.
const PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_ENABLE_PROCESS_TREE: u32 = 0x01;
const PROCESS_CREATION_MITIGATION_POLICY_IMAGE_LOAD_PREFER_SYSTEM32_ALWAYS_ON: u64 =
    0x1000_0000_0000_0000;
const PROCESS_CREATION_MITIGATION_POLICY_BLOCK_NON_MICROSOFT_BINARIES_ALWAYS_ON: u64 =
    0x0000_1000_0000_0000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartIntent {
    pub visible: bool,
    pub reopen_editor: bool,
}

#[derive(Debug)]
pub struct Recovery {
    pub installed_version: Option<String>,
    pub restart: RestartIntent,
    pub succeeded: bool,
    pub error: Option<String>,
    job: Id,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    job: Id,
    manifest: Manifest,
    sha256: String,
    bytes: u64,
    family: String,
    engine_sha256: String,
    restart: RestartIntent,
    phase: Phase,
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    TakenOver,
    Deploying,
    Installed,
    Failed,
}

pub struct VerifiedHandoff {
    root: PathBuf,
    journal: Journal,
    package: File,
    update_lock: File,
    helper_image: File,
}

pub struct HelperTakeover {
    _process: OwnedHandle,
}

pub(super) fn prepare(
    root: &Path,
    ready: &Ready,
    restart: RestartIntent,
) -> Result<VerifiedHandoff> {
    let _apartment = package::Apartment::new()?;
    let current = package::current()?;
    let update_lock = lock(root)?;
    if root.join("current.json").exists() {
        let previous = read_journal(root)?;
        ensure!(
            matches!(previous.phase, Phase::Failed)
                && previous.job == ready.job
                && previous.sha256 == ready.sha256,
            "A previous update needs recovery before another installation"
        );
    }
    let mut file = ready.file.try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let (bytes, digest) =
        manifest::verify(&mut file, &ready.manifest, &manifest::public_key()?, || {
            Ok(())
        })?;
    ensure!(
        bytes == ready.bytes && digest == ready.sha256,
        "The downloaded package changed after verification"
    );
    let job = root.join(ready.job.to_string());
    let family = package::validate(&job.join("package.msix"), &ready.manifest)?;
    ensure!(
        family == current.family && family == ready.family,
        "The update targets a different application family"
    );
    ensure!(
        manifest::mapped_version(&ready.manifest.validate()?) > current.version,
        "The update would downgrade or reinstall the current version"
    );
    let source = std::env::current_exe()?;
    let source = package::locked_file(&source)?;
    let helper = job.join("crosshair-update.exe");
    {
        let mut target = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&helper)?;
        std::io::copy(&mut &source, &mut target)?;
        target.sync_all()?;
    }
    let mut helper_image = package::locked_file(&helper)?;
    let engine_sha256 = file_hash(&mut helper_image, 128 * 1024 * 1024)?;
    let journal = Journal {
        schema: 1,
        job: ready.job,
        manifest: ready.manifest.clone(),
        sha256: digest,
        bytes,
        family,
        engine_sha256,
        restart,
        phase: Phase::Prepared,
        error: None,
    };
    write_journal(root, &journal)?;
    Ok(VerifiedHandoff {
        root: root.to_owned(),
        journal,
        package: file,
        update_lock,
        helper_image,
    })
}

/// Call after durable state flush. A successful return means a separate,
/// authenticated process has reverified the update and is waiting for this
/// engine to exit. Only then may the engine close its editor/widget and quit.
pub fn launch_handoff(mut handoff: VerifiedHandoff) -> Result<HelperTakeover> {
    let result = launch(&handoff);
    if let Err(error) = &result {
        handoff.journal.phase = Phase::Failed;
        handoff.journal.error = Some(format!("Update handoff failed: {error:#}"));
        let _ = write_journal(&handoff.root, &handoff.journal);
    }
    result
}

fn launch(handoff: &VerifiedHandoff) -> Result<HelperTakeover> {
    unsafe {
        let identity = Identity::current()?;
        ensure!(
            identity.package.is_some(),
            "An update helper must be launched by the installed application"
        );
        let event = owned(CreateEventW(None, true, false, PCWSTR::null())?);
        let parent = duplicate(
            GetCurrentProcess(),
            PROCESS_QUERY_LIMITED_INFORMATION.0 | PROCESS_SYNCHRONIZE.0,
        )?;
        let ready = duplicate(
            raw(&event),
            EVENT_MODIFY_STATE.0 | SYNCHRONIZATION_SYNCHRONIZE.0,
        )?;
        let package_handle = duplicate(HANDLE(handoff.package.as_raw_handle()), 0)?;
        let lock_handle = duplicate(HANDLE(handoff.update_lock.as_raw_handle()), 0)?;
        let handles = [
            raw(&ready),
            raw(&parent),
            raw(&package_handle),
            raw(&lock_handle),
        ];

        let mut attribute_bytes = 0;
        let _ = InitializeProcThreadAttributeList(None, 3, None, &mut attribute_bytes);
        ensure!(
            attribute_bytes > 0 && attribute_bytes < 65536,
            "Invalid process attribute size"
        );
        let mut storage = vec![0_usize; attribute_bytes.div_ceil(size_of::<usize>())];
        let attributes = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        InitializeProcThreadAttributeList(Some(attributes), 3, None, &mut attribute_bytes)?;
        let _attributes = Attributes(attributes);
        let policy = PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_ENABLE_PROCESS_TREE;
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY as usize,
            Some((&policy as *const u32).cast()),
            size_of::<u32>(),
            None,
            None,
        )?;
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(handles.as_ptr().cast()),
            size_of_val(&handles),
            None,
            None,
        )?;
        let mitigation = PROCESS_CREATION_MITIGATION_POLICY_IMAGE_LOAD_PREFER_SYSTEM32_ALWAYS_ON
            | PROCESS_CREATION_MITIGATION_POLICY_BLOCK_NON_MICROSOFT_BINARIES_ALWAYS_ON;
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY as usize,
            Some((&mitigation as *const u64).cast()),
            size_of::<u64>(),
            None,
            None,
        )?;
        let helper = handoff
            .root
            .join(handoff.journal.job.to_string())
            .join("crosshair-update.exe");
        let journal = handoff.root.join("current.json");
        let command = format!(
            "\"{}\" --update-helper \"{}\" {} {} {} {}",
            helper.display(),
            journal.display(),
            handles[0].0 as usize,
            handles[1].0 as usize,
            handles[2].0 as usize,
            handles[3].0 as usize
        );
        let executable = package::wide_path(&helper);
        let mut command = wide(&command);
        let directory = package::wide_path(helper.parent().unwrap());
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: size_of::<STARTUPINFOEXW>() as u32,
                ..Default::default()
            },
            lpAttributeList: attributes,
        };
        let mut process = PROCESS_INFORMATION::default();
        CreateProcessW(
            PCWSTR(executable.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_BREAKAWAY_FROM_JOB,
            None,
            PCWSTR(directory.as_ptr()),
            &startup.StartupInfo,
            &mut process,
        )?;
        let process_handle = owned(process.hProcess);
        let thread = owned(process.hThread);
        let validate = (|| {
            let helper_identity = Identity::process(process.dwProcessId)?;
            ensure!(
                helper_identity.package.is_none(),
                "Windows did not release the helper from package identity; the running app was kept open"
            );
            ensure!(
                helper_identity.user_sid == identity.user_sid
                    && helper_identity.logon_sid == identity.logon_sid
                    && helper_identity.session == identity.session
                    && helper_identity.image == helper.canonicalize()?,
                "Update helper identity mismatch"
            );
            ensure!(
                ResumeThread(raw(&thread)) != u32::MAX,
                "Could not start the update helper"
            );
            let wait = WaitForMultipleObjects(&[raw(&event), raw(&process_handle)], false, 30_000);
            ensure!(
                wait == WAIT_OBJECT_0,
                "The update helper failed to take over within 30 seconds"
            );
            Ok(())
        })();
        if let Err(error) = validate {
            let _ = TerminateProcess(raw(&process_handle), 1);
            let _ = WaitForSingleObject(raw(&process_handle), 5000);
            return Err(error);
        }
        // Keep the copied image locked through creation. The child owns its
        // package and update-lock handles independently of this process.
        let _ = &handoff.helper_image;
        Ok(HelperTakeover {
            _process: process_handle,
        })
    }
}

pub fn run_helper(arguments: &[String]) -> Result<()> {
    ensure!(arguments.len() == 5, "Invalid update helper arguments");
    let _apartment = package::Apartment::new()?;
    unsafe {
        SetDllDirectoryW(windows::core::w!(""))?;
    }
    let own = Identity::current()?;
    ensure!(
        own.package.is_none(),
        "The update helper still has package identity"
    );
    let journal_path = PathBuf::from(&arguments[0]).canonicalize()?;
    let root = journal_path
        .parent()
        .context("Missing update directory")?
        .to_owned();
    ensure!(
        journal_path
            .file_name()
            .is_some_and(|name| name == "current.json"),
        "Invalid update journal name"
    );
    let mut journal = read_journal(&root)?;
    let job = root.join(journal.job.to_string()).canonicalize()?;
    ensure!(
        own.image == job.join("crosshair-update.exe") && job.parent() == Some(root.as_path()),
        "The update helper is outside its private staging directory"
    );
    let inherited = arguments[1..]
        .iter()
        .map(|value| -> Result<OwnedHandle> {
            let value: usize = value.parse()?;
            ensure!(
                value > 0 && value < isize::MAX as usize,
                "Invalid inherited update handle"
            );
            let handle = HANDLE(value as _);
            let mut flags = 0;
            unsafe {
                GetHandleInformation(handle, &mut flags)?;
                Ok(owned(handle))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let mut handles = inherited.into_iter();
    let event = handles.next().unwrap();
    let parent = handles.next().unwrap();
    let mut file = File::from(handles.next().unwrap());
    let update_lock = File::from(handles.next().unwrap());
    let result = (|| {
        let parent_id = unsafe { GetProcessId(raw(&parent)) };
        ensure!(parent_id != 0, "Invalid update parent process");
        let parent_identity = Identity::process(parent_id)?;
        ensure!(
            parent_identity.package_family.as_deref() == Some(journal.family.as_str())
                && parent_identity.user_sid == own.user_sid
                && parent_identity.logon_sid == own.logon_sid
                && parent_identity.session == own.session,
            "Update parent identity mismatch"
        );
        ensure!(
            parent_identity
                .image
                .file_name()
                .is_some_and(|name| name == "crosshair-engine.exe"),
            "Unexpected update parent executable"
        );
        let mut parent_file = package::locked_file(&parent_identity.image)?;
        let mut self_file = package::locked_file(&own.image)?;
        ensure!(
            file_hash(&mut parent_file, 128 * 1024 * 1024)? == journal.engine_sha256
                && file_hash(&mut self_file, 128 * 1024 * 1024)? == journal.engine_sha256,
            "The update helper does not match the running engine"
        );
        ensure!(
            file_path(&file)? == job.join("package.msix").canonicalize()?,
            "The inherited update handle refers to another file"
        );
        file.seek(SeekFrom::Start(0))?;
        let (bytes, hash) = manifest::verify(
            &mut file,
            &journal.manifest,
            &manifest::public_key()?,
            || Ok(()),
        )?;
        ensure!(
            hash == journal.sha256 && bytes == journal.bytes,
            "The update changed during handoff"
        );
        let family = package::validate(&job.join("package.msix"), &journal.manifest)?;
        ensure!(family == journal.family, "Update package family mismatch");
        let installed = package::installed()?.context("The installed application is missing")?;
        ensure!(
            installed.family == journal.family
                && installed.version < manifest::mapped_version(&journal.manifest.validate()?),
            "Update target is not newer than the installed package"
        );
        journal.phase = Phase::TakenOver;
        write_journal(&root, &journal)?;
        unsafe {
            SetEvent(raw(&event))?;
        }
        ensure!(
            unsafe { WaitForSingleObject(raw(&parent), 30_000) } == WAIT_OBJECT_0,
            "The application did not close; no installation was attempted"
        );
        wait_for_package_windows(&parent_identity)?;
        journal.phase = Phase::Deploying;
        write_journal(&root, &journal)?;
        deploy(&job.join("package.msix"), &journal.sha256, &own.user_sid)?;
        let installed =
            package::installed()?.context("Windows did not register the updated application")?;
        ensure!(
            installed.version == manifest::mapped_version(&journal.manifest.validate()?)
                && installed.family == journal.family,
            "Windows reported a different installed version after deployment"
        );
        journal.phase = Phase::Installed;
        Ok(())
    })();
    if let Err(error) = &result {
        journal.phase = Phase::Failed;
        journal.error = Some(format!("{error:#}").chars().take(1024).collect());
    }
    write_journal(&root, &journal)?;
    // The new engine must acquire this lock during reconciliation, so release it
    // before activation. The verified package stays locked through deployment.
    drop(file);
    drop(update_lock);
    if unsafe { WaitForSingleObject(raw(&parent), 0) } == WAIT_OBJECT_0 {
        ensure!(
            package::installed()?.is_some_and(|installed| installed.family == journal.family),
            "The registered application is unavailable; reopen it after Windows finishes deployment"
        );
        activate(&journal.family)?;
    }
    result
}

fn wait_for_package_windows(parent: &Identity) -> Result<()> {
    ensure!(
        parent.package.is_some(),
        "The update parent had no package identity"
    );
    unsafe {
        let snapshot = owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        Process32FirstW(raw(&snapshot), &mut entry)?;
        let mut processes = Vec::new();
        loop {
            let end = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            if ["crosshair-editor.exe", "crosshair-gamebar.exe"]
                .iter()
                .any(|allowed| name.eq_ignore_ascii_case(allowed))
                && let Ok(handle) = OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    false,
                    entry.th32ProcessID,
                )
            {
                let handle = owned(handle);
                if let Ok(identity) = Identity::from_handle(raw(&handle))
                    && identity.user_sid == parent.user_sid
                    && identity.logon_sid == parent.logon_sid
                    && identity.session == parent.session
                    && identity.package == parent.package
                    && identity.package_family == parent.package_family
                    && identity
                        .image
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            ["crosshair-editor.exe", "crosshair-gamebar.exe"]
                                .iter()
                                .any(|allowed| name.eq_ignore_ascii_case(allowed))
                        })
                {
                    ensure!(
                        processes.len() < 64,
                        "Too many application windows remain open for a safe update"
                    );
                    processes.push(handle);
                }
            }
            if let Err(error) = Process32NextW(raw(&snapshot), &mut entry) {
                ensure!(
                    error.code() == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0),
                    "Could not finish checking application shutdown: {error}"
                );
                break;
            }
        }
        if !processes.is_empty() {
            let handles: Vec<_> = processes.iter().map(raw).collect();
            ensure!(
                WaitForMultipleObjects(&handles, true, 15_000) == WAIT_OBJECT_0,
                "The editor or Game Bar widget did not close within 15 seconds; the current version was preserved"
            );
        }
        Ok(())
    }
}

fn deploy(path: &Path, hash: &str, user: &str) -> Result<()> {
    let script = deployment_script(path, hash, user)?;
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let arguments = wide(&format!(
        "-NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(encoded)
    ));
    unsafe {
        let system = SHGetKnownFolderPath(&FOLDERID_System, KF_FLAG_DEFAULT, None)?;
        let directory = system.to_string();
        CoTaskMemFree(Some(system.0.cast()));
        let directory = PathBuf::from(directory?);
        let executable =
            package::wide_path(&directory.join("WindowsPowerShell/v1.0/powershell.exe"));
        let directory = package::wide_path(&directory);
        let mut execute = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: windows::core::w!("runas"),
            lpFile: PCWSTR(executable.as_ptr()),
            lpParameters: PCWSTR(arguments.as_ptr()),
            lpDirectory: PCWSTR(directory.as_ptr()),
            nShow: windows::Win32::UI::WindowsAndMessaging::SW_HIDE.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut execute)
            .context("Windows administrator approval was cancelled or unavailable")?;
        ensure!(
            !execute.hProcess.is_invalid(),
            "Windows did not return the deployment process"
        );
        let process = owned(execute.hProcess);
        ensure!(
            WaitForSingleObject(raw(&process), 600_000) == WAIT_OBJECT_0,
            "Windows is still deploying the package; reopen crosshair-rs when installation finishes"
        );
        let mut code = 0;
        GetExitCodeProcess(raw(&process), &mut code)?;
        ensure!(
            code == 0,
            "Windows rejected the unsigned update (0x{code:08X}). Retry from Settings or use the release installer."
        );
        Ok(())
    }
}

fn activate(family: &str) -> Result<()> {
    unsafe {
        let manager: IApplicationActivationManager =
            CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)?;
        let app = wide(&format!("{family}!Engine"));
        manager.ActivateApplication(
            PCWSTR(app.as_ptr()),
            windows::core::w!("--resume-update"),
            AO_NONE,
        )?;
        Ok(())
    }
}

pub fn recover(data_root: &Path) -> Result<Option<Recovery>> {
    let root = data_root.join("updates");
    if !root.exists() {
        return Ok(None);
    }
    let _lock = lock(&root)?;
    if !root.join("current.json").exists() {
        return Ok(None);
    }
    let _apartment = package::Apartment::new()?;
    let journal = read_journal(&root)?;
    let installed = package::installed()?;
    let target = manifest::mapped_version(&journal.manifest.validate()?);
    let succeeded = installed
        .as_ref()
        .is_some_and(|package| package.family == journal.family && package.version >= target);
    Ok(Some(Recovery {
        installed_version: installed.map(|package| manifest::product_version(package.version)),
        restart: journal.restart,
        succeeded,
        error: if succeeded {
            None
        } else {
            Some(journal.error.unwrap_or_else(|| "The update was interrupted. The installed version was preserved; retry from Settings.".into()))
        },
        job: journal.job,
    }))
}

/// Acknowledge only after the engine has applied and durably saved restart
/// intent. A crash before this call keeps that intent available on next launch.
pub fn acknowledge_recovery(data_root: &Path, recovery: &Recovery) -> Result<()> {
    let root = data_root.join("updates");
    let _lock = lock(&root)?;
    ensure!(
        read_journal(&root)?.job == recovery.job,
        "The recovery journal changed"
    );
    fs::remove_file(root.join("current.json"))?;
    clean_job(&root, recovery.job);
    Ok(())
}

pub(super) fn clean_job(root: &Path, job: Id) {
    let directory = root.join(job.to_string());
    if fs::symlink_metadata(&directory).is_ok_and(|metadata| {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 && metadata.is_dir()
    }) {
        for name in ["setup.exe", "package.msix", "crosshair-update.exe"] {
            let _ = fs::remove_file(directory.join(name));
        }
        let _ = fs::remove_dir(directory);
    }
}

pub(super) fn clean_orphans(root: &Path) -> Result<()> {
    let _lock = lock(root)?;
    let keep = if root.join("current.json").exists() {
        Some(read_journal(root)?.job)
    } else {
        None
    };
    // Only our UUID directories and known file names are eligible. Never recurse
    // through links or delete unrelated contents, even in private staging.
    for entry in fs::read_dir(root)?.take(256) {
        let entry = entry?;
        if let Some(job) = entry
            .file_name()
            .to_str()
            .and_then(|name| Id::parse_str(name).ok())
            && Some(job) != keep
        {
            clean_job(root, job);
        }
    }
    Ok(())
}

pub(super) fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0,
        "The update directory cannot be a reparse point"
    );
    let user = Identity::current()?.user_sid;
    let descriptor = wide(&format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;{user})"));
    let path = package::wide_path(path);
    unsafe {
        let mut security = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(descriptor.as_ptr()),
            SDDL_REVISION_1,
            &mut security,
            None,
        )?;
        let mut present = windows::core::BOOL::default();
        let mut defaulted = windows::core::BOOL::default();
        let mut acl = std::ptr::null_mut();
        let result = (|| {
            GetSecurityDescriptorDacl(security, &mut present, &mut acl, &mut defaulted)?;
            SetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl),
                None,
            )
            .ok()?;
            Ok(())
        })();
        LocalFree(Some(HLOCAL(security.0)));
        result
    }
}

fn lock(root: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root.join("update.lock"))
        .context("Another update coordinator is active; wait for installation to finish")
}

fn read_journal(root: &Path) -> Result<Journal> {
    let file = package::locked_file(&root.join("current.json"))?;
    ensure!(
        file.metadata()?.len() <= manifest::MAX_MANIFEST + 4096,
        "Update recovery journal exceeds its limit"
    );
    let mut bytes = Vec::new();
    file.take(manifest::MAX_MANIFEST + 4097)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= manifest::MAX_MANIFEST + 4096,
        "Update recovery journal grew beyond its limit"
    );
    let journal: Journal = serde_json::from_slice(&bytes)?;
    ensure!(
        journal.schema == 1 && journal.bytes > 0 && journal.bytes <= manifest::MAX_PACKAGE,
        "Invalid update recovery journal"
    );
    journal.manifest.validate()?;
    for digest in [&journal.sha256, &journal.engine_sha256] {
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid update recovery hash"
        );
    }
    let publisher_id = journal
        .family
        .strip_prefix(&format!("{}_", manifest::PACKAGE_NAME))
        .context("Invalid update package family")?;
    ensure!(
        publisher_id.len() == 13
            && publisher_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "Invalid update publisher identity"
    );
    Ok(journal)
}

fn write_journal(root: &Path, journal: &Journal) -> Result<()> {
    let path = root.join(format!("journal-{}.tmp", Id::new_v4()));
    let result = (|| {
        let bytes = serde_json::to_vec(journal)?;
        ensure!(
            bytes.len() as u64 <= manifest::MAX_MANIFEST + 4096,
            "Update recovery journal exceeds its limit"
        );
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        let source = package::wide_path(&path);
        let target = package::wide_path(&root.join("current.json"));
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn file_hash(file: &mut File, maximum: u64) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        ensure!(size <= maximum, "Update helper exceeds its size limit");
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn file_path(file: &File) -> Result<PathBuf> {
    let mut buffer = vec![0_u16; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            HANDLE(file.as_raw_handle()),
            &mut buffer,
            FILE_NAME_NORMALIZED,
        )
    } as usize;
    ensure!(
        count > 0 && count < buffer.len(),
        "Could not inspect the inherited update handle"
    );
    Ok(PathBuf::from(String::from_utf16(&buffer[..count])?).canonicalize()?)
}

fn deployment_script(path: &Path, hash: &str, user: &str) -> Result<String> {
    let path = path.canonicalize()?;
    let text = path
        .to_str()
        .context("The update path is not valid Unicode")?;
    let text = text.strip_prefix(r"\\?\").unwrap_or(text);
    ensure!(
        text.as_bytes().get(1) == Some(&b':') && text.as_bytes().get(2) == Some(&b'\\'),
        "Updates must be staged on a local Windows drive"
    );
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "Invalid verified package digest"
    );
    ensure!(
        user.starts_with("S-1-")
            && user.len() <= 184
            && user
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'S' | b'-')),
        "Invalid deployment user"
    );
    Ok(format!(
        "$Package = '{}'\n$ExpectedHash = '{hash}'\n$ExpectedUser = '{user}'\n$Framework = $null\n{}",
        text.replace('\'', "''"),
        include_str!("deploy.ps1")
    ))
}

unsafe fn duplicate(handle: HANDLE, rights: u32) -> Result<OwnedHandle> {
    unsafe {
        let mut copy = HANDLE::default();
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut copy,
            rights,
            true,
            if rights == 0 {
                DUPLICATE_SAME_ACCESS
            } else {
                DUPLICATE_HANDLE_OPTIONS(0)
            },
        )?;
        Ok(owned(copy))
    }
}
struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_failure_requires_a_new_check_before_download() {
        let directory = tempfile::tempdir().unwrap();
        let updates = crate::updates::Updates::new(directory.path().to_owned(), |_| {}).unwrap();
        updates.report_recovery(&Recovery {
            installed_version: Some("0.1.0-alpha.0".into()),
            restart: RestartIntent {
                visible: true,
                reopen_editor: true,
            },
            succeeded: false,
            error: Some("Installation was cancelled".into()),
            job: Id::new_v4(),
        });
        let status = updates.status();
        assert_eq!(status.phase, crosshair_core::protocol::UpdatePhase::Failed);
        assert!(status.version.is_none());
        assert_eq!(status.error.as_deref(), Some("Installation was cancelled"));
        assert!(updates.download().is_err());
    }

    fn journal() -> Journal {
        Journal {
            schema: 1,
            job: Id::new_v4(),
            manifest: crate::updates::tests::release(),
            sha256: "a".repeat(64),
            bytes: 42,
            family: "OhneB.crosshair-rs_123456789abcd".into(),
            engine_sha256: "b".repeat(64),
            restart: RestartIntent {
                visible: false,
                reopen_editor: true,
            },
            phase: Phase::Prepared,
            error: None,
        }
    }

    #[test]
    fn durable_journal_retains_restart_intent_and_install_failure() {
        let directory = tempfile::tempdir().unwrap();
        let mut value = journal();
        write_journal(directory.path(), &value).unwrap();
        let read = read_journal(directory.path()).unwrap();
        assert_eq!(read.job, value.job);
        assert!(!read.restart.visible && read.restart.reopen_editor);
        value.phase = Phase::Failed;
        value.error = Some("fixture deployment denial".into());
        write_journal(directory.path(), &value).unwrap();
        assert_eq!(read_journal(directory.path()).unwrap().error, value.error);
        fs::write(directory.path().join("current.json"), b"{\"schema\":1").unwrap();
        assert!(read_journal(directory.path()).is_err());
        fs::write(
            directory.path().join("current.json"),
            vec![b' '; manifest::MAX_MANIFEST as usize + 4097],
        )
        .unwrap();
        assert!(read_journal(directory.path()).is_err());
    }

    #[test]
    fn startup_cleanup_keeps_the_recovery_job_and_unrelated_files() {
        let directory = tempfile::tempdir().unwrap();
        let value = journal();
        let keep = directory.path().join(value.job.to_string());
        let orphan = Id::new_v4();
        let remove = directory.path().join(orphan.to_string());
        fs::create_dir(&keep).unwrap();
        fs::create_dir(&remove).unwrap();
        fs::write(keep.join("package.msix"), b"fixture").unwrap();
        fs::write(keep.join("setup.exe"), b"fixture").unwrap();
        fs::write(remove.join("package.msix"), b"fixture").unwrap();
        fs::write(remove.join("setup.exe"), b"fixture").unwrap();
        fs::write(directory.path().join("unrelated.txt"), b"keep").unwrap();
        write_journal(directory.path(), &value).unwrap();
        clean_orphans(directory.path()).unwrap();
        assert!(keep.join("package.msix").exists());
        assert!(keep.join("setup.exe").exists());
        assert!(!remove.exists());
        assert!(directory.path().join("unrelated.txt").exists());
    }

    #[test]
    fn deployment_arguments_preserve_unicode_and_never_execute_path_text() {
        use std::os::windows::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        for name in [
            "package #1.msix",
            "package %231.msix",
            "über $(throw 'injected').msix",
        ] {
            let path = directory.path().join(name);
            fs::write(&path, b"fixture").unwrap();
            let script =
                deployment_script(&path, &"a".repeat(64), "S-1-5-21-123-456-789-1001").unwrap();
            // Execute only the argument bindings, never deployment or elevation.
            let bindings = script.split_once("# Embedded").unwrap().0;
            let check = format!(
                "{bindings}\n[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); @($Package,$ExpectedHash,$ExpectedUser) | ConvertTo-Json -Compress"
            );
            let encoded: Vec<u8> = check.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let output = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-EncodedCommand",
                    &STANDARD.encode(encoded),
                ])
                .creation_flags(CREATE_NO_WINDOW.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let values: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                PathBuf::from(&values[0]).canonicalize().unwrap(),
                path.canonicalize().unwrap()
            );
            assert_eq!(values[1], "a".repeat(64));
            assert_eq!(values[2], "S-1-5-21-123-456-789-1001");
            assert!(deployment_script(&path, "not a digest", &values[2]).is_err());
            assert!(deployment_script(&path, &values[1], "S-1-5'; throw 'bad'").is_err());
        }
    }
}
