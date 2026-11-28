//! Elevated storage. Check the ancestry on a fixed local drive. A custom root may already be administrator-controlled;
//! the default root and managed subfolders require TMR's private descriptor. Window consent is a checked file in private settings.
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    FILE_SHARE_WRITE, READ_CONTROL,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};
use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{GUID, HSTRING, PWSTR};

// Owner Rights gets read-control only. Otherwise the creator of a child could implicitly change its DACL under a filtered token.
const PRIVATE: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;RC;;;OW)";
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
struct Local(HLOCAL);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the Windows security routines allocate this result with LocalAlloc, and this guard owns it.
        unsafe {
            LocalFree(Some(self.0));
        }
    }
}

fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    // SAFETY: a machine known-folder ID, using the current token. The returned string is copied before it is freed.
    unsafe {
        let ptr = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).map_err(|e| e.to_string())?;
        let path = PathBuf::from(OsString::from_wide(ptr.as_wide()));
        CoTaskMemFree(Some(ptr.0.cast()));
        Ok(path)
    }
}

pub(super) fn open(requested: Option<&Path>) -> Result<PathBuf, String> {
    let path = match requested {
        Some(path) => path.to_owned(),
        None => known_folder(&FOLDERID_ProgramFiles)?.join("TMR Admin"),
    };
    validate_location(&path)?;
    let root_access = if requested.is_some() { Access::CustomHome } else { Access::Private };
    directory(&path, root_access, false).and_then(|()| create_children(&path)).map_err(|e| {
        if requested.is_some() {
            format!("{e}. Set TMR_HOME to a fresh administrator-controlled folder or unset it for the default.")
        } else {
            format!(
                "{e}. Close TMR and rename the existing {} folder to preserve its contents. Restart TMR to create a fresh home.",
                path.display()
            )
        }
    })?;
    Ok(path)
}

fn validate_location(path: &Path) -> Result<(), String> {
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(), std::path::Prefix::Disk(_)))
        || !path.is_absolute()
        || path.file_name().is_none()
        || path.components().any(|c| match c {
            std::path::Component::ParentDir => true,
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                name.ends_with(['.', ' ']) || name.contains(':')
            }
            _ => false,
        })
    {
        return Err("An elevated TMR_HOME must be an absolute folder on a fixed local drive, with no '..' components.".into());
    }
    let mut ancestors: Vec<_> = path.ancestors().skip(1).collect();
    ancestors.reverse();
    // SAFETY: the HSTRING holds the absolute drive root with its trailing separator for this call. DRIVE_FIXED is 3 in WinBase.h.
    if unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(&HSTRING::from(ancestors[0].as_os_str())) } != 3 {
        return Err("Administrator storage requires a fixed local volume".into());
    }
    for path in ancestors {
        check(path, Access::Ancestor).map_err(|e| format!("{e}. Choose a home under an administrator-controlled directory."))?;
    }
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn create_test_directory(path: &Path) -> Result<(), String> {
    validate_location(path)?;
    directory(path, Access::Private, true)
}

/// Where an elevated test process makes its throwaway home: `%ProgramFiles%\TMR Tests`, made with TMR's own permissions and left in
/// place, each process's home in it removed at exit. Program Files is administrator-controlled on a standard Windows, as the default
/// home needs; `C:\Windows\Temp` isn't on GitHub's runners (CI, 2026-11-25: every elevated test's home was refused there).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn test_parent() -> Result<PathBuf, String> {
    let parent = known_folder(&FOLDERID_ProgramFiles)?.join("TMR Tests");
    validate_location(&parent)?;
    private_directory(&parent)?;
    Ok(parent)
}

fn create_children(home: &Path) -> Result<(), String> {
    for child in ["settings", "configs", "results", "logs"] {
        private_directory(&home.join(child))?;
    }
    Ok(())
}

pub(super) fn consent(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = open_setting(path) else { return false };
    let mut record = [0; CONSENT.len()];
    file.metadata().is_ok_and(|m| m.len() == record.len() as u64) && file.read_exact(&mut record).is_ok() && record == CONSENT
}

const CONSENT: &[u8] = b"TMR administrator window consent v1\n";

