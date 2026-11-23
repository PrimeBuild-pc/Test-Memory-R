//! Files written whole or not at all: a run's results file (TODO 72), and, since an overclocked PC that crashed mid-write lost its
//! calibrations (forum feedback, 2026-11-23), `tmr-cfg.json` and the window's `window.json`. A save goes to `<file>.tmp`, flushed to the
//! disk, then put in the file's place (`replace`, which keeps the file's permissions), so a crash, or a system that went down, leaves the
//! last save whole, not part of this one, and a reader finds the last save or this one, never no file. The temp is the save's alone until
//! it is in the file's place (`make_temp`), so two saves of one file at once, by two TMRs, take turns.
//!
//! The file is written in place instead, flushed as well, but no longer whole if the system goes down mid-write, when the temp can't be
//! made or the replace is refused (a viewer holding the file without letting it be deleted); never through a link (`in_place`). The two
//! settings files are in a folder of their own, `settings\` (`settings_file`), which an install for all users makes writable by users
//! (`dist/tmr.iss`), so their saves are whole there too; beside tmr.exe, in Program Files, a user's TMR could make no temp and wrote
//! every save in place. In place stays for a home whose `settings\` takes no temp: a share, read-only media, an old install's files
//! beside tmr.exe that couldn't be taken into it. Saves are rare (a setting changed, a calibration saved, Start, the window closing, a
//! cycle's results), and none is in a test's loop.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_SHARING_VIOLATION, GENERIC_WRITE,
    HANDLE, WIN32_ERROR,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, PSECURITY_DESCRIPTOR, SE_DACL_AUTO_INHERIT_REQ, SetKernelObjectSecurity,
    SetSecurityDescriptorControl,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_ID_INFO, FILE_RENAME_INFO, FILE_SHARE_READ, FileIdInfo, FileRenameInfo, FileRenameInfoEx, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetVolumeInformationByHandleW, READ_CONTROL, SECURITY_IDENTIFICATION, SetFileInformationByHandle,
    WRITE_DAC,
};
use windows::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;

/// How a save was written
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// Whole: its temp, put in the file's place
    Replaced,
    /// In place: the temp couldn't be made, or put in the file's place
    InPlace,
}

/// How long a save waits for another save of its file to be done with the temp (`made`), or for the file to be let go of so it can be
/// replaced (`write_by`), before it writes the file in place: a save takes milliseconds, and an antivirus's look at a new file about as
/// long
const WAIT: Duration = Duration::from_secs(1);

/// Writes `bytes` as `path`, whole (the module's doc): to `<path>.tmp`, on the disk, put in `path`'s place (`replace`); in place, also on
/// the disk, when the temp can't be made or the replace is refused, which the log says. A temp a crashed save left is removed first. The
/// error names the file, and the temp when the save is left in it: a refused replace whose file won't be written in place either.
pub fn write(path: &Path, bytes: &[u8]) -> Result<Written, String> {
    write_by(path, bytes, open)
}

/// `path` opened to be written from its start: `new`, the temp (`make_temp`); else the file itself, in place (`in_place`)
fn open(path: &Path, new: bool) -> io::Result<File> {
    if new { make_temp(path) } else { in_place(path) }
}

/// `path` opened to be written in place, from its start, only when the file is the name's own, never through a link (review). In an
/// install for all users `settings\` and `results\` are writable by every user, and TMR may run elevated, so a symbolic link there, or a
/// hard link (another name of a file elsewhere), had a save written over a file the user can't write; and any user can have a save
/// written in place, with a folder where its temp goes, or a program holding the file. Opened as the name holds it, not what a link
/// there names, so a link to no file makes none, and a link, symbolic or hard, refuses the save, the file it names left as it was. A
/// reparse point that names no other file, as OneDrive's files are, is opened through it again, with no right to act as TMR given to
/// what it opens (a named pipe's server), and must be the same file.
fn in_place(path: &Path) -> io::Result<File> {
    let named = OpenOptions::new().write(true).create(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0).open(path)?;
    let (kind, mut info) = (named.metadata()?, BY_HANDLE_FILE_INFORMATION::default());
    // SAFETY: the file's handle, open for the call, and a struct of the size the call fills, which outlives it
    unsafe { GetFileInformationByHandle(handle(&named), &mut info) }.map_err(os_error)?;
    let link = || io::Error::other("it is a link, and a save writes no file through one");
    if kind.file_type().is_symlink() || info.nNumberOfLinks > 1 {
        return Err(link());
    }
    let file = if kind.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
        named
    } else {
        let through = OpenOptions::new().write(true).security_qos_flags(SECURITY_IDENTIFICATION.0).open(path)?;
        if id(&through)? != id(&named)? {
            return Err(link());
        }
        through
    };
    file.set_len(0)?;
    Ok(file)
}

