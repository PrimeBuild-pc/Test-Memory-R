//! Files written whole or not at all: a run's results file (TODO 72), and, since an overclocked PC that crashed mid-write lost its
//! calibrations (forum feedback, 2026-11-23), `tmr-cfg.json` and the window's `window.json`. A save goes to `<file>.tmp`, flushed to the
//! disk, then put in the file's place (`replace`, which keeps the file's permissions), so a crash, or a system that went down, leaves the
//! last save whole, not part of this one.
//!
//! The file is written in place instead, flushed as well, but no longer whole if the system goes down mid-write, when the temp can't be
//! made or the replace is refused (a viewer holding the file without letting it be deleted). The two settings files are in a folder of
//! their own, `settings\` (`settings_file`), which an install for all users makes writable by users (`dist/tmr.iss`), so their saves
//! are whole there too; beside tmr.exe, in Program Files, a user's TMR could make no temp and wrote every save in place. In place stays
//! for a home whose `settings\` takes no temp: a share, read-only media, an old install's files beside tmr.exe that couldn't be taken
//! into it. Saves are rare (a setting changed, a calibration saved, Start, the window closing, a cycle's results), and none is in a
//! test's loop.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// How a save was written
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// Whole: its temp, put in the file's place
    Replaced,
    /// In place: the temp couldn't be made, or put in the file's place
    InPlace,
}

/// Writes `bytes` as `path`, whole (the module's doc): to `<path>.tmp`, on the disk, put in `path`'s place (`replace`); in place, also on
/// the disk, when the temp can't be made or the replace is refused, which the log says. A temp a crashed save left is removed first. The
/// error names the file, and the temp when the save is left in it: a refused replace whose file won't be written in place either.
pub fn write(path: &Path, bytes: &[u8]) -> Result<Written, String> {
    write_by(path, bytes, open)
}

/// `path` opened to be written from its start: `new`, only as a new file, never one already there (the temp)
fn open(path: &Path, new: bool) -> io::Result<File> {
    OpenOptions::new().write(true).create(true).truncate(true).create_new(new).open(path)
}

/// `bytes` written to `file` and flushed to the disk (`FlushFileBuffers`) before it returns (TODO 72). NTFS logs a rename at once but
/// writes a file's data back later, so without the flush a system that went down just after a save could keep the rename and lose the
/// data it names. The file is closed when it returns, so it can be put in the file's place.
fn flushed(file: io::Result<File>, bytes: &[u8]) -> io::Result<()> {
    let mut file = file?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// `temp` put in `path`'s place, keeping `path`'s permissions (`ReplaceFileW`, which keeps the replaced file's DACL, attributes and
/// creation time): a rename would leave the file with the temp's, only what its folder hands down. An install for all users gave Users
/// the right to modify `tmr-cfg.json` and `window.json` on the files themselves, beside tmr.exe, and its upgrade moves them into
/// `settings\` with that right (`dist/tmr.iss`), so a save by a TMR run as administrator that renamed would take it away where the
/// folder doesn't hand it down, and no later save by a user's TMR could write them, nor make them again (review of the first version,
/// which renamed). With no file there yet, a rename: there are no permissions to keep.
fn replace(temp: &Path, path: &Path) -> io::Result<()> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::Storage::FileSystem::{REPLACE_FILE_FLAGS, ReplaceFileW};
    use windows::core::{HSTRING, PCWSTR};
    let (replaced, replacement) = (HSTRING::from(path), HSTRING::from(temp));
    // SAFETY: both names are NUL-terminated UTF-16 that live to the end of the call (the HSTRINGs above); no backup file (a null
    // name), no flags, and the two reserved pointers null, as the API requires
    let replaced = unsafe { ReplaceFileW(&replaced, &replacement, PCWSTR::null(), REPLACE_FILE_FLAGS(0), None, None) };
    match replaced {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => fs::rename(temp, path),
        // HRESULT_FROM_WIN32 puts the Win32 code in the low word under facility 7
        Err(e) => {
            let hr = e.code().0 as u32;
            Err(io::Error::from_raw_os_error(if hr & 0xFFFF_0000 == 0x8007_0000 { (hr & 0xFFFF) as i32 } else { hr as i32 }))
        }
    }
}