pub(super) fn set_consent(path: &Path, on: bool) -> Result<(), String> {
    if on {
        crate::atomic_file::write(path, CONSENT).map(drop)?;
        return if consent(path) { Ok(()) } else { Err(format!("Could not confirm the permissions of {}", path.display())) };
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't clear {}: {e}", path.display())),
    }
}

fn private_directory(path: &Path) -> Result<(), String> {
    directory(path, Access::Private, false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Ancestor,
    CustomHome,
    Private,
    SettingsFile,
}

fn directory(path: &Path, access: Access, must_be_new: bool) -> Result<(), String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the descriptor allocated from constant SDDL is kept until CreateDirectoryW returns. The attributes are supplied on creation.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(PRIVATE), SDDL_REVISION_1, &mut descriptor, None)
            .map_err(|e| e.to_string())?;
        let _descriptor = Local(HLOCAL(descriptor.0));
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            ..Default::default()
        };
        if let Err(e) = CreateDirectoryW(&HSTRING::from(path.as_os_str()), Some(&attrs))
            && (must_be_new || e.code() != ERROR_ALREADY_EXISTS.to_hresult())
        {
            return Err(format!("Cannot create {}: {e}", path.display()));
        }
    }
    check(path, access)
}

fn check(path: &Path, access: Access) -> Result<(), String> {
    let check = || -> Result<(), String> {
        let file = OpenOptions::new()
            .access_mode(READ_CONTROL.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)
            .map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err("expected a plain directory".into());
        }
        if path.parent().is_none() {
            use windows::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_NONE};
            let mut relative = [0u16; 2];
            // SAFETY: the open directory handle and output buffer live through the call. Relative to the volume, a real drive root is "\".
            let len = unsafe { GetFinalPathNameByHandleW(HANDLE(file.as_raw_handle()), &mut relative, VOLUME_NAME_NONE) };
            if len != 1 || relative[0] != u16::from(b'\\') {
                return Err("expected the actual volume root; use the full drive path".into());
            }
        }
        permissions(&file, access)?;
        Ok(())
    };
    check().map_err(|e| format!("Cannot use administrator storage at {}: {e}", path.display()))
}

fn open_setting(path: &Path) -> std::io::Result<std::fs::File> {
    use std::io::Error;

    use windows::Win32::Foundation::GENERIC_READ;
    let file = OpenOptions::new().access_mode(GENERIC_READ.0 | READ_CONTROL.0).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0).open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Error::other("expected a plain settings file"));
    }
    permissions(&file, Access::SettingsFile).map_err(Error::other)?;
    Ok(file)
}

pub(super) fn read_setting(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    open_setting(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn permissions(file: &std::fs::File, access: Access) -> Result<(), String> {
    let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let (mut descriptor, mut owner, mut acl) = (PSECURITY_DESCRIPTOR::default(), PSID::default(), std::ptr::null_mut::<ACL>());
    // SAFETY: GetSecurityInfo returns the owner and ACL inside the LocalAlloc buffer kept alive through all inspections below.
    unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            info,
            Some(&mut owner),
            None,
            Some(&mut acl),
            None,
            Some(&mut descriptor),
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let _descriptor = Local(HLOCAL(descriptor.0));
        if owner.0.is_null() || acl.is_null() {
            return Err("missing owner or permissions".into());
        }
        if access == Access::Private {
            let mut string = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, SDDL_REVISION_1, info, &mut string, None)
                .map_err(|e| e.to_string())?;
            let _string = Local(HLOCAL(string.0.cast()));
            let sddl = string.to_string().map_err(|e| e.to_string())?;
            if sddl.replace("D:PAI(", "D:P(") != PRIVATE {
                return Err("expected TMR's administrator-only permissions".into());
            }
        } else {
            let mut owner_limited = false;
            let trusted_owner = trusted(owner)?;
            if access != Access::SettingsFile && !trusted_owner {
                return Err("directory owner can change its permissions".into());
            }
            for i in 0..(*acl).AceCount {
                let mut entry = std::ptr::null_mut();
                GetAce(acl, i.into(), &mut entry).map_err(|e| e.to_string())?;
                let header = &*entry.cast::<ACE_HEADER>();
                if u32::from(header.AceFlags) & INHERIT_ONLY_ACE.0 != 0 || u32::from(header.AceType) == ACCESS_DENIED_ACE_TYPE {
                    continue;
                }
                if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
                    return Err("unsupported permission entry on parent".into());
                }
                let allow = &*entry.cast::<ACCESS_ALLOWED_ACE>();
                // List/read/traverse and creating a sibling do not allow changing or replacing an existing child. Each child is checked.
                const SAFE: u32 = 0x0012_00af; // READ_CONTROL, SYNCHRONIZE, read attributes/EA/data, execute, add file/subdirectory
                let principal = sid_string(PSID((&raw const allow.SidStart).cast_mut().cast()))?;
                owner_limited |= principal == "S-1-3-4";
                let safe = if access == Access::Ancestor { SAFE } else { SAFE & !6 };
                if allow.Mask & !safe != 0 && !trusted_name(&principal) {
                    return Err("a non-administrator can modify this location".into());
                }
            }
            if access == Access::SettingsFile && !trusted_owner && !owner_limited {
                return Err("untrusted file owner can change its permissions".into());
            }
        }
    }
    Ok(())
}

