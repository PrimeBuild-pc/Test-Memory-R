//! Ordinary launches use the executable's folder or TMR_HOME for configs, results, logs and settings. Elevated launches use a separate
//! administrator-controlled home, established before either frontend opens those files. An explicitly named input is resolved from the
//! working directory first.

use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod admin;

pub use crate::memory::privileges::elevation;

struct Home {
    path: PathBuf,
    admin_sid: Option<String>,
}
static HOME: OnceLock<Result<Home, String>> = OnceLock::new();

fn selected() -> Result<&'static Home, String> {
    #[cfg(test)]
    use_test_home();
    HOME.get_or_init(|| {
        if !elevation().map_err(|e| format!("Cannot determine process elevation: {e}"))? {
            return Ok(Home { path: ordinary_home(), admin_sid: None });
        }
        let path = admin::open(from_var().as_deref())?;
        Ok(Home { path, admin_sid: Some(crate::memory::lsa::Token::read()?.user.to_string()) })
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// The engine tests and the frontend fixtures use a fresh, process-wide temporary home, including when the test runner is elevated.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn use_test_home() {
    static TEMP: OnceLock<PathBuf> = OnceLock::new();
    extern "C" fn clean() {
        if let Some(dir) = TEMP.get() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    unsafe extern "C" {
        fn atexit(callback: extern "C" fn()) -> i32;
    }
    let home = HOME.get_or_init(|| {
        // SAFETY: the CRT calls this static function at process exit, once all the libtest threads have finished using their shared home.
        if unsafe { atexit(clean) } != 0 {
            return Err("Couldn't register temporary test-home cleanup".into());
        }
        let elevated = elevation().map_err(|e| e.to_string())?;
        let base = if elevated { admin::test_parent()? } else { std::env::temp_dir() };
        // SAFETY: CoCreateGuid returns the new GUID by value and needs no COM apartment.
        let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|e| e.to_string())?;
        let path = base.join(format!("tmr-test-home-{id:?}"));
        if elevated {
            admin::create_test_directory(&path)?;
        } else {
            std::fs::create_dir(&path).map_err(|e| e.to_string())?;
        }
        TEMP.set(path.clone()).expect("only this initializer owns the freshly created temporary directory");
        let admin_sid = if elevated {
            admin::open(Some(&path))?;
            Some(crate::memory::lsa::Token::read()?.user.to_string())
        } else {
            None
        };
        Ok(Home { path, admin_sid })
    });
    home.as_ref().expect("initialize temporary test home");
    assert!(TEMP.get().is_some(), "a frontend fixture used the installed home before selecting test storage");
}

/// Called before either frontend reads settings or opens a log. Failure stops startup.
pub fn initialize() -> Result<(), String> {
    selected().map(drop)
}

/// Whether this process selected protected storage for an elevated run.
pub fn elevated() -> bool {
    selected().expect("TMR home must be available before use").admin_sid.is_some()
}

/// Elevated window preferences are per home and Windows account.
pub fn window_settings_name() -> String {
    selected()
        .expect("TMR home must be available before use")
        .admin_sid
        .as_ref()
        .map_or_else(|| "window.json".into(), |sid| format!("window-{sid}.json"))
}

fn consent_path() -> Option<PathBuf> {
    let home = selected().ok()?;
    Some(home.path.join("settings").join(format!("allow-window-{}.consent", home.admin_sid.as_ref()?)))
}

/// Read the per-account administrator-window switch afresh. An absent or invalid record requires a prompt.
pub fn window_consent() -> bool {
    consent_path().is_some_and(|p| admin::consent(&p))
}

/// Only an explicit change in the elevated Settings dialog writes this switch. Other window saves cannot restore a revoked decision.
pub fn set_window_consent(on: bool) -> Result<(), String> {
    let path = consent_path().ok_or("Administrator window settings require an elevated launch")?;
    admin::set_consent(&path, on)
}

/// Settings files belonging to the elevated home are read with their security descriptor checked on the open handle.
pub(crate) fn protected_setting(path: &Path) -> bool {
    selected().is_ok_and(|h| h.admin_sid.is_some() && path.parent() == Some(h.path.join("settings").as_path()))
}

/// Read the same handle whose permissions were checked for protected settings, including their backup.
pub fn read_settings(path: &Path) -> std::io::Result<Vec<u8>> {
    if protected_setting(path) { admin::read_setting(path) } else { std::fs::read(path) }
}

/// The environment variable that names another home
pub const HOME_VAR: &str = "TMR_HOME";

/// The protected home when elevated. Otherwise, `TMR_HOME` when it is set (a relative one from the current folder), else the folder of the real exe.
/// `current_exe` gives the path the exe was started by, which can be a symlink or a short (8.3) name, so it is resolved first, and taken
/// as it is when it can't be. The current folder only when even `current_exe` fails.
pub fn dir() -> &'static Path {
    &selected().expect("TMR home must be available before use").path
}

