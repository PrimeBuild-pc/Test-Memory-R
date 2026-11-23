//! TMR's settings files (the owner, 2026-11-23): `tmr-cfg.json`, the engine's record of the machine and its calibrations
//! (`app_config.rs`), and the window's `window.json` (tmr-gui's `settings.rs`), in the `settings\` folder of TMR's home (`home.rs`). A
//! folder of their own, so a save is written whole (`atomic_file`) in every install: for all users, the installer makes `settings\`
//! writable by users, as it makes `logs\` (`dist/tmr.iss`), where the files beside tmr.exe could only be written in place, Program Files
//! taking no new file from a user, a save's temp among them.
//!
//! Each save is followed by its last good copy, `<file>.bak`, written whole as well (`save`). A load whose file is there but unreadable,
//! or not a settings file, takes the copy when it reads, and keeps the bad file as `<file>.unreadable` (`keep_aside`); one whose file
//! isn't there takes the defaults, as a file deleted to start again asks. The copy holds the version just written, not the one it
//! replaced: it is read only in place of a damaged file, and then what was lost is the last save, which the version it replaced is a
//! save behind (a calibration measured and saved, then its file damaged by a crash, would be lost with it). Written after the file, so
//! a crash mid-save, where a save is in place, damages one of the two at most.
//!
//! A file still in the old place, the home itself (an unzipped exe of an earlier version, a copy by hand), is taken into `settings\`
//! the first time TMR loads or saves it, when `settings\` has none, and left where it was (`path`).

use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic_file::{self, Written};

/// The folder in TMR's home that holds the settings files
pub const FOLDER: &str = "settings";

/// The settings file `name` (`tmr-cfg.json`, `window.json`): in the home's `settings\`, a file still in the old place taken first
/// (`in_home`)
pub fn path(name: &str) -> PathBuf {
    in_home(crate::home::dir(), name)
}

/// `home`'s settings file `name`, in its `settings\`. When that has none and the home itself has one, from before `settings\`, it is
/// taken first: read, and saved into `settings\` whole, the old one left where it was, as it may be no file TMR can delete (`take`).
/// One that can't be taken is the old one, so nothing of it is lost: one that can't be read is said by its load and isn't saved over,
/// and in a home where `settings\` can't be made or takes no file (read-only media, an old install's folder) it is read, and saved,
/// where it is.
fn in_home(home: &Path, name: &str) -> PathBuf {
    let (path, old) = (home.join(FOLDER).join(name), home.join(name));
    // A link is a file there, for this as for a save: none is followed
    let there = |path: &Path| fs::symlink_metadata(path).is_ok();
    if there(&path) || !there(&old) {
        return path;
    }
    match take(&old, &path) {
        Ok(()) => {
            log::info!("{} taken into {}; the old one is left where it was", old.display(), path.display());
            path
        }
        Err(e) => {
            log::warn!("{} isn't taken into {} ({e}): it is read and saved where it is", old.display(), path.display());
            old
        }
    }
}

/// `old`'s bytes saved as `path`, whole, as they are: whether they are a settings file is its load's to say
fn take(old: &Path, path: &Path) -> Result<(), String> {
    let bytes = fs::read(old).map_err(|e| format!("couldn't read it: {e}"))?;
    make_folder(path)?;
    atomic_file::write(path, &bytes).map(drop)
}

/// `path`'s folder, made when it isn't there
fn make_folder(path: &Path) -> Result<(), String> {
    path.parent().map_or(Ok(()), |folder| fs::create_dir_all(folder).map_err(|e| format!("Couldn't make {} ({e})", folder.display())))
}

/// Saves `bytes` as the settings file `path`, whole (`atomic_file::write`: in place, flushed, where the temp can't be made), its folder
/// made first; then its last good copy (`backup_of`), the same bytes, whole too (the module's doc). A copy that can't be written leaves
/// the save done, which the log says: where `settings\` takes no new file, there may be none.
pub fn save(path: &Path, bytes: &[u8]) -> Result<Written, String> {
    make_folder(path)?;
    let written = atomic_file::write(path, bytes)?;
    if let Err(e) = atomic_file::write(&backup_of(path), bytes) {
        log::info!("{} is saved, but not its last good copy: {e}", path.display());
    }
    Ok(written)
}