/// `<path>.tmp`
fn temp_of(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    PathBuf::from(temp)
}

/// `write`, its files opened by `open` (`open`, or a test's that refuses one)
fn write_by(path: &Path, bytes: &[u8], open: impl Fn(&Path, bool) -> io::Result<File>) -> Result<Written, String> {
    let temp = temp_of(path);
    let (file, temp_file) = (path.display(), temp.display());
    // A new file (TODO 104): in an install for all users results\ is writable by every user, and TMR may run elevated, so the temp is
    // never a file someone else put there, a link among them. One a crashed or failed save left is removed first (a link, not what it
    // names); one that can't be removed, like a folder that takes no new file, leaves the file to be written in place.
    let _ = fs::remove_file(&temp);
    let made = match open(&temp, true) {
        Ok(made) => made,
        Err(e) => {
            log::info!("Couldn't make {temp_file} ({e}): writing {file} in place");
            return flushed(open(path, false), bytes)
                .map(|()| Written::InPlace)
                .map_err(|in_place| format!("Couldn't make {temp_file} ({e}) or write {file} in place ({in_place})"));
        }
    };
    // Not in place: a write that failed here (a full disk) would fail there too, after emptying the file
    if let Err(e) = flushed(Ok(made), bytes) {
        let _ = fs::remove_file(&temp);
        return Err(format!("Couldn't write {temp_file}, so {file} is as it was: {e}"));
    }
    if let Err(refused) = replace(&temp, path) {
        log::warn!("Couldn't replace {file} with {temp_file} ({refused}): writing it in place");
        flushed(open(path, false), bytes)
            .map_err(|e| format!("Couldn't replace {file} ({refused}) or write it in place ({e}): this save is in {temp_file}"))?;
        let _ = fs::remove_file(&temp);
        return Ok(Written::InPlace);
    }
    Ok(Written::Replaced)
}

/// A folder that takes no new file for as long as it lives, as Program Files was to a user where an install for all users made
/// `tmr-cfg.json` and `window.json` beside tmr.exe writable by users, and as a share or a `settings\` made without users' right to add a
/// file may be: Everyone is denied adding a file to it (`icacls`' WD, a folder's FILE_ADD_FILE), the folder alone, so what it holds can
/// still be written, and deleted.
#[cfg(test)]
pub(crate) struct NoNewFiles<'a>(&'a Path);

#[cfg(test)]
impl<'a> NoNewFiles<'a> {
    pub(crate) fn on(folder: &'a Path) -> Self {
        icacls(folder, &["/deny", "*S-1-1-0:(WD)"]);
        assert!(fs::write(folder.join("probe"), b"").is_err(), "{} still takes a new file", folder.display());
        NoNewFiles(folder)
    }
}

#[cfg(test)]
impl Drop for NoNewFiles<'_> {
    fn drop(&mut self) {
        icacls(self.0, &["/remove:d", "*S-1-1-0"]);
    }
}