fn ordinary_home() -> PathBuf {
    if let Some(home) = from_var() {
        return std::path::absolute(&home).unwrap_or(home);
    }
    std::env::current_exe()
        .ok()
        .map(|exe| std::fs::canonicalize(&exe).unwrap_or(exe))
        .and_then(|exe| exe.parent().map(without_verbatim_prefix))
        .unwrap_or_else(|| std::path::absolute(".").unwrap_or_else(|_| PathBuf::from(".")))
}

/// `TMR_HOME`, when it is set and not empty
fn from_var() -> Option<PathBuf> {
    std::env::var_os(HOME_VAR).filter(|home| !home.is_empty()).map(PathBuf::from)
}

/// The configs the user keeps, which a relative `config=` is looked for in after the current folder
pub fn configs() -> PathBuf {
    dir().join("configs")
}

/// The run's results files
pub fn results() -> PathBuf {
    dir().join("results")
}

/// The run's log files
pub fn logs() -> PathBuf {
    dir().join("logs")
}

/// Makes the home's folders, and checks TMR can write in the two a run writes to, `results\` and `logs\`: a session calls it before it
/// allocates, so a home TMR can't write to (a tmr.exe copied into Program Files) stops it then, not at the end of a run with no results.
/// `configs\` is only the user's: a run never writes there, so one that can't be made stops nothing.
pub fn prepare() -> Result<(), String> {
    if let Err(e) = std::fs::create_dir_all(configs()) {
        log::debug!("Couldn't make {}: {e}", configs().display());
    }
    for folder in [results(), logs()] {
        std::fs::create_dir_all(&folder).and_then(|()| probe(&folder)).map_err(|e| cant_write(&folder, &e))?;
    }
    Ok(())
}

/// Whether TMR can save `path` in its home: write the file when it is there (one writable by users in a folder that isn't, as an
/// install for all users made `tmr-cfg.json` beside tmr.exe), or make a new one in its folder when it isn't, the folder made first, as
/// a save makes it (`settings\`). Opening an existing file to write it, without truncating, changes nothing; a calibration checks it
/// before it measures, rather than finding out after.
pub(crate) fn can_save(path: &Path) -> Result<(), String> {
    let checked = if path.exists() {
        std::fs::OpenOptions::new().write(true).open(path).map(drop)
    } else {
        path.parent().map_or(Ok(()), |folder| std::fs::create_dir_all(folder).and_then(|()| probe(folder)))
    };
    checked.map_err(|e| cant_write(path, &e))
}

/// Writes a new file in `folder` and deletes it. A name of this process's own, and only a new file: in an install for all users these
/// folders are writable by every user, and TMR may run elevated, so it never opens a file someone else put there, a link among them.
/// Deleted by its handle when it closes, not by its name.
fn probe(folder: &Path) -> std::io::Result<()> {
    use windows::Win32::Foundation::GENERIC_WRITE;
    use windows::Win32::Storage::FileSystem::{DELETE, FILE_FLAG_DELETE_ON_CLOSE};
    let name = format!(".tmr-write-check-{}", std::process::id());
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_WRITE.0 | DELETE.0)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
        .open(folder.join(name))
        .map(drop)
}

fn cant_write(path: &Path, e: &std::io::Error) -> String {
    if elevated() {
        return format!("TMR couldn't write to its administrator home at {}: {e}", path.display());
    }
    match from_var() {
        Some(home) => format!(
            "TMR can't write to {} ({e}). {HOME_VAR} is {}: set it to a folder you can write to, or unset it to keep TMR's files beside \
             tmr.exe.",
            path.display(),
            home.display()
        ),
        None => format!(
            "TMR can't write to {} ({e}). It keeps its files beside tmr.exe: move tmr.exe to a folder you can write to, install it with \
             its installer, or set {HOME_VAR} to such a folder.",
            path.display()
        ),
    }
}

