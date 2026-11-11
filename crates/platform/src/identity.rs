use std::{
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::PathBuf,
};

use anyhow::{Context, Result, ensure};
use windows::{
    Win32::{
        Foundation::{
            APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, LocalFree,
        },
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, PSID, TOKEN_GROUPS,
            TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER, TokenLogonSid, TokenUser,
        },
        Storage::Packaging::Appx::{GetPackageFamilyName, GetPackageFullName},
        System::{
            RemoteDesktop::ProcessIdToSessionId,
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, GetProcessId, OpenProcess,
                OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
    },
    core::{PCWSTR, PWSTR},
};

#[derive(Clone, Debug)]
pub struct Identity {
    pub user_sid: String,
    pub logon_sid: String,
    pub session: u32,
    pub package: Option<String>,
    pub package_family: Option<String>,
    pub image: PathBuf,
}

impl Identity {
    pub fn from_handle(process: HANDLE) -> Result<Self> {
        let pid = unsafe { GetProcessId(process) };
        ensure!(pid != 0, "Process handle is unavailable");
        unsafe { Self::from_process(process, pid) }
    }

    pub fn current() -> Result<Self> {
        unsafe { Self::from_process(GetCurrentProcess(), GetCurrentProcessId()) }
    }

    pub fn process(pid: u32) -> Result<Self> {
        unsafe {
            let process = owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?);
            Self::from_process(raw(&process), pid)
        }
    }

    unsafe fn from_process(process: HANDLE, pid: u32) -> Result<Self> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
            let token = owned(token);
            let user = token_information(raw(&token), TokenUser)?;
            let user = &*user.as_ptr().cast::<TOKEN_USER>();
            let user_sid = sid_string(user.User.Sid)?;
            let groups = token_information(raw(&token), TokenLogonSid)?;
            let groups = &*groups.as_ptr().cast::<TOKEN_GROUPS>();
            ensure!(groups.GroupCount == 1, "Process has no unique logon SID");
            let logon_sid = sid_string(groups.Groups[0].Sid)?;
            let mut session = 0;
            ProcessIdToSessionId(pid, &mut session)?;
            let mut path = vec![0; 32_768];
            let mut length = path.len() as u32;
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )?;
            let image =
                PathBuf::from(String::from_utf16(&path[..length as usize])?).canonicalize()?;
            let package = package_name(process, false)?;
            let package_family = package_name(process, true)?;
            Ok(Self {
                user_sid,
                logon_sid,
                session,
                package,
                package_family,
                image,
            })
        }
    }

    pub fn verify_peer(&self, peer: &Self, allowed_files: &[&str]) -> Result<()> {
        ensure!(
            peer.user_sid == self.user_sid
                && peer.logon_sid == self.logon_sid
                && peer.session == self.session,
            "IPC peer belongs to another user or logon session"
        );
        ensure!(
            peer.package == self.package,
            "IPC peer package version does not match"
        );
        let name = peer
            .image
            .file_name()
            .and_then(|name| name.to_str())
            .context("IPC peer has no executable name")?;
        ensure!(
            allowed_files
                .iter()
                .any(|allowed| name.eq_ignore_ascii_case(allowed)),
            "Unexpected IPC executable"
        );
        ensure!(
            peer.image.parent() == self.image.parent(),
            "IPC executable is outside the application directory"
        );
        Ok(())
    }
}

pub fn control_pipe_name(scope: &str) -> Result<String> {
    ensure!(
        !scope.is_empty()
            && scope.len() <= 64
            && scope
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "Invalid IPC scope"
    );
    let mut session = 0;
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut session)?;
    }
    Ok(format!(
        r"\\.\pipe\LOCAL\crosshair-rs.control.v1.{session}.{scope}"
    ))
}

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// # Safety
/// Transfer one valid owned Windows handle, never a pseudo handle or a borrowed handle.
pub unsafe fn owned(handle: HANDLE) -> OwnedHandle {
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

unsafe fn token_information(token: HANDLE, kind: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    unsafe {
        let mut bytes = 0;
        let _ = GetTokenInformation(token, kind, None, 0, &mut bytes);
        ensure!(
            bytes > 0 && bytes <= 64 * 1024,
            "Invalid process token information size"
        );
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        GetTokenInformation(
            token,
            kind,
            Some(buffer.as_mut_ptr().cast()),
            bytes,
            &mut bytes,
        )?;
        Ok(buffer)
    }
}

pub(crate) unsafe fn sid_string(sid: PSID) -> Result<String> {
    unsafe {
        let mut value = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut value)?;
        let result = value.to_string();
        LocalFree(Some(HLOCAL(value.0.cast())));
        Ok(result?)
    }
}

unsafe fn package_name(process: HANDLE, family: bool) -> Result<Option<String>> {
    unsafe {
        let mut length = 0;
        let result = if family {
            GetPackageFamilyName(process, &mut length, None)
        } else {
            GetPackageFullName(process, &mut length, None)
        };
        if result == APPMODEL_ERROR_NO_PACKAGE {
            return Ok(None);
        }
        ensure!(
            result == ERROR_INSUFFICIENT_BUFFER && (1..=1024).contains(&length),
            "Could not inspect package identity"
        );
        let mut buffer = vec![0; length as usize];
        let result = if family {
            GetPackageFamilyName(process, &mut length, Some(PWSTR(buffer.as_mut_ptr())))
        } else {
            GetPackageFullName(process, &mut length, Some(PWSTR(buffer.as_mut_ptr())))
        };
        result.ok()?;
        Ok(Some(PCWSTR(buffer.as_ptr()).to_string()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_other_session_package_and_executable() -> Result<()> {
        let own = Identity::current()?;
        let name = own.image.file_name().unwrap().to_str().unwrap();
        own.verify_peer(&own, &[name])?;
        let mut peer = own.clone();
        peer.session += 1;
        assert!(own.verify_peer(&peer, &[name]).is_err());
        peer = own.clone();
        peer.package = Some("different_package".into());
        assert!(own.verify_peer(&peer, &[name]).is_err());
        assert!(own.verify_peer(&own, &["another.exe"]).is_err());
        assert!(control_pipe_name("../escape").is_err());
        Ok(())
    }
}