/// The file `file` is: its volume and its ID in it (all 128 bits: ReFS's don't fit `nFileIndex`'s 64)
fn id(file: &File) -> io::Result<FILE_ID_INFO> {
    let mut id = FILE_ID_INFO::default();
    // SAFETY: the file's handle, open for the call, and a struct of the size given, which outlives it
    unsafe { GetFileInformationByHandleEx(handle(file), FileIdInfo, (&raw mut id).cast(), size_of::<FILE_ID_INFO>() as u32) }
        .map_err(os_error)?;
    Ok(id)
}

/// `temp` made new, never one already there (a link among them), and the save's alone until it is in the file's place: others may read
/// it, not write, rename or delete it. So a second save of the file, another TMR's, waits for it (`made`), where it deleted it and made
/// its own, which left the first nothing to put in the file's place and both writing the file in place (review: two TMRs saving at once
/// wrote half their saves in place, and a reader saw the file empty). The temp is put there itself (`replace`), so it has the rights to be
/// renamed and to be given the file's permissions.
fn make_temp(temp: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_WRITE.0 | DELETE.0 | WRITE_DAC.0)
        .share_mode(FILE_SHARE_READ.0)
        .open(temp)
}

/// `bytes` written to `file` and flushed to the disk (`FlushFileBuffers`) before it returns (TODO 72). NTFS logs a rename at once but
/// writes a file's data back later, so without the flush a system that went down just after a save could keep the rename and lose the
/// data it names.
fn flushed(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

/// The save's temp, `temp`, put in `path`'s place with `path`'s permissions (`keep_permissions`), renamed over it (`rename`). Renamed
/// alone, the file would have the temp's, only what its folder hands down: an install for all users gave Users the right to modify
/// `tmr-cfg.json` and `window.json` on the files themselves, beside tmr.exe, where TMR still saves them when they weren't taken into
/// `settings\` (`settings_file`), so a save by a TMR run as administrator would take it away where the folder doesn't hand it down, and
/// no later save by a user's TMR could write them, nor make them again (review of the first version, which renamed). `ReplaceFileW`,
/// which kept them itself, moved the file away before it put the temp there, so a reader in that moment found no file, which a load
/// takes as the defaults, to be saved over it, and it held the file from readers for a moment (review: one TMR saving 200 times, a
/// reader all the while found no file 448 times and the file held 701).
fn replace(temp: &File, path: &Path) -> io::Result<()> {
    keep_permissions(path, temp)?;
    rename(temp, path)
}

/// `path`'s DACL put on `temp`, which is to replace it: the entries set on the file itself and those it inherits from its folder, which
/// the temp, in the same folder, inherits too (`SE_DACL_AUTO_INHERIT_REQ` merges them as for a file made there, so `icacls` reads the
/// two the same). None to keep with no file there yet, or a link, which is replaced, not followed (`rename`), nor on a file system that
/// keeps no permissions (FAT, exFAT). A folder there is refused: no rename would replace it.
fn keep_permissions(path: &Path, temp: &File) -> io::Result<()> {
    let mut flags = 0;
    // SAFETY: the temp's handle, open for the call; only the file system's flags asked for, into a u32 that outlives it
    unsafe { GetVolumeInformationByHandleW(handle(temp), None, None, None, Some(&mut flags), None) }.map_err(os_error)?;
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Ok(());
    }
    let there = OpenOptions::new()
        .access_mode(READ_CONTROL.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path);
    let there = match there {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        there => there?,
    };
    let kind = there.metadata()?.file_type();
    if kind.is_dir() {
        return Err(io::Error::from(io::ErrorKind::IsADirectory));
    }
    if kind.is_symlink() {
        return Ok(());
    }
    let mut needed = 0;
    // SAFETY: the file's handle, open with READ_CONTROL; no buffer, to be told the size, which fails (ERROR_INSUFFICIENT_BUFFER)
    let _ = unsafe { GetKernelObjectSecurity(handle(&there), DACL_SECURITY_INFORMATION.0, None, 0, &mut needed) };
    // u64s: aligned for the descriptor, which is self-relative, all of it in the buffer
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    let descriptor = PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast());
    // SAFETY: `buffer` holds `needed` bytes and lives to the end of the function, past every use of `descriptor`; the temp's handle has
    // WRITE_DAC (`make_temp`)
    unsafe {
        GetKernelObjectSecurity(handle(&there), DACL_SECURITY_INFORMATION.0, Some(descriptor), needed, &mut needed).map_err(os_error)?;
        SetSecurityDescriptorControl(descriptor, SE_DACL_AUTO_INHERIT_REQ, SE_DACL_AUTO_INHERIT_REQ).map_err(os_error)?;
        SetKernelObjectSecurity(handle(temp), DACL_SECURITY_INFORMATION, descriptor).map_err(os_error)
    }
}