/// Resolve a test config the same way for the loader and a window's saved invocation.
pub fn config_path(name: &str) -> Result<PathBuf, String> {
    find(name, &configs()).or_else(|looked| {
        // The installer stages its examples beside the executable, outside the shared writable data folders.
        let typed = Path::new(name);
        if !elevated()
            && typed.components().count() == 1
            && let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let example = dir.join("examples").join(typed);
            if example.is_file() {
                return Ok(example);
            }
        }
        Err(looked)
    })
}

/// The file `path` names, and where it was looked for when it isn't found: as typed, from the current folder; then, when `path` is
/// relative, in `folder` (the home's `configs\` or `results\`), and in the home, as TMR shows its own files (`results\TMR_….json`).
pub(crate) fn find(path: &str, folder: &Path) -> Result<PathBuf, String> {
    let typed = Path::new(path);
    if typed.exists() {
        return Ok(typed.to_path_buf());
    }
    if typed.is_relative() {
        for place in [folder, dir()] {
            let found = place.join(typed);
            if found.exists() {
                return Ok(found);
            }
        }
        return Err(format!("looked in the current folder, {} and {}", folder.display(), dir().display()));
    }
    Err("looked where it says".to_string())
}

/// `path` as Explorer shows it: `canonicalize` gives a drive path as `\\?\C:\...` and a share as `\\?\UNC\server\share\...`.
fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else { return path.to_path_buf() };
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{share}"))
    } else if let Some(drive) = text.strip_prefix(r"\\?\").filter(|rest| rest.as_bytes().get(1) == Some(&b':')) {
        PathBuf::from(drive)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical path loses the `\\?\` prefix a user never typed, a drive's and a share's; any other path is kept as it is.
    #[test]
    fn a_home_reads_as_explorer_shows_it() {
        for (canonical, shown) in [
            (r"\\?\C:\Program Files\TMR", r"C:\Program Files\TMR"),
            (r"\\?\UNC\server\share\tmr", r"\\server\share\tmr"),
            (r"C:\tools", r"C:\tools"),
            (r"\\?\Volume{0e5b}\tmr", r"\\?\Volume{0e5b}\tmr"),
        ] {
            assert_eq!(without_verbatim_prefix(Path::new(canonical)), PathBuf::from(shown), "{canonical}");
        }
    }

    /// A file named as typed is found from the current folder first; a relative name not there, in the folder given; an absolute one
    /// only where it says. One found nowhere says where it was looked for.
    #[test]
    fn a_typed_path_is_found_here_then_in_the_home() {
        let dir = crate::results::TempDir::new("home-find");
        let folder = Path::new(&dir.file("configs")).to_path_buf();
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("mine.cfg"), b"").unwrap();
        assert_eq!(find("mine.cfg", &folder), Ok(folder.join("mine.cfg")), "in the folder");
        assert_eq!(find("Cargo.toml", &folder), Ok(PathBuf::from("Cargo.toml")), "here first");
        assert!(find("absent.cfg", &folder).unwrap_err().contains(&folder.display().to_string()), "says where it looked");
        let absolute = folder.join("mine.cfg").display().to_string();
        assert_eq!(find(&absolute, &folder), Ok(PathBuf::from(&absolute)));
        assert!(find(&folder.join("absent.cfg").display().to_string(), &folder).is_err());
    }

    /// The write check leaves nothing behind, and a file already there in an existing one's place doesn't stop it.
    #[test]
    fn the_write_check_leaves_no_file() {
        let dir = crate::results::TempDir::new("home-probe");
        let folder = Path::new(&dir.file("results")).to_path_buf();
        std::fs::create_dir_all(&folder).unwrap();
        probe(&folder).unwrap();
        probe(&folder).unwrap();
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0, "the probe's file is gone");
        let saved = folder.join("tmr-cfg.json");
        assert!(can_save(&saved).is_ok(), "a new file in a folder TMR can write");
        std::fs::write(&saved, b"{}").unwrap();
        assert!(can_save(&saved).is_ok(), "the file there");
        assert_eq!(std::fs::read(&saved).unwrap(), b"{}", "untouched by the check");
        let settings = folder.join(crate::settings_file::FOLDER);
        assert!(can_save(&settings.join("tmr-cfg.json")).is_ok(), "settings\\ not made yet");
        assert_eq!(std::fs::read_dir(&settings).unwrap().count(), 0, "made, and empty");
    }
}