/// `icacls <target> <args>`, which must succeed
#[cfg(test)]
fn icacls(target: &Path, args: &[&str]) {
    let done = std::process::Command::new("icacls").arg(target).args(args).output().expect("icacls");
    let said = [&done.stdout, &done.stderr].map(|said| String::from_utf8_lossy(said).trim().to_string()).join(" ");
    assert!(done.status.success(), "icacls {} {args:?}: {said}", target.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::TempDir;

    /// A save replaces the file, or makes it, whole, and leaves no temp; a temp a crashed save left doesn't stop it
    #[test]
    fn a_save_replaces_the_file_and_leaves_no_temp() {
        let dir = TempDir::new("atomic-replace");
        let path = PathBuf::from(dir.file("tmr-cfg.json"));
        let temp = temp_of(&path);
        assert_eq!(temp, PathBuf::from(dir.file("tmr-cfg.json.tmp")));
        assert_eq!(write(&path, b"first"), Ok(Written::Replaced), "a new file");
        assert_eq!(write(&path, b"second"), Ok(Written::Replaced));
        assert_eq!((fs::read(&path).unwrap(), temp.exists()), (b"second".to_vec(), false));

        fs::write(&temp, b"half of a save, from before a crash").unwrap();
        assert_eq!(write(&path, b"third"), Ok(Written::Replaced), "past the stale temp");
        assert_eq!((fs::read(&path).unwrap(), temp.exists()), (b"third".to_vec(), false));
    }

    /// A temp that can't be made, the save writes the file in place, flushed; when it can't be written in place either, the error names
    /// both and the file is as it was. A temp that can't be written leaves the file as it was, and no temp.
    #[test]
    fn a_temp_that_cant_be_made_writes_the_file_in_place() {
        let dir = TempDir::new("atomic-in-place");
        let path = PathBuf::from(dir.file("window.json"));
        let temp = temp_of(&path);
        fs::write(&path, b"as it was").unwrap();
        let no_temp = |at: &Path, new: bool| if new { Err(io::Error::from(io::ErrorKind::PermissionDenied)) } else { open(at, new) };
        assert_eq!(write_by(&path, b"in place", no_temp), Ok(Written::InPlace));
        assert_eq!((fs::read(&path).unwrap(), temp.exists()), (b"in place".to_vec(), false));

        let neither = |_: &Path, _: bool| Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let error = write_by(&path, b"lost", neither).unwrap_err();
        assert!(error.contains(&*path.to_string_lossy()) && error.contains(&*temp.to_string_lossy()), "{error}");
        assert_eq!(fs::read(&path).unwrap(), b"in place");

        // A handle that can't be written, as a full disk refuses one
        let unwritable = |at: &Path, new: bool| if new { File::open(&path) } else { open(at, new) };
        let error = write_by(&path, b"lost", unwritable).unwrap_err();
        assert!(error.contains(&*temp.to_string_lossy()) && error.contains("as it was"), "{error}");
        assert_eq!((fs::read(&path).unwrap(), temp.exists()), (b"in place".to_vec(), false));
    }

    /// A save keeps the file's own permissions: an install for all users gave Users the right to modify `tmr-cfg.json` and `window.json`
    /// on the files themselves, which its upgrade moves into `settings\` with them (`dist/tmr.iss`), and a save that renamed its temp
    /// over the file left only what the folder hands down, so one by a TMR run as administrator took that right away (review of the
    /// first version). Read as SDDL (`icacls /save`), which names
    /// no account in the system's language: `(A;;0x1301bf;;;BU)` is Users' modify, on the file itself (no `ID`, inherited, flag).
    #[test]
    fn a_save_keeps_the_files_own_permissions() {
        let dir = TempDir::new("atomic-permissions");
        let path = PathBuf::from(dir.file("tmr-cfg.json"));
        let sddl = || {
            let saved = PathBuf::from(dir.file("acl.txt"));
            icacls(&path, &["/save", saved.to_str().expect("a UTF-8 temp path")]);
            let bytes = fs::read(&saved).unwrap();
            String::from_utf16_lossy(&bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect::<Vec<_>>())
        };
        const USERS_MODIFY: &str = "(A;;0x1301bf;;;BU)";
        fs::write(&path, b"{}").unwrap();
        icacls(&path, &["/grant", "*S-1-5-32-545:(M)"]);
        assert!(sddl().contains(USERS_MODIFY), "the installer's grant: {}", sddl());
        assert_eq!(write(&path, b"{\"saved\": 1}"), Ok(Written::Replaced));
        assert!(sddl().contains(USERS_MODIFY), "kept: {}", sddl());
        assert_eq!((fs::read(&path).unwrap(), temp_of(&path).exists()), (b"{\"saved\": 1}".to_vec(), false));
    }

    /// In a folder that takes no new file, as Program Files was to a user's TMR beside tmr.exe in an install for all users, a save writes
    /// the file in place
    #[test]
    fn a_folder_that_takes_no_new_file_has_its_file_written_in_place() {
        let dir = TempDir::new("atomic-no-new-files");
        let folder = PathBuf::from(dir.file("TMR"));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("window.json");
        fs::write(&path, b"{}").unwrap();
        let _locked = NoNewFiles::on(&folder);
        assert_eq!(write(&path, b"{\"chart\": false}"), Ok(Written::InPlace));
        assert_eq!((fs::read(&path).unwrap(), temp_of(&path).exists()), (b"{\"chart\": false}".to_vec(), false));
    }
}
