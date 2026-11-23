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
//! once, the first time TMR loads or saves it, and left where it was (`path`): with its last good copy, so a `settings\` that has had
//! it holds one of the two, and a file deleted there to start again takes the defaults, not the old one again.

use std::fs;
use std::path::{Path, PathBuf};

use crate::atomic_file::{self, Written};

/// The folder in TMR's home that holds the settings files
pub const FOLDER: &str = "settings";

/// The settings file `name` (`tmr-cfg.json`, `window.json`): in the home's `settings\`, a file still in the old place taken first
/// (`in_home`)
pub fn path(name: &str) -> PathBuf {
    in_home(crate::home::dir(), name, crate::memory::privileges::is_elevated())
}

/// `home`'s settings file `name`, in its `settings\`. When that has had none (it holds neither the file nor its last good copy) and the
/// home itself has one, from before `settings\`, it is taken first: read, and saved into `settings\` whole, with its copy, the old one
/// left where it was, as it may be no file TMR can delete (`take`). Taken once: a file deleted from `settings\` to start again leaves its
/// copy, so it takes the defaults, as the guide says, not the old one again (review: the old file outlives the take in an unzipped exe
/// updated in place, a `TMR_HOME` of the repo, an install whose upgrade couldn't move it).
///
/// One that can't be taken is the old one, so nothing of it is lost: one that can't be read is said by its load and isn't saved over,
/// and in a home where `settings\` can't be made or takes no file (read-only media, an old install's folder) it is read, and saved,
/// where it is. So is every one when TMR runs as administrator, `elevated` (review). In a home users can't write, an install for all
/// users whose new tmr.exe was copied over the old by hand, without the installer, a `settings\` it made, for this file or for a save of
/// the other, would hand down only the home's right to read: every user's TMR, finding the file there, could then save it neither whole
/// nor in place, where the old one, beside tmr.exe, was theirs to write. TMR run as its user takes it: into a `settings\` the installer
/// made writable by users (`dist/tmr.iss`), or one it makes as that user can.
fn in_home(home: &Path, name: &str, elevated: bool) -> PathBuf {
    let (path, old) = (home.join(FOLDER).join(name), home.join(name));
    // A link is a file there, for this as for a save: none is followed
    let there = |path: &Path| fs::symlink_metadata(path).is_ok();
    if there(&path) || there(&backup_of(&path)) || !there(&old) {
        return path;
    }
    if elevated {
        log::info!(
            "{} isn't taken into {}: TMR, run as administrator, takes none, as users may be unable to write what it makes there; it is \
             read and saved where it is",
            old.display(),
            path.display()
        );
        return old;
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

/// `old`'s bytes saved as `path` and its last good copy, whole, as they are (`save`): whether they are a settings file is its load's to
/// say. The copy marks `settings\` as having had the file (`in_home`).
fn take(old: &Path, path: &Path) -> Result<(), String> {
    let bytes = fs::read(old).map_err(|e| format!("couldn't read it: {e}"))?;
    save(path, &bytes).map(drop)
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
/// write, nor make, to save in. Written as a save is (`atomic_file::write`), never through a link at `<path>.unreadable` (review: a copy
/// wrote through one, so TMR run as administrator, at a setup or the window's start, wrote the bad file's bytes, which any user can
/// choose, over the file a link there named).
pub fn keep_aside(path: &Path) -> Result<PathBuf, String> {
    let kept = with_suffix(path, ".unreadable");
    match fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| atomic_file::write(&kept, &bytes)) {
        Ok(_) => {
            log::warn!("{} is unreadable: kept as {}", path.display(), kept.display());
            Ok(kept)
        }
        Err(e) => {
            log::warn!("{} is unreadable, and couldn't be kept as {} ({e})", path.display(), kept.display());
            Err(e)
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
    /// with its last good copy, and left where it was; one already in `settings\` is never taken over, whatever the old place has
    #[test]
    fn a_settings_file_in_the_old_place_is_taken_into_settings() {
        let dir = TempDir::new("settings-take");
        let home = PathBuf::from(dir.file("home"));
        fs::create_dir(&home).unwrap();
        let path = home.join(FOLDER).join("tmr-cfg.json");
        assert_eq!(in_home(&home, "tmr-cfg.json", false), path, "nothing there");
        assert!(!home.join(FOLDER).exists(), "a path, nothing made");

        let old = home.join("tmr-cfg.json");
        fs::write(&old, b"{\"machine_id\": \"from before settings\"}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", false), path);
        assert_eq!(fs::read(&path).unwrap(), fs::read(&old).unwrap(), "taken as it is");
        assert_eq!(fs::read(backup_of(&path)).unwrap(), fs::read(&old).unwrap(), "with its last good copy");
        assert!(old.exists() && !with_suffix(&path, ".tmp").exists(), "the old one left, no temp");

        fs::write(&old, b"{\"machine_id\": \"a copy by hand\"}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", false), path);
        assert_eq!(fs::read(&path).unwrap(), b"{\"machine_id\": \"from before settings\"}", "not over settings\\'s own");

        // Whatever the bytes: whether they are a settings file is the load's to say
        fs::write(home.join("window.json"), [0u8, 0, 0xFF]).unwrap();
        assert_eq!(fs::read(in_home(&home, "window.json", false)).unwrap(), [0u8, 0, 0xFF]);
    }

    /// Taken once (review): a file deleted from `settings\` to start again, straight after the take or after a save, is the defaults,
    /// not the old one taken again, which is left where it was; its last good copy is left alone too
    #[test]
    fn a_settings_file_deleted_to_start_again_is_the_defaults_not_the_old_one() {
        use crate::app_config::AppConfig;
        let dir = TempDir::new("settings-take-once");
        let home = PathBuf::from(dir.file("home"));
        fs::create_dir(&home).unwrap();
        let (path, old) = (home.join(FOLDER).join("tmr-cfg.json"), home.join("tmr-cfg.json"));
        fs::write(&old, b"{\"machine_id\": \"old place, before settings\"}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", false), path);
        fs::remove_file(&path).unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", false), path, "straight after the take");
        assert_eq!(AppConfig::load(&path).machine_id, None, "the defaults");

        save(&path, b"{\"machine_id\": \"newer, saved in settings\"}").unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", false), path, "after a save");
        assert_eq!(AppConfig::load(&path).machine_id, None, "the defaults");
        assert!(!path.exists() && old.exists());
        assert_eq!(fs::read(backup_of(&path)).unwrap(), b"{\"machine_id\": \"newer, saved in settings\"}");
    }

    /// Run as administrator, TMR takes no file into `settings\` (review): in a home users can't write, a folder it made, for this file
    /// or a save of the other, would be theirs to read only, so the old file is read and saved where it is, `settings\` there or not.
    /// With no old file, or one already in `settings\`, the path is `settings\`'s, nothing made; run as its user, TMR takes it
    #[test]
    fn run_as_administrator_tmr_takes_no_settings_file() {
        let dir = TempDir::new("settings-elevated");
        let home = PathBuf::from(dir.file("home"));
        fs::create_dir(&home).unwrap();
        let (folder, old) = (home.join(FOLDER), home.join("tmr-cfg.json"));
        assert_eq!(in_home(&home, "tmr-cfg.json", true), folder.join("tmr-cfg.json"), "no old file");
        fs::write(&old, b"{}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", true), old);
        assert!(!folder.exists(), "settings\\ not made");

        // Made by a save of the other file, run as administrator too
        save(&folder.join("window.json"), b"{}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", true), old);
        assert!(!folder.join("tmr-cfg.json").exists(), "not taken");

        assert_eq!(in_home(&home, "tmr-cfg.json", false), folder.join("tmr-cfg.json"));
        assert_eq!(fs::read(folder.join("tmr-cfg.json")).unwrap(), b"{}", "taken run as its user");
        assert_eq!(in_home(&home, "tmr-cfg.json", true), folder.join("tmr-cfg.json"), "then settings\\'s, run as administrator too");
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
        assert_eq!(in_home(&home, "window.json", false), old, "unread");
        assert!(!home.join(FOLDER).join("window.json").exists());
        drop(held);

        let blocked = TempDir::new("settings-no-folder");
        let home = PathBuf::from(blocked.file("home"));
        fs::create_dir(&home).unwrap();
        fs::write(home.join(FOLDER), b"a file where the folder would be").unwrap();
        fs::write(home.join("tmr-cfg.json"), b"{}").unwrap();
        assert_eq!(in_home(&home, "tmr-cfg.json", false), home.join("tmr-cfg.json"));
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

    /// A link at `<file>.unreadable`, or at its last good copy, is replaced, never written through (review: in an install for all users
    /// `settings\` is writable by every user, and a file kept aside, or a copy written in place, went over the file a link there named,
    /// with what the bad file held, which that user can write): the kept file, or the save, is written whole, and where the copy can only
    /// be written in place, it isn't, and the save is done
    #[test]
    fn a_link_where_a_settings_file_is_kept_aside_or_copied_isnt_written_through() {
        let dir = TempDir::new("settings-links");
        let victim = PathBuf::from(dir.file("victim.txt"));
        fs::write(&victim, b"not TMR's").unwrap();
        let path = PathBuf::from(dir.file("tmr-cfg.json"));
        fs::write(&path, b"what any user wrote here, not a config").unwrap();
        fs::hard_link(&victim, with_suffix(&path, ".unreadable")).unwrap();
        assert!(keep_aside(&path).is_ok());
        assert_eq!(fs::read(with_suffix(&path, ".unreadable")).unwrap(), b"what any user wrote here, not a config");
        assert_eq!(fs::read(&victim).unwrap(), b"not TMR's");

        let other = PathBuf::from(dir.file("window.json"));
        fs::write(&other, b"{\"chart\": ").unwrap();
        if atomic_file::symlink(&victim, &with_suffix(&other, ".unreadable")) {
            assert!(keep_aside(&other).is_ok());
            assert!(!fs::symlink_metadata(with_suffix(&other, ".unreadable")).unwrap().file_type().is_symlink());
            assert_eq!(fs::read(&victim).unwrap(), b"not TMR's");
        }

        // The copy's temp stopped by a folder of its name, so it could only be written in place
        let (copied, copy) = (PathBuf::from(dir.file("third.json")), with_suffix(&PathBuf::from(dir.file("third.json")), ".bak"));
        fs::hard_link(&victim, &copy).unwrap();
        fs::create_dir(with_suffix(&copy, ".tmp")).unwrap();
        assert_eq!(save(&copied, b"{}"), Ok(Written::Replaced));
        assert_eq!((fs::read(&copied).unwrap(), fs::read(&victim).unwrap()), (b"{}".to_vec(), b"not TMR's".to_vec()));
    }
}