/// `temp` renamed to `path`, over what is there, a link included (the link, not the file it names): with POSIX semantics, so the name
/// holds the old file or the new one, never none, and a reader of the old one doesn't stop it; as before them where the file system has
/// none (FAT, a share)
fn rename(temp: &File, path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    const FILE_RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 1;
    const FILE_RENAME_FLAG_POSIX_SEMANTICS: u32 = 2;
    // Absolute: with no root folder given, the name is the whole path
    let name: Vec<u16> = std::path::absolute(path)?.as_os_str().encode_wide().collect();
    let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName) + (name.len() + 1) * 2;
    // u64s: aligned for FILE_RENAME_INFO (it holds a handle), zeroed, so the name ends in a NUL and there is no root folder
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    let renamed = |class, flags| {
        // SAFETY: `buffer` holds the struct and the name with its NUL (`size`) and outlives the call; the name's length is in bytes,
        // without the NUL, as the API takes it; the temp's handle has DELETE (`make_temp`)
        unsafe {
            (*info).Anonymous.Flags = flags;
            (*info).FileNameLength = (name.len() * 2) as u32;
            name.as_ptr().copy_to_nonoverlapping((&raw mut (*info).FileName).cast::<u16>(), name.len());
            SetFileInformationByHandle(handle(temp), class, info.cast(), size as u32)
        }
    };
    let unsupported = [ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_INVALID_FUNCTION];
    match renamed(FileRenameInfoEx, FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS) {
        // FileRenameInfo's ReplaceIfExists is the same field's low byte
        Err(e) if unsupported.iter().any(|code| e.code() == code.to_hresult()) => {
            renamed(FileRenameInfo, FILE_RENAME_FLAG_REPLACE_IF_EXISTS)
        }
        renamed => renamed,
    }
    .map_err(os_error)
}

/// `file`'s handle, for a Win32 call
fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}

/// The Win32 error a `windows` call failed with, as std's: HRESULT_FROM_WIN32 puts it in the low word under facility 7
pub(crate) fn os_error(e: windows::core::Error) -> io::Error {
    let hr = e.code().0 as u32;
    io::Error::from_raw_os_error(if hr & 0xFFFF_0000 == 0x8007_0000 { (hr & 0xFFFF) as i32 } else { hr as i32 })
}

