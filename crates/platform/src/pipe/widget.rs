use crate::identity::{Identity, owned, raw, sid_string};
use anyhow::{Context, Result, ensure};
use windows::Win32::{
    Foundation::{HLOCAL, LocalFree, WIN32_ERROR},
    NetworkManagement::WindowsFirewall::{
        NetworkIsolationEnumAppContainers, NetworkIsolationFreeAppContainers,
    },
    Security::{
        ACL,
        Authorization::{
            EXPLICIT_ACCESS_W, GRANT_ACCESS, GetSecurityInfo, SE_KERNEL_OBJECT, SetEntriesInAclW,
            SetSecurityInfo, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
        },
        DACL_SECURITY_INFORMATION, NO_INHERITANCE, PSECURITY_DESCRIPTOR, PSID, TOKEN_ACCESS_MASK,
        TOKEN_QUERY,
    },
    Storage::FileSystem::{READ_CONTROL, WRITE_DAC},
    System::Threading::{GetCurrentProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
};

pub(super) fn prepare_access(identity: &Identity) -> Result<String> {
    let family = identity
        .package_family
        .as_deref()
        .context("Game Bar requires the installed package")?;
    // Deriving our own family SID inside a packaged desktop process can succeed with a null SID.
    let mut count = 0;
    let mut entries = std::ptr::null_mut();
    unsafe {
        WIN32_ERROR(NetworkIsolationEnumAppContainers(
            0,
            &mut count,
            &mut entries,
        ))
        .ok()?;
        let result = (|| {
            ensure!(
                !entries.is_null(),
                "Windows has no registered AppContainers"
            );
            let container = std::slice::from_raw_parts(entries, count as usize)
                .iter()
                .find(|entry| {
                    !entry.appContainerName.is_null()
                        && entry
                            .appContainerName
                            .to_string()
                            .is_ok_and(|name| name.eq_ignore_ascii_case(family))
                })
                .context("Windows has no AppContainer registered for the Game Bar widget")?;
            let sid = PSID(container.appContainerSid.cast());
            ensure!(
                !sid.is_invalid(),
                "The registered Game Bar widget has no SID"
            );
            let value = sid_string(sid)?;
            allow_widget_queries(sid)?;
            Ok(value)
        })();
        if !entries.is_null() {
            NetworkIsolationFreeAppContainers(entries);
        }
        result
    }
}

fn allow_widget_queries(sid: PSID) -> Result<()> {
    unsafe {
        let grant = |handle, access| -> Result<()> {
            let mut old = std::ptr::null_mut::<ACL>();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut old),
                None,
                Some(&mut descriptor),
            )
            .ok()?;
            let entry = EXPLICIT_ACCESS_W {
                grfAccessPermissions: access,
                grfAccessMode: GRANT_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: TRUSTEE_W {
                    TrusteeForm: TRUSTEE_IS_SID,
                    TrusteeType: TRUSTEE_IS_UNKNOWN,
                    ptstrName: windows::core::PWSTR(sid.0.cast()),
                    ..Default::default()
                },
            };
            let mut updated = std::ptr::null_mut();
            let merged = SetEntriesInAclW(Some(&[entry]), Some(old), &mut updated).ok();
            let result = merged.and_then(|_| {
                SetSecurityInfo(
                    handle,
                    SE_KERNEL_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    Some(updated),
                    None,
                )
                .ok()
            });
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            if !updated.is_null() {
                let _ = LocalFree(Some(HLOCAL(updated.cast())));
            }
            result.map_err(Into::into)
        };
        grant(GetCurrentProcess(), PROCESS_QUERY_LIMITED_INFORMATION.0)?;
        let mut token = Default::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ACCESS_MASK(TOKEN_QUERY.0 | WRITE_DAC.0 | READ_CONTROL.0),
            &mut token,
        )?;
        let token = owned(token);
        grant(raw(&token), TOKEN_QUERY.0)
    }
}