/// `<path>.bak`: the settings file's last good copy, which a load takes when the file is there but unreadable (the module's doc)
pub fn backup_of(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

/// `path`, a settings file there but unreadable, copied to `<path>.unreadable`, over an older one, before a save writes over it: the
/// user's to look at, or delete. Where, or why it couldn't be: where the folder takes no new file (a file still in the old place of an
/// install for all users, `in_home`), and then a calibration's save writes over it, the window's not (`settings.rs`). A copy, not a
/// rename: a rename in its own folder needs no right to make a file there, so it would work there, and leave no file the user could
/// write, nor make, to save in.
pub fn keep_aside(path: &Path) -> Result<PathBuf, String> {
    let kept = with_suffix(path, ".unreadable");
    match fs::copy(path, &kept) {
        Ok(_) => {
            log::warn!("{} is unreadable: kept as {}", path.display(), kept.display());
            Ok(kept)
        }
        Err(e) => {
            log::warn!("{} is unreadable, and couldn't be kept as {} ({e})", path.display(), kept.display());
            Err(e.to_string())
        }
    }
}

/// `path` with `suffix` after its name
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::TempDir;

    /// A home with nothing in it has its settings files in `settings\`; one still in the old place is taken into it, whole and as it is,
    /// and left where it was; one already in `settings\` is never taken over, whatever the old place has
    #[test]
    fn a_settings_file_in_the_old_place_is_taken_into_settings() {
        let dir = TempDir::new("settings-take");
        let home = PathBuf::from(dir.file("home"));
        fs::create_dir(&home).unwrap();
        let path = home.join(FOLDER).join("tmr-cfg.json");
        assert_eq!(in_home(&home, "tmr-cfg.json"), path, "nothing there");
        assert!(!home.join(FOLDER).exists(), "a path, nothing made");

        let old = home.join("tmr-cfg.json");
        fs::write(&old, b"{\"machine_id\": \"from before settings\"}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json"), path);
        assert_eq!(fs::read(&path).unwrap(), fs::read(&old).unwrap(), "taken as it is");
        assert!(old.exists() && !with_suffix(&path, ".tmp").exists(), "the old one left, no temp");

        fs::write(&old, b"{\"machine_id\": \"a copy by hand\"}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json"), path);
        assert_eq!(fs::read(&path).unwrap(), b"{\"machine_id\": \"from before settings\"}", "not over settings\\'s own");

        // Whatever the bytes: whether they are a settings file is the load's to say
        fs::write(home.join("window.json"), [0u8, 0, 0xFF]).unwrap();
        assert_eq!(fs::read(in_home(&home, "window.json")).unwrap(), [0u8, 0, 0xFF]);
    }

    /// One that can't be taken is the old one, so nothing of it is lost: unread (held by a program that shares it with no one), its load
    /// says so; where `settings\` can't be made (a file has its name), it is read and saved where it is
    #[test]
    fn a_settings_file_that_cant_be_taken_stays_the_old_one() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = TempDir::new("settings-not-taken");
        let home = PathBuf::from(dir.file("home"));
        fs::create_dir(&home).unwrap();
        let old = home.join("window.json");
        fs::write(&old, b"{}").unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&old).unwrap();
        assert_eq!(in_home(&home, "window.json"), old, "unread");
        assert!(!home.join(FOLDER).join("window.json").exists());
        drop(held);

        let blocked = TempDir::new("settings-no-folder");
        let home = PathBuf::from(blocked.file("home"));
        fs::create_dir(&home).unwrap();
        fs::write(home.join(FOLDER), b"a file where the folder would be").unwrap();
        fs::write(home.join("tmr-cfg.json"), b"{}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json"), home.join("tmr-cfg.json"));
    }

    /// A save writes the file and its last good copy, the same bytes, making `settings\` first; a copy that can't be written leaves the
    /// save done
    #[test]
    fn a_save_writes_the_file_then_its_last_good_copy() {
        let dir = TempDir::new("settings-save");
        let path = PathBuf::from(dir.file("home")).join(FOLDER).join("tmr-cfg.json");
        assert_eq!(save(&path, b"{\"first\": 1}"), Ok(Written::Replaced), "settings\\ made");
        assert_eq!(save(&path, b"{\"second\": 2}"), Ok(Written::Replaced));
        assert_eq!(
            (fs::read(&path).unwrap(), fs::read(backup_of(&path)).unwrap()),
            (b"{\"second\": 2}".to_vec(), b"{\"second\": 2}".to_vec())
        );
        assert_eq!(backup_of(&path), path.with_file_name("tmr-cfg.json.bak"));

        // A folder where the copy would be: neither written, nor replaced
        let other = path.with_file_name("window.json");
        fs::create_dir(backup_of(&other)).unwrap();
        assert_eq!(save(&other, b"{}"), Ok(Written::Replaced));
        assert_eq!(fs::read(&other).unwrap(), b"{}");
    }

    /// In a `settings\` that takes no new file, as a share or a folder made without users' right to add one may be, a save writes the
    /// file in place, flushed, and its last good copy in place too when there is one; with none, the save is still done
    #[test]
    fn where_settings_takes_no_new_file_a_save_is_in_place() {
        let dir = TempDir::new("settings-in-place");
        let folder = PathBuf::from(dir.file(FOLDER));
        fs::create_dir(&folder).unwrap();
        let (path, other) = (folder.join("tmr-cfg.json"), folder.join("window.json"));
        for file in [&path, &backup_of(&path), &other] {
            fs::write(file, b"{}").unwrap();
        }
        let _locked = atomic_file::NoNewFiles::on(&folder);
        assert_eq!(save(&path, b"{\"saved\": 1}"), Ok(Written::InPlace));
        assert_eq!(
            (fs::read(&path).unwrap(), fs::read(backup_of(&path)).unwrap()),
            (b"{\"saved\": 1}".to_vec(), b"{\"saved\": 1}".to_vec())
        );
        assert_eq!(save(&other, b"{\"chart\": false}"), Ok(Written::InPlace), "no copy, and none can be made");
        assert_eq!((fs::read(&other).unwrap(), backup_of(&other).exists()), (b"{\"chart\": false}".to_vec(), false));
    }

    /// A file kept aside is a copy, over an older one, the file itself left to be saved over
    #[test]
    fn a_file_kept_aside_is_a_copy() {
        let dir = TempDir::new("settings-keep-aside");
        let path = PathBuf::from(dir.file("window.json"));
        fs::write(&path, b"{\"chart\": ").unwrap();
        fs::write(with_suffix(&path, ".unreadable"), b"an older one").unwrap();
        assert_eq!(keep_aside(&path), Ok(PathBuf::from(dir.file("window.json.unreadable"))));
        assert_eq!(fs::read(dir.file("window.json.unreadable")).unwrap(), b"{\"chart\": ");
        assert!(path.exists());
        assert!(keep_aside(&PathBuf::from(dir.file("absent.json"))).is_err());
    }
}