/// Whether `e` is one of `codes`
fn is(e: &io::Error, codes: &[WIN32_ERROR]) -> bool {
    codes.iter().any(|code| e.raw_os_error() == Some(code.0 as i32))
}

/// `<path>.tmp`
fn temp_of(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    PathBuf::from(temp)
}

/// The save's temp, `open(temp, true)`. A new file (TODO 104): in an install for all users results\ is writable by every user, and TMR
/// may run elevated, so the temp is never a file someone else put there, a link among them. One a crashed or failed save left is removed
/// first (a link, not what it names). Another save's, held so it can't be removed, or made since it was, is waited for, up to `WAIT`,
/// as that save puts it in its file's place; one that can't be removed otherwise, like a folder, leaves the file to be written in place.
fn made(temp: &Path, open: &impl Fn(&Path, bool) -> io::Result<File>) -> io::Result<File> {
    let until = Instant::now() + WAIT;
    loop {
        let removed = fs::remove_file(temp).err();
        let made = open(temp, true);
        let since = removed.as_ref().is_none_or(|e| e.kind() == io::ErrorKind::NotFound);
        let held = removed.filter(|e| is(e, &[ERROR_SHARING_VIOLATION]));
        match made {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && (held.is_some() || since) && Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5))
            }
            // The cause, when another save held it past `WAIT`
            Err(e) => return Err(held.unwrap_or(e)),
            made => return made,
        }
    }
}