fn trusted_name(sid: &str) -> bool {
    matches!(sid, "S-1-5-18" | "S-1-5-32-544" | TRUSTED_INSTALLER)
}
fn trusted(sid: PSID) -> Result<bool, String> {
    Ok(trusted_name(&sid_string(sid)?))
}

fn sid_string(sid: PSID) -> Result<String, String> {
    let mut string = PWSTR::null();
    // SAFETY: the SID comes from the descriptor held by check. The allocated string is valid until its guard drops.
    unsafe {
        ConvertSidToStringSidW(sid, &mut string).map_err(|e| e.to_string())?;
        let _string = Local(HLOCAL(string.0.cast()));
        string.to_string().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_override_requires_an_absolute_local_folder() {
        for path in [r".", r"D:relative", r"D:\", r"D:\storage\..\tmr", r"\\server\share\data"] {
            assert!(open(Some(Path::new(path))).unwrap_err().contains("absolute folder"), "{path}");
        }
    }

    #[test]
    fn protected_override_directories_can_be_reopened() {
        if !crate::memory::privileges::is_elevated() {
            return;
        }
        let home = crate::home::dir().join("alternate");
        for _ in 0..2 {
            assert_eq!(open(Some(&home)).unwrap(), home);
            for child in ["settings", "configs", "results", "logs"] {
                check(&home.join(child), Access::Private).unwrap();
            }
        }
    }
    #[test]
    fn damaged_admin_settings_are_reported_and_preserved_in_place() {
        use std::io::Write;

        use crate::app_config::AppConfig;
        use crate::{home, settings_file};
        if !crate::memory::privileges::is_elevated() {
            return;
        }
        assert!(home::dir().starts_with(test_parent().unwrap()));
        let p = home::dir().join("settings").join(format!("test-corrupt-{}.json", std::process::id()));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&p).unwrap();
        file.write_all(b"invalid JSON").unwrap();
        drop(file);
        assert!(AppConfig::load_for_setup(&p).1.is_some());
        assert!(AppConfig::default().save(&p).is_err());
        assert!(settings_file::keep_aside(&p).is_err());
        assert!(!p.with_added_extension("unreadable").exists());
        assert_eq!(read_setting(&p).unwrap(), b"invalid JSON");
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn consent_requires_a_protected_file_and_revocation_survives_a_settings_save() {
        if !crate::memory::privileges::is_elevated() {
            return;
        }
        let settings = crate::home::dir().join("consent-test");
        private_directory(&settings).unwrap();
        private_directory(&settings).unwrap();
        let switch = settings.join("allow-window");
        let prefs = settings.join("window.json");
        assert!(!consent(&switch));
        set_consent(&switch, true).unwrap();
        assert!(consent(&switch));
        set_consent(&switch, false).unwrap();
        crate::settings_file::save(&prefs, b"{}").unwrap();
        assert_eq!(read_setting(&prefs).unwrap(), b"{}");
        assert!(!consent(&switch));
        // A directory with precisely the administrator-home descriptor is still just a directory.
        private_directory(&switch).unwrap();
        assert!(!consent(&switch));
        assert!(set_consent(&switch, true).is_err());
    }
}