/// `write`, its files opened by `open` (`open`, or a test's that refuses one)
fn write_by(path: &Path, bytes: &[u8], open: impl Fn(&Path, bool) -> io::Result<File>) -> Result<Written, String> {
    let temp = temp_of(path);
    let (file, temp_file) = (path.display(), temp.display());
    let in_place = || open(path, false).and_then(|mut file| flushed(&mut file, bytes));
    let mut made = match made(&temp, &open) {
        Ok(made) => made,
        Err(e) => {
            log::info!("Couldn't make {temp_file} ({e}): writing {file} in place");
            return in_place()
                .map(|()| Written::InPlace)
                .map_err(|in_place| format!("Couldn't make {temp_file} ({e}) or write {file} in place ({in_place})"));
        }
    };
    // Not in place: a write that failed here (a full disk) would fail there too, after emptying the file. The temp is let go of before
    // it is removed by name, which removes no other save's: one made since is held (`make_temp`).
    if let Err(e) = flushed(&mut made, bytes) {
        drop(made);
        let _ = fs::remove_file(&temp);
        return Err(format!("Couldn't write {temp_file}, so {file} is as it was: {e}"));
    }
    // A file held by a program that doesn't let it be deleted is waited for, up to `WAIT`: an antivirus's look at the last save holds it
    let until = Instant::now() + WAIT;
    let replaced = loop {
        match replace(&made, path) {
            Err(e) if is(&e, &[ERROR_SHARING_VIOLATION, ERROR_ACCESS_DENIED]) && Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5))
            }
            replaced => break replaced,
        }
    };
    if let Err(refused) = replaced {
        log::warn!("Couldn't replace {file} with {temp_file} ({refused}): writing it in place");
        // Held while the file is written: another save waits for this one
        in_place().map_err(|e| format!("Couldn't replace {file} ({refused}) or write it in place ({e}): this save is in {temp_file}"))?;
        drop(made);
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

/// A symbolic link at `link` to `target`; false where this account may make none (no Developer Mode, not run as administrator), and
/// the test does without
#[cfg(test)]
pub(crate) fn symlink(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => true,
        Err(e) if e.raw_os_error() == Some(windows::Win32::Foundation::ERROR_PRIVILEGE_NOT_HELD.0 as i32) => {
            eprintln!("No symbolic link made ({e}): its part of the test is skipped");
            false
        }
        Err(e) => panic!("{}: {e}", link.display()),
    }
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
    /// first version); and the rename that took `ReplaceFileW`'s place (review: it left no file under the name for a moment) keeps them
    /// all, those the file inherits and whether it inherits any. Read as SDDL (`icacls /save`), which names no account in the system's
    /// language: `(A;;0x1301bf;;;BU)` is Users' modify, on the file itself (no `ID`, inherited, flag), and `D:P` a file that inherits
    /// nothing from its folder.
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
        let granted = sddl();
        assert!(granted.contains(USERS_MODIFY), "the installer's grant: {granted}");
        assert_eq!(write(&path, b"{\"saved\": 1}"), Ok(Written::Replaced));
        assert_eq!(sddl(), granted, "kept, with what the file inherits");
        assert_eq!((fs::read(&path).unwrap(), temp_of(&path).exists()), (b"{\"saved\": 1}".to_vec(), false));

        // Its inherited entries made its own, so it inherits nothing
        icacls(&path, &["/inheritance:d"]);
        let own = sddl();
        assert!(own.contains("D:P") && own.contains(USERS_MODIFY), "{own}");
        assert_eq!(write(&path, b"{\"saved\": 2}"), Ok(Written::Replaced));
        assert_eq!(sddl(), own, "still inheriting nothing");
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

    /// Two TMRs saving one file at once, and one saving it while it is read (review: a reader saw it empty, cut short, or not there,
    /// and half the saves were in place): every save is whole, and a reader sees one save or another, whole, never no file, as a load
    /// would take the defaults for it. Two readers: one reads the file, which an antivirus looks at once a save, so it reads it a few dozen
    /// times; one asks only that it is there, thousands of times (`ReplaceFileW` had it not there about one time in 200).
    #[test]
    fn saves_at_once_are_whole_and_the_file_is_never_missing() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = TempDir::new("atomic-at-once");
        let path = PathBuf::from(dir.file("window.json"));
        // One length for every save, so a part of one is seen as such
        let text = |writer: usize, n: usize| {
            format!(
                "{{\"writer\": {writer}, \"save\": {n:03}, \"pad\": \"{}\"}}
",
                "x".repeat(16 << 10)
            )
        };
        assert_eq!(write(&path, text(9, 0).as_bytes()), Ok(Written::Replaced));
        let (done, path, text) = (&AtomicBool::new(false), &path, &text);
        let reader = |whole: bool| {
            move || {
                let (mut reads, mut seen) = (0, Vec::new());
                while !done.load(Ordering::Relaxed) {
                    reads += 1;
                    let read = if whole { fs::read(path) } else { fs::metadata(path).map(|_| text(0, 0).into_bytes()) };
                    match read {
                        Ok(bytes)
                            if bytes.len() == text(0, 0).len()
                                && bytes.ends_with(
                                    b"\"}
",
                                ) => {}
                        Ok(bytes) => seen.push(format!("{} bytes", bytes.len())),
                        Err(e) => seen.push(e.to_string()),
                    }
                }
                (reads, seen)
            }
        };
        let (writes, read, there) = std::thread::scope(|scope| {
            let (read, there) = (scope.spawn(reader(true)), scope.spawn(reader(false)));
            let writers: Vec<_> = (0..2)
                .map(|writer| scope.spawn(move || (0..100).map(|n| write(path, text(writer, n).as_bytes())).collect::<Vec<_>>()))
                .collect();
            let writes: Vec<_> = writers.into_iter().flat_map(|writer| writer.join().unwrap()).collect();
            done.store(true, Ordering::Relaxed);
            (writes, read.join().unwrap(), there.join().unwrap())
        });
        let not_whole: Vec<_> = writes.iter().filter(|written| **written != Ok(Written::Replaced)).collect();
        assert!(not_whole.is_empty(), "{} of 200 saves not whole: {:?}", not_whole.len(), &not_whole[..not_whole.len().min(3)]);
        for (reads, seen) in [read, there] {
            assert!(seen.is_empty(), "of {reads} reads, {} not of a whole file: {:?}", seen.len(), &seen[..seen.len().min(8)]);
        }
        assert!(!temp_of(path).exists(), "no temp left");
    }

    /// A link at the file is replaced by the save, never written through (review: in an install for all users `settings\` and `results\`
    /// are writable by every user, and a link there had a save by TMR run as administrator written over the file it names): a hard link,
    /// another name of a file elsewhere, and a symbolic link, one to no file too, through which a save makes none
    #[test]
    fn a_link_at_the_file_is_replaced_not_written_through() {
        let dir = TempDir::new("atomic-links");
        let victim = PathBuf::from(dir.file("victim.txt"));
        fs::write(&victim, b"not TMR's").unwrap();
        let hard = PathBuf::from(dir.file("tmr-cfg.json"));
        fs::hard_link(&victim, &hard).unwrap();
        assert_eq!(write(&hard, b"{}"), Ok(Written::Replaced));
        assert_eq!((fs::read(&hard).unwrap(), fs::read(&victim).unwrap()), (b"{}".to_vec(), b"not TMR's".to_vec()));

        let symbolic = PathBuf::from(dir.file("window.json"));
        if symlink(&victim, &symbolic) {
            assert_eq!(write(&symbolic, b"{}"), Ok(Written::Replaced));
            assert!(!fs::symlink_metadata(&symbolic).unwrap().file_type().is_symlink(), "the link replaced");
            assert_eq!((fs::read(&symbolic).unwrap(), fs::read(&victim).unwrap()), (b"{}".to_vec(), b"not TMR's".to_vec()));
            let (nowhere, named) = (PathBuf::from(dir.file("result.json")), PathBuf::from(dir.file("named.txt")));
            assert!(symlink(&named, &nowhere));
            assert_eq!(write(&nowhere, b"{}"), Ok(Written::Replaced));
            assert!(!named.exists(), "no file made through the link");
        }
        assert!([&hard, &symbolic].iter().all(|path| !temp_of(path).exists()));
    }

    /// Written in place, a save writes no file through a link (review): where its temp can't be made, as any user can stop it with a
    /// folder of its name, or the file can't be replaced, as a program can hold its own file, a link at the file, hard or symbolic,
    /// refuses the save, the file it names as it was, and a link to no file has none made; a file of its own is written in place
    #[test]
    fn in_place_a_save_writes_no_file_through_a_link() {
        use windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
        let dir = TempDir::new("atomic-links-in-place");
        let victim = PathBuf::from(dir.file("victim.txt"));
        fs::write(&victim, b"not TMR's").unwrap();
        let no_temp = |path: &Path| fs::create_dir(temp_of(path)).unwrap();
        let refused = |path: &Path| {
            let error = write(path, b"{}").unwrap_err();
            assert!(error.contains("a link"), "{error}");
            assert_eq!(fs::read(&victim).unwrap(), b"not TMR's", "{}", path.display());
        };
        let hard = PathBuf::from(dir.file("tmr-cfg.json"));
        fs::hard_link(&victim, &hard).unwrap();
        no_temp(&hard);
        refused(&hard);

        // Held by a program that lets it be read and written, not deleted, so it can't be replaced
        let held = PathBuf::from(dir.file("window.json"));
        fs::hard_link(&victim, &held).unwrap();
        let holder = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0).open(&held).unwrap();
        refused(&held);
        drop(holder);

        let symbolic = PathBuf::from(dir.file("result.json"));
        if symlink(&victim, &symbolic) {
            no_temp(&symbolic);
            refused(&symbolic);
            let (nowhere, named) = (PathBuf::from(dir.file("other.json")), PathBuf::from(dir.file("named.txt")));
            assert!(symlink(&named, &nowhere));
            no_temp(&nowhere);
            assert!(write(&nowhere, b"{}").is_err());
            assert!(!named.exists(), "no file made through the link");
        }

        let own = PathBuf::from(dir.file("own.json"));
        fs::write(&own, b"as it was, and longer than the save").unwrap();
        no_temp(&own);
        assert_eq!(write(&own, b"{\"saved\": 1}"), Ok(Written::InPlace));
        assert_eq!(fs::read(&own).unwrap(), b"{\"saved\": 1}", "written from its start, the rest cut");
    }
}
