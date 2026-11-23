//! `--sanitize <file>`, and the window's ••• menu (the owner, 2026-11-23, from the forum's feedback): copies of a run's results file and
//! its log without what ties them to the machine and the people on it, to post. One file names the pair, the results file
//! (`results\<stem>.json`) or the log (`logs\<stem>.log`), which share their stem (`run_context::run_file_stem`); the other is looked
//! for in TMR's layout (`..\logs\` beside a results file's folder, `..\results\` beside a log's), then in the same folder (a pair
//! copied together), and is the first there with the same run ID (the results file's `metadata.run_id`, the log header's,
//! `logfile::run_id_of`; an empty one is none), so another run's file in one place doesn't hide this run's in the other. Nothing is
//! written unless both are there, both read, the results file is one TMR reads, and the run IDs are the same.
//!
//! `--sanitize-single <file>` (the owner, 2026-11-23) sanitizes one file alone, no partner looked for and no run ID checked: a results
//! file without its log, or a log without its results file (one from before TMR recorded a run ID among them), each into its own copy
//! beside it. It removes what the pair's sanitize would that the file shows by itself, by the same rules (`Scrubber`). A results file
//! names its own values. A log alone has no results file to name them, so they are found by TMR's own lines for them (`log_values`: the
//! console's for the system and each DIMM, the SMBIOS parse's, a calibration identity check's `current=`) and then replaced wherever else
//! the log has them as words; its words of the hardware are those lines' (`log_part_words`). What only the other file would have named
//! stays, and the outcome says so in a line (`RESULTS_ALONE`, `LOG_ALONE`); with nothing replaced, it says only that the file shows
//! nothing to replace by itself, never that it names no one.
//!
//! What it replaces with `REMOVED`, each occurrence in either file (`Kind` counts them). Each file gets every rule by itself, a pair's as
//! a file alone's; its partner only adds the values and names it gives.
//! - the values TMR labels, by their label whatever their length, but a stand-in (`stand_in`): each DIMM's serial, the system UUID,
//!   `machine_id` and `memory_id`, in the results file's fields for them (`LABELED_FIELDS`) and in the log's lines for them
//!   (`labeled_spans`), then wherever else either file has one as a word, one long enough to be no other word (`informative`). A run
//!   with `--serials` keeps them; without it the run left them out already, and its stand-ins stay;
//! - what only the log has: the SMBIOS parse's string tables (`RUST_LOG=debug`: the system's, the board's and the chassis' serials and
//!   asset tags among them), and the values tmr-cfg.json recorded, in a calibration's identity check (its `stored=` and `current=`);
//! - the user folder of a profile path (`C:\Users\<name>\`, but `Public` and `Default`) and a OneDrive's organisation
//!   (`OneDrive - <name>`, and a SharePoint library's it syncs, `C:\Users\<user>\<name>\<site> - <library>\`), each whole
//!   (`folder_name`), and a share's PC (`\\<PC>\share`, `//<PC>/share`, but `localhost` and WSL's: `OWN_HOSTS`);
//! - accounts as the large-page check names them (`👤 User: PC\name`, by its label, and beside a SID: a grant's, the security policy's
//!   holders), but the built-in ones (`BUILTIN\Administrators`), and every SID of a machine or a domain (`S-1-5-21-…`, Entra ID's
//!   `S-1-12-1-…`; the well-known ones, `S-1-5-32-544`, stay);
//! - the PC's name in a Windows event's XML (`<Computer>`, a WHEA event's, which the log has at debug and where Windows gives no message
//!   for the event): its full DNS name, which no account has (a NetBIOS name stops at 15 characters, and a run whose large-page question
//!   was answered, or an Entra ID account, names no PC);
//! - then those accounts' PC (or domain) and user names, the events' and the shares' PC names, their hosts and DNS domains, and the user
//!   folders' names, wherever else either file has them as words, but a name that identifies no one (`Administrator`, `tmr`:
//!   `NO_ONES_NAMES`), one shorter than 3 letters, and one that is a word of the machine's description (a PC named `RYZEN` on a Ryzen),
//!   which are replaced only where the rules above find them.
//!
//! The results file stays JSON that TMR reads (`--show-results`, `--compare-results`, the window's Compare tab): only its strings change,
//! each a whole value, its layout as it was, and the copy is read back before anything is written. The values as words and the names
//! are replaced only in its free text (`FREE_TEXT_FIELDS`: its command line, its config's name), never in TMR's own values: an account
//! named `failed` leaves the verdict as it was. What it can't know stays: a name in a folder outside `\Users\`, a config's own name, the
//! time zone in the timestamps, the hardware itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::results::TestRunResult;

/// What the copies have in place of each value they leave out
pub const REMOVED: &str = "<removed>";

/// What a copy's name has before its extension: `<stem>.sanitized.json`, `<stem>.sanitized.log`
const SANITIZED: &str = ".sanitized";

/// The large-page check's line naming the account TMR runs as (`memory::diagnose_large_pages`)
const USER_LINE: &str = "👤 User: ";

/// The domains of the accounts Windows has on every PC: no PC's or person's
const BUILT_IN_DOMAINS: [&str; 9] = [
    "BUILTIN", "NT AUTHORITY", "NT SERVICE", "NT VIRTUAL MACHINE", "NT Pseudo Domain", "Window Manager", "Font Driver Host", "IIS APPPOOL",
    "APPLICATION PACKAGE AUTHORITY",
];

/// The domains of a Microsoft or an Entra ID account, which name no PC: the account is replaced, and its user name, not the domain
const ACCOUNT_DOMAINS: [&str; 2] = ["AzureAD", "MicrosoftAccount"];

/// The profiles every PC shares, whose folders name no one: not replaced in a path
const SHARED_PROFILES: [&str; 4] = ["Public", "Default", "Default User", "All Users"];

/// The folders Windows makes in a profile, which name no organisation (`library_organisations`), as they are on the disk (Explorer shows
/// them in the system's language)
const PROFILE_FOLDERS: [&str; 13] = [
    "Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos", "Favorites", "Links", "Contacts", "Searches", "Saved Games",
    "3D Objects", "AppData",
];

/// The names that identify no one, which are replaced only where an account or a path has them, not as words elsewhere: the built-in
/// accounts', the shared profiles', and words TMR's own lines are full of (an account named `tmr` would take every `tmr::` with it)
const NO_ONES_NAMES: [&str; 12] = [
    "Administrator", "Guest", "DefaultAccount", "WDAGUtilityAccount", "defaultuser0", "tmr", "test", "user", "admin", "owner", "home",
    "dev",
];

/// What SMBIOS gives where a maker wrote no serial: no machine's
const PLACEHOLDERS: [&str; 9] = [
    "Unknown", "None", "Not Specified", "Not Available", "N/A", "To Be Filled By O.E.M.", "Default string", "System Serial Number",
    "Serial Number",
];

/// The calibration identity checks' lines, whose `stored=` values tmr-cfg.json recorded and whose `current=` is the run's own value
/// (`MachineFingerprint::matches`), each with the kind of that value
const RECORDED_LINES: [(&str, Kind); 3] = [
    ("machine ID mismatch", Kind::MachineId),
    ("system UUID mismatch", Kind::SystemUuid),
    ("memory configuration changed", Kind::MemoryId),
];

/// The console's line for the system (tmr-cli's `machine_text`), with its UUID after `SYSTEM_UUID`; each DIMM's line follows it,
/// `MODULE_INDENT` in, its serial last, after `MODULE_SERIAL`
const SYSTEM_LINE: &str = "🖥️  System: ";
const SYSTEM_UUID: &str = " · UUID ";
const MODULE_INDENT: &str = "    ";
const MODULE_SERIAL: &str = " · serial ";

/// The SMBIOS parse's summary line (`smbios::parse_smbios`), with the system UUID after `SMBIOS_UUID`
const SMBIOS_LINE: &str = "SMBIOS: BIOS=";
const SMBIOS_UUID: &str = ", UUID=";

/// The CPU's lines, which name it: the System Information table's row, and the cache detection's at debug (`cache::CacheInfo`)
const CPU_LINES: [&str; 2] = ["CPU           ", "]   CPU: "];

/// What the outcome of a file sanitized alone says it couldn't remove, without the other file to name it: for a results file, then a
/// log
const RESULTS_ALONE: &str = "Without its log: a PC or user name only the log gives (the account TMR ran as, a WHEA event's PC) stays \
                             where this file has it, outside a user folder's path or a share's";
const LOG_ALONE: &str = "Without its results file: the serials, UUID and fingerprints were found by TMR's lines for them, not by their \
                         values: one in a line of another kind (an older build's) stays";

/// A refusal's hint, where the pair isn't one run's: the file can still be sanitized alone
const ALONE_HINT: &str = "--sanitize-single (the window's Sanitize one file) sanitizes one file alone";

/// Why a sanitize wrote nothing, its check after the scrub
const LOST_RUN_ID: &str = "a sanitized copy lost its run ID: nothing written";

/// The results file's fields whose values TMR labels, each by its keys from the file's top (an array's items each the field), with the
/// kind of its value: a module's serial, and the identity's UUID and fingerprints. Each is replaced whole, whatever its length, but a
/// stand-in.
const LABELED_FIELDS: [(&[&str], Kind); 4] = [
    (&["identity", "memory_modules", "serial_number"], Kind::Serial),
    (&["identity", "system_uuid"], Kind::SystemUuid),
    (&["identity", "machine_id"], Kind::MachineId),
    (&["identity", "memory_id"], Kind::MemoryId),
];

/// The results file's fields of free text, which the run's command line and its config gave, each by its keys from the file's top: where
/// the values are replaced as words, and the names. Every other string is TMR's own (a verdict, a test's name, a page size), which only
/// the rules' patterns change (a path's user folder, a SID), so an account named `failed` leaves the verdict as it was.
const FREE_TEXT_FIELDS: [&[&str]; 4] =
    [&["run_config", "command_line"], &["run_config", "config_name"], &["metadata", "config_name"], &["metadata", "filename"]];

/// The names a share's path can have that name no particular PC (`share_hosts`): this PC's own (`localhost`), WSL's, and a remote
/// desktop's for its client's drives (`tsclient`)
const OWN_HOSTS: [&str; 5] = ["localhost", "127.0.0.1", "wsl$", "wsl.localhost", "tsclient"];

/// What can come just before a share's path (`share_hosts`): a space, a quote, a bracket, `=` (`config=`) or a list's punctuation. A
/// colon can't (`C:\\Users`, as a debug line escapes it), nor a word or a dot (`.\\configs`).
const BEFORE_A_SHARE: &[u8] = b" \t\r\n\"'`=([{<>,;|";

/// What a replacement removed, for its count
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Serial,
    SystemUuid,
    MachineId,
    MemoryId,
    /// A fingerprint or UUID tmr-cfg.json recorded, in a calibration's identity check
    Recorded,
    /// The SMBIOS parse's string table, in a debug log
    SmbiosStrings,
    UserFolder,
    OneDrive,
    Account,
    Sid,
    /// A PC's, a domain's or a user's name, where neither an account nor a path has it: an event's `<Computer>` among them
    Name,
}

impl Kind {
    /// `count` of this kind, as the outcome says it
    fn counted(self, count: usize) -> String {
        let (one, many) = match self {
            Kind::Serial => ("DIMM serial", "DIMM serials"),
            Kind::SystemUuid => ("system UUID", "system UUIDs"),
            Kind::MachineId => ("machine fingerprint (machine_id)", "machine fingerprints (machine_id)"),
            Kind::MemoryId => ("memory fingerprint (memory_id)", "memory fingerprints (memory_id)"),
            Kind::Recorded => ("fingerprint or UUID from tmr-cfg.json", "fingerprints or UUIDs from tmr-cfg.json"),
            Kind::SmbiosStrings => ("SMBIOS string table", "SMBIOS string tables"),
            Kind::UserFolder => ("user folder in a path", "user folders in paths"),
            Kind::OneDrive => ("OneDrive organisation", "OneDrive organisations"),
            Kind::Account => ("account name", "account names"),
            Kind::Sid => ("SID", "SIDs"),
            Kind::Name => ("PC, domain or user name", "PC, domain or user names"),
        };
        format!("{count} {}", if count == 1 { one } else { many })
    }
}

/// The copies a sanitize wrote, and what it replaced in them
#[derive(Clone, Debug)]
pub struct Sanitized {
    /// Each copy it wrote: the results file's, then the log's; for a file alone (`sanitize_single`), that file's
    pub copies: Vec<PathBuf>,
    /// How many of each kind it replaced, in the files together; none of a kind it found none of
    pub replaced: BTreeMap<Kind, usize>,
    /// For a file alone, what it couldn't remove without the other file (`RESULTS_ALONE`, `LOG_ALONE`); none for a pair
    pub alone: Option<&'static str>,
}

impl Sanitized {
    /// What the console and the window show: the copies, then what was replaced in them, and for a file alone what couldn't be
    pub fn text(&self) -> String {
        let one = self.copies.len() == 1;
        let mut text = if one {
            "🧹 Sanitized copy written, the original as it was:\n".to_string()
        } else {
            "🧹 Sanitized copies written, the originals as they were:\n".to_string()
        };
        for copy in &self.copies {
            text += &format!("   {}\n", copy.display());
        }
        if self.replaced.is_empty() {
            // What TMR looked for and found none of, never that the files name no one: a name TMR didn't write is none it can see (the
            // line after says so), and a file alone has a word only the other file would show to be a name (`alone` says what)
            text += if self.alone.is_some() {
                "   Nothing to replace that the file shows by itself\n"
            } else {
                "   Nothing found to replace: no serial, UUID or fingerprint, and no account, SID, user folder, OneDrive organisation or \
                 PC name\n"
            };
        } else {
            let kinds: Vec<String> = self.replaced.iter().map(|(kind, count)| kind.counted(*count)).collect();
            text += &format!("   Replaced with {REMOVED}: {}\n", kinds.join(", "));
        }
        if let Some(alone) = self.alone {
            text += &format!("   {alone}\n");
        }
        let read = if one { "it before you post it" } else { "them before you post them" };
        text += &format!("   Read {read}: a name TMR didn't write, in a config's name or a folder's, is still there\n");
        text
    }
}

/// Sanitized copies of the run `file` names, its results file or its log, written beside each, and what was replaced in them; or why
/// not, with nothing written.
pub fn sanitize(file: &str) -> Result<Sanitized, String> {
    let pair = Pair::of(file)?;
    let copies = [copy_of(&pair.results), copy_of(&pair.log)];
    for copy in &copies {
        not_there(copy, "the pair")?;
    }

    let scrubber = Scrubber::of_pair(&pair.json, &pair.results_text, &pair.log_text);
    let mut replaced = BTreeMap::new();
    let clean_results = scrubber.json(&pair.results_text, &mut replaced)?;
    let clean_log = scrubber.log(&pair.log_text, &mut replaced);
    // Read back as TMR reads them, the pair's run ID in both, before anything is written
    let back = read_back(&clean_results)?;
    let id = Some(pair.run_id.as_str());
    if back.metadata.run_id.as_deref() != id || crate::logfile::run_id_of(&clean_log) != id {
        return Err(LOST_RUN_ID.to_string());
    }
    write_new(&copies[0], &clean_results)?;
    if let Err(e) = write_new(&copies[1], &clean_log) {
        let _ = std::fs::remove_file(&copies[0]);
        return Err(e);
    }
    Ok(Sanitized { copies: copies.into(), replaced, alone: None })
}

/// A sanitized copy of `file`, a results file or a log, alone (`--sanitize-single`, the owner 2026-11-23): no partner looked for and no
/// run ID checked, the copy written beside it, and what was replaced in it; or why not, with nothing written. It removes what the pair's
/// sanitize would that the file shows by itself (a results file's values its own, a log's from TMR's lines for them, `Scrubber::of_log`),
/// and its outcome says what it couldn't (`Sanitized::alone`).
pub fn sanitize_single(file: &str) -> Result<Sanitized, String> {
    let (path, which) = located(file)?;
    let text = read(&path)?;
    let copy = copy_of(&path);
    let mut replaced = BTreeMap::new();
    let (clean, alone) = match which {
        RunFile::Results => {
            let (result, json) = results_of(&path, &text)?;
            not_there(&copy, "it")?;
            let clean = Scrubber::new(&json, &[&text]).json(&text, &mut replaced)?;
            if read_back(&clean)?.metadata.run_id != result.metadata.run_id {
                return Err(LOST_RUN_ID.to_string());
            }
            (clean, RESULTS_ALONE)
        }
        RunFile::Log => {
            not_there(&copy, "it")?;
            let clean = Scrubber::of_log(&text).log(&text, &mut replaced);
            if crate::logfile::run_id_of(&clean) != crate::logfile::run_id_of(&text) {
                return Err(LOST_RUN_ID.to_string());
            }
            (clean, LOG_ALONE)
        }
    };
    write_new(&copy, &clean)?;
    Ok(Sanitized { copies: vec![copy], replaced, alone: Some(alone) })
}

/// Which of a run's two files a path is, by its extension
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunFile {
    Results,
    Log,
}

/// The file `file` names, found as `--show-results` finds a results file (as given, or by its name in TMR's home's `results\`, and a log
/// in its `logs\`), and which of a run's it is; or why it is none to sanitize: neither kind, not there, or a sanitized copy already
fn located(file: &str) -> Result<(PathBuf, RunFile), String> {
    let extension = Path::new(file).extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
    let (folder, which) = match extension.as_deref() {
        Some("json") => (crate::home::results(), RunFile::Results),
        Some("log") => (crate::home::logs(), RunFile::Log),
        _ => return Err(format!("{file} is neither a results file (.json) nor a log (.log)")),
    };
    let found = crate::home::find(file, &folder).map_err(|looked| format!("{file} isn't there ({looked})"))?;
    let found = std::path::absolute(&found).unwrap_or(found);
    let stem = found.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| format!("{}: its name isn't text", found.display()))?;
    if stem.to_ascii_lowercase().ends_with(SANITIZED) {
        return Err(format!("{} is a sanitized copy already", found.display()));
    }
    Ok((found, which))
}

/// The text of the file at `path`, or why it can't be read as text
fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `text`, the results file at `path`'s, as TMR reads it and as JSON; or why it isn't one TMR reads
fn results_of(path: &Path, text: &str) -> Result<(TestRunResult, serde_json::Value), String> {
    let unreadable = |e: serde_json::Error| format!("{} isn't a results file TMR reads: {e}", path.display());
    Ok((serde_json::from_str(text).map_err(unreadable)?, serde_json::from_str(text).map_err(unreadable)?))
}

/// `clean`, a sanitized results file's text, read back as TMR reads one, before it is written; or why it doesn't
fn read_back(clean: &str) -> Result<TestRunResult, String> {
    serde_json::from_str(clean).map_err(|e| format!("the sanitized results file doesn't read back ({e}): nothing written"))
}

/// Nothing at `copy`, where a copy goes, or why the copy of `what` (the pair, it) isn't written
fn not_there(copy: &Path, what: &str) -> Result<(), String> {
    if copy.exists() {
        return Err(format!("{} is there already: move or delete it to sanitize {what} again", copy.display()));
    }
    Ok(())
}

/// `id`, the run ID the file at `path` has, or why it pairs with none: it has none, as a file from before TMR recorded one, or an empty
/// one, which two files could share without being one run's
fn run_id<'a>(path: &Path, id: Option<&'a str>) -> Result<&'a str, String> {
    match id {
        None => Err(format!("{} has no run ID, as files from before TMR recorded one: it can't be paired", path.display())),
        Some(id) if id.trim().is_empty() => Err(format!("{} has an empty run ID: it can't be paired", path.display())),
        Some(id) => Ok(id),
    }
}

/// A run's results file and its log, each read, the results file as JSON too, and the run ID they share
struct Pair {
    results: PathBuf,
    log: PathBuf,
    results_text: String,
    log_text: String,
    json: serde_json::Value,
    run_id: String,
}

impl Pair {
    /// The pair `file` names (`located`), its partner by the stem they share: the first of the places it is looked for in (TMR's layout,
    /// then the same folder) whose file is the same run's, so another run's file in one doesn't hide this run's in the other; or why there
    /// is none, each partner's reason said
    fn of(file: &str) -> Result<Pair, String> {
        let (found, which) = located(file)?;
        let (partner_extension, partner_folder, partner) = match which {
            RunFile::Results => ("log", "logs", "log"),
            RunFile::Log => ("json", "results", "results file"),
        };
        // Text: `located` checked it
        let stem = found.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
        let name = format!("{stem}.{partner_extension}");
        let here = found.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let places: Vec<PathBuf> =
            here.parent().map(|up| up.join(partner_folder).join(&name)).into_iter().chain([here.join(&name)]).collect();
        let partners: Vec<&PathBuf> = places.iter().filter(|place| place.is_file()).collect();
        if partners.is_empty() {
            let looked: Vec<String> = places.iter().map(|place| place.display().to_string()).collect();
            return Err(format!("{}'s {partner} isn't there: looked for {}; {ALONE_HINT}", found.display(), looked.join(" and ")));
        }
        // What the file named says by itself, whichever its partner: that it reads, is a results file TMR reads, and has a run ID
        let text = read(&found)?;
        let id = match which {
            RunFile::Results => results_of(&found, &text)?.0.metadata.run_id,
            RunFile::Log => crate::logfile::run_id_of(&text).map(str::to_string),
        };
        let id = run_id(&found, id.as_deref()).map_err(|e| format!("{e}; {ALONE_HINT}"))?;
        let mut refused = Vec::new();
        for partner in partners {
            match Pair::checked(&found, which, &text, id, partner) {
                Ok(pair) => return Ok(pair),
                Err(e) => refused.push(e),
            }
        }
        Err(format!("{}; {ALONE_HINT}", refused.join("; ")))
    }

    /// The pair of `found`, `which` of a run's files it is, its text `text` and its run ID `id`, and `partner`, when the partner reads, the
    /// results file of the two is one TMR reads, and the partner's run ID is `id`; or why not
    fn checked(found: &Path, which: RunFile, text: &str, id: &str, partner: &Path) -> Result<Pair, String> {
        let partner_text = read(partner)?;
        let ((results, results_text), (log, log_text)) = match which {
            RunFile::Results => ((found, text.to_string()), (partner, partner_text)),
            RunFile::Log => ((partner, partner_text), (found, text.to_string())),
        };
        let (result, json) = results_of(results, &results_text)?;
        let (results_id, log_id) = match which {
            RunFile::Results => (id, run_id(log, crate::logfile::run_id_of(&log_text))?),
            RunFile::Log => (run_id(results, result.metadata.run_id.as_deref())?, id),
        };
        if results_id != log_id {
            return Err(format!(
                "{} and {} are two runs': the results file's run ID is {results_id}, the log's {log_id}",
                results.display(),
                log.display()
            ));
        }
        let (results, log) = (results.to_path_buf(), log.to_path_buf());
        Ok(Pair { results, log, results_text, log_text, json, run_id: id.to_string() })
    }
}

/// The copy of `original` beside it: `<stem>.sanitized.<extension>`
fn copy_of(original: &Path) -> PathBuf {
    let stem = original.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    let extension = original.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    original.with_file_name(format!("{stem}{SANITIZED}.{extension}"))
}

/// Writes `text` to `path`, a new file only: never one there already, a link among them. A file it made but couldn't fill (a disk full,
/// a share gone) it removes, as nothing written says, rather than leave a part of a copy to be posted, which a sanitize again refuses.
fn write_new(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let failed = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(failed)?;
    if let Err(e) = file.write_all(text.as_bytes()).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(failed(e));
    }
    Ok(())
}

/// What both files' scrubbing replaces besides its rules: the run's values, and the names both files give
struct Scrubber {
    /// The run's values, each with its kind, replaced wherever they stand as words: in a log, and in a results file's free text
    values: Vec<(String, Kind)>,
    /// The accounts the log names, `PC\name`, replaced whole, before their parts
    accounts: Vec<String>,
    /// Their PC (or domain) and user names, the user folders' names, and the events' and shares' PC names, replaced as words where
    /// nothing else found them (in a results file, only in its free text), the longest first
    names: Vec<String>,
}

impl Scrubber {
    /// The values `json`, a results file, records, the words of the hardware it describes, and the names `texts`, its text (or its text
    /// and its log's), give
    fn new(json: &serde_json::Value, texts: &[&str]) -> Self {
        Self::with(labeled_values(json), &part_words(&json["identity"]), texts)
    }

    /// A pair's: the values `json`, its results file, records and those TMR's lines in `log` give (`log_values`), the words of the
    /// hardware both describe, and the names both texts give. Each file gets every rule a file alone would, its partner only adding to
    /// them: a value the results file has none of (an empty UUID) still goes from the log.
    fn of_pair(json: &serde_json::Value, results: &str, log: &str) -> Self {
        let mut values = labeled_values(json);
        values.extend(log_values(log));
        let mut parts = part_words(&json["identity"]);
        parts.extend(log_part_words(log, &values));
        Self::with(values, &parts, &[results, log])
    }

    /// A log's alone (`sanitize_single`), without a results file to name its values: those TMR's lines for them give (`log_values`),
    /// the words of the hardware those lines' (`log_part_words`), and the names it gives
    fn of_log(log: &str) -> Self {
        let values = log_values(log);
        Self::with(values.clone(), &log_part_words(log, &values), &[log])
    }

    /// `values`, the run's, each with its kind, and the names `texts` give, but a word of the hardware's description among them (`parts`)
    fn with(mut values: Vec<(String, Kind)>, parts: &BTreeSet<String>, texts: &[&str]) -> Self {
        values.retain(|(value, _)| informative(value));
        // The longest first, so none is replaced in part by a shorter one; each once, ASCII case aside
        values.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase())));
        values.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));

        let (mut accounts, mut names) = (Vec::new(), Vec::new());
        for text in texts {
            for account in accounts_in(text) {
                learn_account(account, &mut accounts, &mut names);
            }
            names.extend(user_folders(text).into_iter().map(|(start, end)| text[start..end].to_string()));
            for (start, end) in event_computers(text).into_iter().chain(share_hosts(text)) {
                learn_computer(&text[start..end], &mut names);
            }
        }
        for list in [&mut accounts, &mut names] {
            list.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())));
            list.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        }
        let no_ones = |name: &str| NO_ONES_NAMES.iter().chain(&SHARED_PROFILES).any(|no_one| name.eq_ignore_ascii_case(no_one));
        // A name that is a word of the machine's own description (a PC named RYZEN, on a Ryzen) tells no more than the parts do, and
        // replaced as a word it takes the CPU's name with it, in both files: a copy compared by its parts (`compare_identity`, as one
        // without fingerprints is) would read as another machine's. It goes only where an account, a path or an event names it.
        names.retain(|name| name.chars().count() >= 3 && !no_ones(name) && !parts.contains(&name.to_ascii_lowercase()));
        Scrubber { values, accounts, names }
    }

    /// `log`, a log's text, scrubbed: its SMBIOS string tables, the values its lines label (`labeled_spans`), then the run's values
    /// wherever else it has them, then what its calibration checks recorded, then the rest; all of it free text
    fn log(&self, log: &str, counts: &mut BTreeMap<Kind, usize>) -> String {
        // TMR writes these metadata lines itself. A learned account name such as "run" must never change their labels or pairing ID.
        let header_len = log
            .split_inclusive('\n')
            .take_while(|line| {
                ["# TMR ", "# build: ", "# paired result file: results/", "# run id: "].iter().any(|label| line.starts_with(label))
            })
            .map(str::len)
            .sum();
        let (header, body) = log.split_at(header_len);
        let text = by_line(body, |line| smbios_strings(line, counts));
        let text = replace_labeled(&text, &labeled_spans(&text), counts);
        let text = self.values(&text, counts);
        let text = by_line(&text, |line| recorded(line, counts));
        format!("{header}{}", self.rest(&text, counts, true))
    }

    /// `json`, a results file's text, with each of its strings scrubbed by its field, the rest as it was: a labeled field's value whole
    /// (`LABELED_FIELDS`), but a stand-in; free text (`FREE_TEXT_FIELDS`) as a log's text is; and TMR's own values by the rules'
    /// patterns only, never by a value or a name as a word. Why it couldn't be read as JSON, otherwise.
    fn json(&self, json: &str, counts: &mut BTreeMap<Kind, usize>) -> Result<String, String> {
        json_strings(json, |field, value| {
            if let Some((_, kind)) = LABELED_FIELDS.iter().find(|(labeled, _)| *labeled == field) {
                if stand_in(value) {
                    return value.to_string();
                }
                *counts.entry(*kind).or_default() += 1;
                return REMOVED.to_string();
            }
            let free = FREE_TEXT_FIELDS.contains(&field);
            let text = if free { self.values(value, counts) } else { value.to_string() };
            self.rest(&text, counts, free)
        })
    }

    /// `text` without the run's values
    fn values(&self, text: &str, counts: &mut BTreeMap<Kind, usize>) -> String {
        let mut text = text.to_string();
        for (value, kind) in &self.values {
            if let Some(scrubbed) = replace_words(&text, value, *kind, counts) {
                text = scrubbed;
            }
        }
        text
    }

    /// `text` without its accounts, its paths' user folders, OneDrive organisations and shares' PCs, its SIDs, its events' PC names, then,
    /// when it is `free` text, the names
    fn rest(&self, text: &str, counts: &mut BTreeMap<Kind, usize>, free: bool) -> String {
        let mut text = text.to_string();
        for account in &self.accounts {
            if let Some(scrubbed) = replace_words(&text, account, Kind::Account, counts) {
                text = scrubbed;
            }
        }
        let spans = user_folders(&text);
        text = replace_spans(&text, &spans, Kind::UserFolder, counts);
        let spans = onedrive_organisations(&text);
        text = replace_spans(&text, &spans, Kind::OneDrive, counts);
        let spans = library_organisations(&text);
        text = replace_spans(&text, &spans, Kind::OneDrive, counts);
        let spans = share_hosts(&text);
        text = replace_spans(&text, &spans, Kind::Name, counts);
        let spans = machine_sids(&text);
        text = replace_spans(&text, &spans, Kind::Sid, counts);
        let spans = event_computers(&text);
        text = replace_spans(&text, &spans, Kind::Name, counts);
        if free {
            for name in &self.names {
                if let Some(scrubbed) = replace_words(&text, name, Kind::Name, counts) {
                    text = scrubbed;
                }
            }
        }
        text
    }
}

/// The values `json`, a results file, has in its labeled fields (`LABELED_FIELDS`), each trimmed, with its kind
fn labeled_values(json: &serde_json::Value) -> Vec<(String, Kind)> {
    fn strings_at<'a>(value: &'a serde_json::Value, keys: &[&str], found: &mut Vec<&'a str>) {
        match (value, keys.split_first()) {
            (serde_json::Value::Array(items), _) => items.iter().for_each(|item| strings_at(item, keys, found)),
            (_, Some((key, rest))) => strings_at(&value[*key], rest, found),
            (serde_json::Value::String(text), None) => found.push(text),
            _ => {}
        }
    }
    let mut values = Vec::new();
    for (keys, kind) in LABELED_FIELDS {
        let mut found = Vec::new();
        strings_at(json, keys, &mut found);
        values.extend(found.into_iter().map(|value| (value.trim().to_string(), kind)));
    }
    values
}

/// Whether `value`, a serial, a UUID or a fingerprint, is a stand-in for one, naming no machine: a run's, in brackets
/// (`smbios::LEFT_OUT`, an earlier build's `(left out: --no-serials)`, the SMBIOS parse's `(none)`), `REMOVED`, what SMBIOS gives for
/// none (`PLACEHOLDERS`, a character repeated: `00000000`), or nothing. Its label keeps it, where any other value is replaced.
fn stand_in(value: &str) -> bool {
    let value = value.trim();
    let alphanumerics: Vec<char> = value.chars().filter(|c| c.is_alphanumeric()).collect();
    let placeholder = PLACEHOLDERS.iter().any(|placeholder| value.eq_ignore_ascii_case(placeholder));
    let repeated = alphanumerics.first().is_none_or(|first| alphanumerics.iter().all(|c| c == first));
    value.starts_with('(') || value == REMOVED || placeholder || repeated
}

/// Whether `value`, a serial, a UUID or a fingerprint the results file records (or a log's line shows), is replaced as a word wherever
/// else the files have it, as well as where its label has it: not a stand-in (`stand_in`), nor shorter than 4 letters and digits, which
/// would be found in words that aren't it (`A7` goes only where its label has it)
fn informative(value: &str) -> bool {
    !stand_in(value) && value.chars().filter(|c| c.is_alphanumeric()).count() >= 4
}

/// The accounts `text` names: the large-page check's (`USER_LINE`), and each beside a machine's or a domain's SID, `name (S-1-5-21-…)`,
/// after `to `, `, ` or `: ` (a grant's line, the security policy's holders)
fn accounts_in(text: &str) -> Vec<&str> {
    let mut accounts = Vec::new();
    for line in text.lines() {
        if let Some((_, account)) = line.split_once(USER_LINE) {
            accounts.push(account.trim());
        }
        let mut from = 0;
        while let Some(at) = line[from..].find(" (S-1-").map(|at| from + at) {
            if machine_sid_end(line, at + 2).is_some() {
                let before = &line[..at];
                let start = [" to ", ", ", ": "].iter().filter_map(|mark| before.rfind(mark).map(|i| i + mark.len())).max().unwrap_or(0);
                accounts.push(before[start..].trim());
            }
            from = at + 2;
        }
    }
    accounts
}

/// Whether `account`, as the large-page check names one, is someone's: not a built-in one (`BUILTIN\Administrators`), nor none
/// (`unknown (…)`, a SID that has no name, which the SIDs' rule takes)
fn names_someone(account: &str) -> bool {
    let built_in = |domain: &str| BUILT_IN_DOMAINS.iter().any(|built_in| domain.eq_ignore_ascii_case(built_in));
    let none = account.is_empty() || account.starts_with("S-1-") || account.starts_with("unknown (");
    !none && !account.split_once('\\').is_some_and(|(domain, _)| built_in(domain))
}

/// `account`, as the large-page check names one, into the accounts and names to replace, when it is someone's (`names_someone`); a
/// Microsoft or Entra ID account's domain not, as it names no PC
fn learn_account(account: &str, accounts: &mut Vec<String>, names: &mut Vec<String>) {
    if !names_someone(account) {
        return;
    }
    match account.split_once('\\') {
        Some((domain, user)) => {
            accounts.push(account.to_string());
            if !ACCOUNT_DOMAINS.iter().any(|named| domain.eq_ignore_ascii_case(named)) {
                names.push(domain.to_string());
            }
            names.push(user.to_string());
        }
        None => names.push(account.to_string()),
    }
}

/// `computer`, an event's or a share's PC name (`event_computers`, `share_hosts`), into the names to replace: as Windows gives it, its
/// full DNS name, then its host and its DNS domain (`corp.contoso.com`), but a domain of one label (`local`, `lan`), a word TMR's own
/// lines have; an IP address only whole, as its numbers are other words'
fn learn_computer(computer: &str, names: &mut Vec<String>) {
    let computer = computer.trim();
    names.push(computer.to_string());
    if computer.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return;
    }
    if let Some((host, domain)) = computer.split_once('.') {
        names.push(host.to_string());
        if domain.contains('.') {
            names.push(domain.to_string());
        }
    }
}

/// The words of the machine's description in `identity`, a results file's, in lower case: those of every string it has, the CPU's,
/// the BIOS', the board's and the modules' names among them, but the labeled values (`LABELED_FIELDS`)
fn part_words(identity: &serde_json::Value) -> BTreeSet<String> {
    let labeled = |key: &str| LABELED_FIELDS.iter().any(|(keys, _)| keys.last() == Some(&key));
    fn walk(value: &serde_json::Value, labeled: &dyn Fn(&str) -> bool, words: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::String(text) => {
                let apart = |c: char| c.is_ascii() && !word_byte(c as u8);
                words.extend(text.split(apart).filter(|word| !word.is_empty()).map(str::to_ascii_lowercase));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| walk(item, labeled, words)),
            serde_json::Value::Object(fields) => {
                fields.iter().filter(|(key, _)| !labeled(key)).for_each(|(_, value)| walk(value, labeled, words));
            }
            _ => {}
        }
    }
    let mut words = BTreeSet::new();
    walk(identity, &labeled, &mut words);
    words
}

/// `text` with `scrub` applied to each of its lines, their ends kept
fn by_line(text: &str, mut scrub: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        match scrub(line) {
            Some(scrubbed) => out.push_str(&scrubbed),
            None => out.push_str(line),
        }
    }
    out
}

/// `line` without its string table, when it is the SMBIOS parse's debug line for a structure (`strings=[…], next_offset=`) and the table
/// has strings
fn smbios_strings(line: &str, counts: &mut BTreeMap<Kind, usize>) -> Option<String> {
    const TABLE: &str = "strings=[";
    const AFTER: &str = "], next_offset=";
    if !line.contains("SMBIOS: Type ") {
        return None;
    }
    let start = line.find(TABLE)? + TABLE.len() - 1;
    let end = start + line[start..].find(AFTER)? + 1;
    if &line[start..end] == "[]" {
        return None;
    }
    *counts.entry(Kind::SmbiosStrings).or_default() += 1;
    Some(format!("{}{REMOVED}{}", &line[..start], &line[end..]))
}

/// `line` without the values tmr-cfg.json recorded, when it is a calibration's identity check (`RECORDED_LINES`): its `stored=` and its
/// `current=`, each up to a comma or the line's end, but a stand-in
fn recorded(line: &str, counts: &mut BTreeMap<Kind, usize>) -> Option<String> {
    if !RECORDED_LINES.iter().any(|(check, _)| line.contains(check)) {
        return None;
    }
    let mut out = line.to_string();
    for key in ["stored=", "current="] {
        let Some((start, end)) = field(&out, key, ',') else { continue };
        let value = out[start..end].trim();
        if !value.is_empty() && !value.starts_with('(') && value != REMOVED {
            out.replace_range(start..end, REMOVED);
            *counts.entry(Kind::Recorded).or_default() += 1;
        }
    }
    (out != line).then_some(out)
}

/// The value after `key` in `line`, as a byte range: up to `end` or the line's end, its spaces kept
fn field(line: &str, key: &str, end: char) -> Option<(usize, usize)> {
    let start = line.find(key)? + key.len();
    Some((start, line[start..].find([end, '\r', '\n']).map_or(line.len(), |at| start + at)))
}

/// TMR's lines in `log` that describe the machine: the console's for the system and each DIMM's below it (`SYSTEM_LINE`), the SMBIOS
/// parse's summary (`SMBIOS_LINE`) and the CPU's (`CPU_LINES`), each without its end, with where it starts in `log` and whether it is a
/// DIMM's
fn machine_lines(log: &str) -> Vec<(usize, &str, bool)> {
    let mut lines = Vec::new();
    let (mut at, mut below_system) = (0, false);
    for whole in log.split_inclusive('\n') {
        let line = whole.trim_end_matches(['\r', '\n']);
        let module = below_system && line.starts_with(MODULE_INDENT);
        below_system = module || line.contains(SYSTEM_LINE);
        if below_system || line.contains(SMBIOS_LINE) || CPU_LINES.iter().any(|cpu| line.contains(cpu)) {
            lines.push((at, line, module));
        }
        at += whole.len();
    }
    lines
}

/// The values TMR's own lines in `log` label, as byte ranges, each with its kind, but a stand-in (`stand_in`): each DIMM's serial, its
/// line's last field (`MODULE_SERIAL`); the system UUID, in the console's system line and the SMBIOS parse's summary; and the account
/// the large-page check names (`USER_LINE`), when it is someone's (`names_someone`). Each goes by its label whatever its length, in a
/// pair's log as in a log alone.
fn labeled_spans(log: &str) -> Vec<(usize, usize, Kind)> {
    let mut spans = Vec::new();
    let mut found = |at: usize, line: &str, span: Option<(usize, usize)>, kind: Kind| {
        let Some((start, end)) = span else { return };
        let value = &line[start..end];
        let start = start + (value.len() - value.trim_start().len());
        let end = end - (value.len() - value.trim_end().len());
        // An account is someone's by `names_someone`, which its line checked; a stand-in is a serial's or a UUID's
        if start < end && (kind == Kind::Account || !stand_in(&line[start..end])) {
            spans.push((at + start, at + end, kind));
        }
    };
    for (at, line, module) in machine_lines(log) {
        let serial = line.rfind(MODULE_SERIAL).filter(|_| module).map(|serial| (serial + MODULE_SERIAL.len(), line.len()));
        found(at, line, serial, Kind::Serial);
        if line.contains(SYSTEM_LINE) {
            found(at, line, field(line, SYSTEM_UUID, '·'), Kind::SystemUuid);
        }
        if line.contains(SMBIOS_LINE) {
            found(at, line, field(line, SMBIOS_UUID, ','), Kind::SystemUuid);
        }
    }
    let mut at = 0;
    for whole in log.split_inclusive('\n') {
        let line = whole.trim_end_matches(['\r', '\n']);
        if let Some(user) = line.find(USER_LINE).map(|user| user + USER_LINE.len())
            && names_someone(line[user..].trim())
        {
            found(at, line, Some((user, line.len())), Kind::Account);
        }
        at += whole.len();
    }
    spans.sort_unstable();
    spans
}

/// `text` with each of `spans` (byte ranges, apart, in order, each with its kind) replaced by `REMOVED`, counted by its kind
fn replace_labeled(text: &str, spans: &[(usize, usize, Kind)], counts: &mut BTreeMap<Kind, usize>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for &(start, end, kind) in spans {
        out.push_str(&text[last..start]);
        out.push_str(REMOVED);
        *counts.entry(kind).or_default() += 1;
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// The values a log names, each with its kind, by TMR's lines for them, as a results file would name them: those its lines label
/// (`labeled_spans`), but the account, and the run's own fingerprints and UUID, a calibration identity check's `current=`
/// (`RECORDED_LINES`). A stand-in among them is no value (`informative`).
fn log_values(log: &str) -> Vec<(String, Kind)> {
    let labeled = labeled_spans(log).into_iter().filter(|(_, _, kind)| *kind != Kind::Account);
    let mut values: Vec<(String, Kind)> = labeled.map(|(start, end, kind)| (log[start..end].to_string(), kind)).collect();
    for line in log.lines() {
        if let Some((_, kind)) = RECORDED_LINES.iter().find(|(check, _)| line.contains(check))
            && let Some((start, end)) = field(line, "current=", ',')
        {
            values.push((line[start..end].trim().to_string(), *kind));
        }
    }
    values
}

/// The words of the machine's description in `log`, as `part_words` gives a results file's: those of TMR's lines for it
/// (`machine_lines`), a record's after its time, level and target, but `values`, which `Scrubber` replaces
fn log_part_words(log: &str, values: &[(String, Kind)]) -> BTreeSet<String> {
    fn described(line: &str) -> &str {
        line.strip_prefix('[').and_then(|record| record.split_once("] ")).map_or(line, |(_, text)| text)
    }
    let values: Vec<&str> = values.iter().map(|(value, _)| value.as_str()).filter(|value| !value.is_empty()).collect();
    let without_values = |line: &str| values.iter().fold(described(line).to_string(), |line, value| line.replace(value, " "));
    let lines = machine_lines(log).into_iter().map(|(_, line, _)| serde_json::Value::String(without_values(line)));
    part_words(&serde_json::Value::Array(lines.collect()))
}

// A byte of a word, which a name found as a word doesn't run into: a letter or a digit, `_`, `-`, or any byte of a character beyond ASCII
fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || !b.is_ascii()
}

/// `text` with each occurrence of `word`, ASCII case aside, that no word byte runs into on either side replaced by `REMOVED` and counted as
/// `kind`; `None` when there is none
fn replace_words(text: &str, word: &str, kind: Kind, counts: &mut BTreeMap<Kind, usize>) -> Option<String> {
    let (bytes, wanted) = (text.as_bytes(), word.as_bytes());
    if wanted.is_empty() || wanted.len() > bytes.len() {
        return None;
    }
    let mut spans = Vec::new();
    let mut i = 0;
    while i + wanted.len() <= bytes.len() {
        let end = i + wanted.len();
        let alone = (i == 0 || !word_byte(bytes[i - 1])) && (end == bytes.len() || !word_byte(bytes[end]));
        // `word` starts with a character's first byte, so a match starts and ends on a character's edge
        if alone && bytes[i..end].eq_ignore_ascii_case(wanted) {
            spans.push((i, end));
            i = end;
        } else {
            i += 1;
        }
    }
    (!spans.is_empty()).then(|| replace_spans(text, &spans, kind, counts))
}

/// `text` with each of `spans` (byte ranges, in order, apart) replaced by `REMOVED`, counted as `kind`
fn replace_spans(text: &str, spans: &[(usize, usize)], kind: Kind, counts: &mut BTreeMap<Kind, usize>) -> String {
    let spans: Vec<(usize, usize, Kind)> = spans.iter().map(|&(start, end)| (start, end, kind)).collect();
    replace_labeled(text, &spans, counts)
}

/// A byte that ends a folder's name in a path: a separator, a character Windows allows in no file's or folder's name, or a line's end
fn ends_a_folder(b: u8) -> bool {
    matches!(b, b'\\' | b'/' | b'"' | b':' | b'|' | b'*' | b'?' | b'<' | b'>' | b'\r' | b'\n' | b'\t')
}

/// A byte that ends a user's name in a path: one that ends a folder's (`ends_a_folder`), or a character Windows allows in no user's name
/// (`[ ] ; = , +`), which an organisation's folder can have (`OneDrive - Smith, Jones & Co`)
fn ends_a_user(b: u8) -> bool {
    ends_a_folder(b) || matches!(b, b'[' | b']' | b';' | b'=' | b',' | b'+')
}

/// The name in `text` from `start`, a folder's in a path, as a byte range, up to what `ends` it. A separator or a quote closes it: it is
/// the folder's whole name, its spaces and punctuation among them. Otherwise it is a path's last folder, in a sentence or at a line's
/// end: up to an aside after it (` (`), and without the spaces and the punctuation it ends with, which no folder's name does.
fn folder_name(text: &str, start: usize, ends: fn(u8) -> bool) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut end = start;
    while end < bytes.len() && !ends(bytes[end]) {
        end += 1;
    }
    let mut name = &text[start..end];
    if !matches!(bytes.get(end), Some(b'\\' | b'/' | b'"')) {
        name = name.find(" (").map_or(name, |aside| &name[..aside]);
        name = name.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ';' | ')' | ']' | '\''));
    }
    (start, start + name.trim_end().len())
}

/// The user folders of `text`'s profile paths, as byte ranges: the name after `\Users\` (or `/Users/`, or a quoted path's `\\Users\\`),
/// but the shared ones (`Public`, `Default`, `All Users`) and one already replaced
fn user_folders(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let separator = |b: u8| b == b'\\' || b == b'/';
    let mut spans = Vec::new();
    let mut i = 1;
    while i + 6 <= bytes.len() {
        if separator(bytes[i - 1]) && bytes[i..i + 5].eq_ignore_ascii_case(b"users") && separator(bytes[i + 5]) {
            let mut start = i + 5;
            while start < bytes.len() && separator(bytes[start]) {
                start += 1;
            }
            let (start, end) = folder_name(text, start, ends_a_user);
            let name = &text[start..end];
            let shared = SHARED_PROFILES.iter().any(|shared| name.eq_ignore_ascii_case(shared));
            if !name.is_empty() && !shared && name != REMOVED {
                spans.push((start, end));
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    spans
}

/// The organisations of `text`'s OneDrive folders (`OneDrive - Contoso Ltd\`), as byte ranges, each up to what ends a folder's name
/// (`ends_a_folder`), but one already replaced
fn onedrive_organisations(text: &str) -> Vec<(usize, usize)> {
    const ONEDRIVE: &[u8] = b"onedrive - ";
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i + ONEDRIVE.len() <= bytes.len() {
        if bytes[i..i + ONEDRIVE.len()].eq_ignore_ascii_case(ONEDRIVE) {
            let (start, end) = folder_name(text, i + ONEDRIVE.len(), ends_a_folder);
            if start < end && &text[start..end] != REMOVED {
                spans.push((start, end));
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    spans
}

/// The organisations of the SharePoint libraries OneDrive syncs into `text`'s profiles (`\Users\<name>\<organisation>\<site> -
/// <library>\`, or with `/`, or escaped), as byte ranges: the folder after the user's, whole (`folder_name`), where the folder in it is
/// named as a library is (` - ` in it). Not a folder Windows makes in a profile (`PROFILE_FOLDERS`), OneDrive's own, nor one already
/// replaced; a folder of the user's in the profile with one so named in it goes too, as no path tells the two apart (review: the
/// organisation stayed, where `OneDrive - <organisation>` went).
fn library_organisations(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let separator = |b: u8| b == b'\\' || b == b'/';
    // The folder at `start` and the end of the separators after it, when one follows it
    let folder = |start: usize, ends: fn(u8) -> bool| {
        let (start, end) = if text[start..].starts_with(REMOVED) { (start, start + REMOVED.len()) } else { folder_name(text, start, ends) };
        let after = end + bytes[end..].iter().take_while(|&&b| separator(b)).count();
        (start, end, after)
    };
    let mut spans = Vec::new();
    let mut i = 1;
    while i + 6 <= bytes.len() {
        if !(separator(bytes[i - 1]) && bytes[i..i + 5].eq_ignore_ascii_case(b"users") && separator(bytes[i + 5])) {
            i += 1;
            continue;
        }
        let user = i + 5 + bytes[i + 5..].iter().take_while(|&&b| separator(b)).count();
        let (_, user_end, organisation) = folder(user, ends_a_user);
        let (start, end, library) = folder(organisation, ends_a_folder);
        i = end.max(i + 1);
        if organisation == user_end || library == end || start == end {
            continue;
        }
        let (name, (library_start, library_end, _)) = (&text[start..end], folder(library, ends_a_folder));
        let windows = PROFILE_FOLDERS.iter().any(|own| name.eq_ignore_ascii_case(own));
        let onedrive = name.get(..8).is_some_and(|start| start.eq_ignore_ascii_case("OneDrive"));
        if text[library_start..library_end].contains(" - ") && name != REMOVED && !windows && !onedrive {
            spans.push((start, end));
        }
    }
    spans
}

/// The PCs of `text`'s share paths, as byte ranges: the name after a path's opening `\\` (`\\PC\share`, or `\\\\PC\\share` as a
/// debug line escapes it, or after a device path's `\\?\UNC\`), or `//`, which Windows takes as well (`//PC/share`, review: typed so,
/// as a command line has it, the PC stayed), where a space, a quote, `=` or the like comes before it (`BEFORE_A_SHARE`), but this PC's
/// own (`OWN_HOSTS`). A name starts with a letter or a digit: not a device's path (`\\?\C:\`, `\\.\PhysicalDrive0`), nor one already
/// replaced; after `//`, a separator follows it, so `//TODO:` names none. A URL's `//` is none: a colon comes before it. Nor is an
/// escaped path's folder after one replaced (`"C:\\Users\\<removed>\\Documents"`, whose `>` came before a share's `\\`).
fn share_hosts(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let host_byte = |b: u8| word_byte(b) || b == b'.' || b == b'$';
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let slash = bytes[i];
        if slash != b'\\' && slash != b'/' {
            i += 1;
            continue;
        }
        let opening = i;
        while i < bytes.len() && bytes[i] == slash {
            i += 1;
        }
        // No escaped `/`: JSON and a debug line leave it as it is
        let width = i - opening;
        let widths: &[usize] = if slash == b'\\' { &[2, 4] } else { &[2] };
        if !widths.contains(&width) || (opening > 0 && !BEFORE_A_SHARE.contains(&bytes[opening - 1])) {
            continue;
        }
        // An escaped path's separator after a folder already replaced (`C:\\Users\\<removed>\\Documents`), whose `>` is no share's
        if text[..opening].ends_with(REMOVED) {
            continue;
        }
        // A device path's share, `\\?\UNC\PC\share`: its separators each half the opening's
        let separator = &text[opening..opening + width / 2];
        let mut start = i;
        for device in ["?", "."] {
            let unc = format!("{device}{separator}UNC{separator}");
            if text.get(i..i + unc.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(&unc)) {
                start = i + unc.len();
            }
        }
        let end = start + bytes[start..].iter().take_while(|&&b| host_byte(b)).count();
        let named = bytes.get(start).is_some_and(|&b| b.is_ascii_alphanumeric() || !b.is_ascii())
            && (slash == b'\\' || matches!(bytes.get(end), Some(b'/' | b'\\')));
        if named && !OWN_HOSTS.iter().any(|own| text[start..end].eq_ignore_ascii_case(own)) {
            spans.push((start, end));
        }
        i = end.max(i);
    }
    spans
}

/// Where the SID at `start` in `text` ends, when it is a machine's or a domain's: `S-1-`, an authority and 4 sub-authorities or more
/// (`S-1-5-21-<domain>-<rid>`, Entra ID's `S-1-12-1-…`), which no word byte runs into. Not a well-known SID (`S-1-5-32-544`, `S-1-5-18`).
fn machine_sid_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if !bytes[start..].starts_with(b"S-1-") || (start > 0 && word_byte(bytes[start - 1])) {
        return None;
    }
    let mut i = start + 4;
    let mut groups = 0;
    loop {
        let digits = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            return None;
        }
        groups += 1;
        match (bytes.get(i), bytes.get(i + 1)) {
            (Some(b'-'), Some(next)) if next.is_ascii_digit() => i += 1,
            _ => break,
        }
    }
    // The authority, then its sub-authorities
    (groups > 4 && bytes.get(i).is_none_or(|&b| !word_byte(b))).then_some(i)
}

/// The machines' and domains' SIDs in `text`, as byte ranges (`machine_sid_end`)
fn machine_sids(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find("S-1-").map(|at| from + at) {
        match machine_sid_end(text, start) {
            Some(end) => {
                spans.push((start, end));
                from = end;
            }
            None => from = start + 4,
        }
    }
    spans
}

/// The PC names in `text`'s Windows events, as byte ranges: each `<Computer>` element's text, which a WHEA event's XML has (the log gives
/// it at debug, and where Windows has no message for the event: `whea::read_record`), but one already replaced
fn event_computers(text: &str) -> Vec<(usize, usize)> {
    const OPEN: &str = "<Computer>";
    const CLOSE: &str = "</Computer>";
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find(OPEN).map(|at| from + at + OPEN.len()) {
        // Within the line, up to the element's end; `REMOVED` starts with `<`, so a name replaced already is none
        let Some(end) = text[start..].find(['<', '\n']).map(|at| start + at) else { break };
        if text[end..].starts_with(CLOSE) && !text[start..end].trim().is_empty() {
            spans.push((start, end));
        }
        from = end;
    }
    spans
}

/// `json` with each of its string values replaced by what `scrub` makes of it, given its field (the keys from the file's top to it, an
/// array's items each the array's field) and its value, decoded; re-encoded only where it changed: its layout, its keys, its numbers and
/// every other string as they were. Why a string in it isn't JSON's, otherwise.
fn json_strings(json: &str, mut scrub: impl FnMut(&[&str], &str) -> String) -> Result<String, String> {
    let bytes = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    // Each open object's key so far, `None` for an array
    let mut open: Vec<Option<String>> = Vec::new();
    let (mut i, mut last) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'{' => open.push(Some(String::new())),
            b'[' => open.push(None),
            b'}' | b']' => {
                open.pop();
            }
            _ => {}
        }
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < bytes.len() && bytes[i] != b'"' {
            // An escape's next byte is its own, a quote among them
            i += if bytes[i] == b'\\' { 2 } else { 1 };
        }
        let end = (i + 1).min(bytes.len());
        i = end;
        let literal = &json[start..end];
        let value: String = serde_json::from_str(literal).map_err(|e| format!("a string the results file has isn't JSON's ({e})"))?;
        // A key, which a colon follows, is TMR's own name for a field: its object's key from here
        if json[end..].trim_start().starts_with(':') {
            if let Some(Some(key)) = open.last_mut() {
                *key = value;
            }
            continue;
        }
        let field: Vec<&str> = open.iter().flatten().map(String::as_str).collect();
        let scrubbed = scrub(&field, &value);
        if scrubbed != value {
            out.push_str(&json[last..start]);
            out.push_str(&serde_json::to_string(&scrubbed).map_err(|e| e.to_string())?);
            last = end;
        }
    }
    out.push_str(&json[last..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::TempDir;

    const RUN: &str = "0123456789abcdef0123456789abcdef";
    const SERIALS: [&str; 2] = ["1A2B3C4D", "1A2B3C5E"];
    const UUID: &str = "4C4C4544-0042-3510-8051-B7C04F4D3332";
    const MACHINE: &str = "a1b2c3d4e5f60718";
    const MEMORY: &str = "0f1e2d3c4b5a6978";
    const STORED: &str = "fedcba9876543210";
    const SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
    const OTHER_SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1002";
    const BOARD_SERIAL: &str = "MB-77QX12345";
    const PROFILE: &str = r"C:\Users\Jamie Doe";
    /// What a sanitized copy has none of, ASCII case aside: every value above, and the names
    const GONE: [&str; 14] = [
        SERIALS[0], SERIALS[1], UUID, MACHINE, MEMORY, STORED, SID, OTHER_SID, BOARD_SERIAL, "jamie", "Jamie Doe", "DESKTOP-J4M13",
        "robin", "Contoso",
    ];
    /// What a sanitized log of `Run`'s with `--serials` keeps: what isn't the machine's or anyone's (a built-in group and its SID, the word
    /// Administrator, the parts), and its lines' shapes, each value in them `REMOVED`
    const KEPT: [&str; 10] = [
        r"BUILTIN\Administrators (S-1-5-32-544), <removed> (<removed>)",
        "Running with Administrator privileges",
        "0 Unknown); <removed>'s run on <removed>, memory_id <removed>",
        r"\OneDrive - <removed>\configs\x.cfg --serials",
        "👤 User: <removed>\n",
        "strings=<removed>, next_offset=171",
        "strings=[], next_offset=25\n",
        "stored=<removed>, current=<removed>\n",
        "G Skill F5-6000J3038F16G · serial <removed>\n",
        "# run id: 0123456789abcdef0123456789abcdef\n",
    ];

    /// A synthetic run, `stem`: its results file in `<dir>\results\` and its log in `<dir>\logs\` (or both in `<dir>`, `one_folder`), the
    /// log's run ID `log_id`, the results' `RUN`. With `kept`, as a run with `--serials` keeps them, every kind of value; without, as a run
    /// leaves them out, its log naming no account or path either. Every value is made up.
    struct Run {
        results: PathBuf,
        log: PathBuf,
    }

    impl Run {
        fn new(dir: &TempDir, stem: &str, log_id: &str, kept: bool, one_folder: bool) -> Run {
            let root = PathBuf::from(dir.file(""));
            let (results_dir, logs_dir) = if one_folder { (root.clone(), root.clone()) } else { (root.join("results"), root.join("logs")) };
            for folder in [&results_dir, &logs_dir] {
                std::fs::create_dir_all(folder).unwrap();
            }
            let (results, log) = (results_dir.join(format!("{stem}.json")), logs_dir.join(format!("{stem}.log")));
            let mut result = TestRunResult::new(Default::default());
            result.metadata.run_id = Some(RUN.to_string());
            result.metadata.filename = format!("results/{stem}.json");
            let module = |serial: &str| crate::smbios::MemoryModule {
                slot: "A-2".to_string(),
                manufacturer: "G Skill".to_string(),
                part_number: "F5-6000J3038F16G".to_string(),
                serial_number: serial.to_string(),
                capacity_human: "16.0 GB".to_string(),
                ..Default::default()
            };
            let left_out = crate::smbios::LEFT_OUT.to_string();
            result.identity = crate::run_context::RunIdentity {
                machine_id: if kept { MACHINE.to_string() } else { left_out.clone() },
                memory_id: if kept { MEMORY.to_string() } else { left_out },
                system_uuid: kept.then(|| UUID.to_string()),
                memory_modules: if kept { vec![module(SERIALS[0]), module(SERIALS[1])] } else { vec![module(""), module("")] },
                ..Default::default()
            };
            let config = format!(r"config={PROFILE}\OneDrive - Contoso Ltd\configs\x.cfg");
            result.run_config.command_line = ["tmr.exe", &config, "--serials"].map(String::from).to_vec();
            if !kept {
                result.run_config.command_line = ["tmr.exe", "plan=quick"].map(String::from).to_vec();
            }
            result.save_atomic(results.to_str().unwrap()).unwrap();
            let header = format!(
                "# TMR 0.1.2 — run started 2026-11-23 10:00:00 +11:00\n# paired result file: results/{stem}.json\n# run id: {log_id}\n"
            );
            let body = if kept {
                format!(
                    "[2026-11-23 10:00:00.001 INFO tmr_cli] 📝 Log file: {PROFILE}\\tmr\\logs\\{stem}.log (here, warnings and errors)\n\
                     \x20 Command Line: {PROFILE}\\tmr\\tmr.exe {config} --serials\n\
                     ✅ Running with Administrator privileges\n\
                     👤 User: DESKTOP-J4M13\\jamie\n\
                     [2026-11-23 10:00:00.002 INFO tmr::memory::privileges] The security policy gives 'Lock pages in memory' to \
                     BUILTIN\\Administrators (S-1-5-32-544), DESKTOP-J4M13\\robin ({OTHER_SID})\n\
                     [2026-11-23 10:00:00.003 INFO tmr::memory::privileges] Granting 'Lock pages in memory' to \
                     DESKTOP-J4M13\\jamie ({SID})\n\
                     [2026-11-23 10:00:00.004 DEBUG tmr::smbios] SMBIOS: Type 2 at offset 120, formatted_len=15, strings=[\"ASUSTeK\", \
                     \"PRIME B650M-A\", \"{BOARD_SERIAL}\"], next_offset=171, bytes_at_next=[03, 16]\n\
                     [2026-11-23 10:00:00.004 DEBUG tmr::smbios] SMBIOS: Type 16 at offset 0, formatted_len=23, strings=[], \
                     next_offset=25\n\
                     [2026-11-23 10:00:00.005 INFO tmr::smbios] SMBIOS: BIOS=AMI 3222, System=ASUS PRIME, UUID={UUID}, Memory=2 module(s)\n\
                     [2026-11-23 10:00:00.006 WARN tmr::app_config] Cache calibration: machine ID mismatch (CPU/cache/BIOS changed): \
                     stored={STORED}, current={MACHINE}\n\
                     🖥️  System: ASUS PRIME B650M-A · UUID {UUID} · 2 modules, 32 GiB\n\
                     \x20   A-2: 16 GiB DDR5 6000 MT/s · G Skill F5-6000J3038F16G · serial {}\n\
                     \x20   B-2: 16 GiB DDR5 6000 MT/s · G Skill F5-6000J3038F16G · serial {}\n\
                     [2026-11-23 10:00:00.007 INFO tmr::session] Topology: 4 physical cores (4 P-cores, 0 E-cores, 0 Unknown); jamie's \
                     run on DESKTOP-J4M13, memory_id {MEMORY}\n\
                     💾 Results saved: {PROFILE}\\tmr\\results\\{stem}.json\n",
                    SERIALS[0], SERIALS[1]
                )
            } else {
                let left_out = crate::smbios::LEFT_OUT;
                format!(
                    "✅ Running with Administrator privileges\n\
                     [2026-11-23 10:00:00.006 WARN tmr::app_config] Cache calibration: memory configuration changed (DIMM swap, XMP \
                     change, or capacity change): stored={left_out}, current={left_out}\n\
                     🖥️  System: ASUS PRIME B650M-A · 2 modules, 32 GiB · serials left out (--serials keeps them)\n\
                     \x20   A-2: 16 GiB DDR5 6000 MT/s · G Skill F5-6000J3038F16G\n"
                )
            };
            std::fs::write(&log, header + &body).unwrap();
            Run { results, log }
        }
    }

    #[test]
    fn an_account_named_run_keeps_the_pairing_header() {
        let dir = TempDir::new("sanitize-run-account");
        let pair = Run::new(&dir, "TMR_2026-11-24_090000", RUN, true, false);
        let original = read(&pair.log).replace("jamie", "run");
        std::fs::write(&pair.log, &original).unwrap();
        for paired in [true, false] {
            let copies =
                if paired { sanitize(pair.results.to_str().unwrap()) } else { sanitize_single(pair.log.to_str().unwrap()) }.unwrap();
            let log = read(copies.copies.iter().find(|p| p.extension().is_some_and(|ext| ext == "log")).unwrap());
            assert!(log.starts_with(original.split("[2026").next().unwrap()));
            assert!(!log.contains(r"DESKTOP-J4M13\run"));
            for path in copies.copies {
                std::fs::remove_file(path).unwrap();
            }
        }
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// What the files in `dir` and its `results\` and `logs\` are called, sorted
    fn files(dir: &TempDir) -> Vec<String> {
        let root = PathBuf::from(dir.file(""));
        let mut names: Vec<String> = [root.clone(), root.join("results"), root.join("logs")]
            .iter()
            .flat_map(|folder| std::fs::read_dir(folder).into_iter().flatten().flatten())
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The pair's copies `done` wrote: the results file's, then the log's
    fn pair(done: &Sanitized) -> (&Path, &Path) {
        match &done.copies[..] {
            [results, log] => (results, log),
            copies => panic!("not a pair's copies: {copies:?}"),
        }
    }

    /// What `done` replaced, by kind
    fn counts(done: &Sanitized) -> Vec<(Kind, usize)> {
        done.replaced.iter().map(|(kind, count)| (*kind, *count)).collect()
    }

    /// A run kept with `--serials`, then sanitized from its results file: the copies beside the originals, which stay as they were, have
    /// none of its values or names, each counted by kind, and keep what isn't the machine's or anyone's (a built-in group and its SID,
    /// the word Administrator, the parts); the results file reads as TMR reads one (`--show-results`, `--compare-results`)
    #[test]
    fn a_pair_loses_every_kind_of_value_and_still_reads_as_tmr_reads_it() {
        let dir = TempDir::new("sanitize-kept");
        let run = Run::new(&dir, "TMR_2026-11-23_10-00-00", RUN, true, false);
        let (results_before, log_before) = (read(&run.results), read(&run.log));
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pair(&done).0, run.results.with_file_name("TMR_2026-11-23_10-00-00.sanitized.json"));
        assert_eq!(pair(&done).1, run.log.with_file_name("TMR_2026-11-23_10-00-00.sanitized.log"));
        assert_eq!((read(&run.results), read(&run.log)), (results_before, log_before), "the originals as they were");

        let (results, log) = (read(pair(&done).0), read(pair(&done).1));
        for copy in [&results, &log] {
            let lower = copy.to_ascii_lowercase();
            for gone in GONE {
                assert!(!lower.contains(&gone.to_ascii_lowercase()), "{gone} is still there:\n{copy}");
            }
        }
        for kept in KEPT.into_iter().chain([r"Log file: C:\Users\<removed>\tmr\logs\TMR_2026-11-23_10-00-00.log (here"]) {
            assert!(log.contains(kept), "{kept} isn't in the log:\n{log}");
        }
        assert_eq!(counts(&done), [
            (Kind::Serial, 4),
            (Kind::SystemUuid, 3),
            (Kind::MachineId, 2),
            (Kind::MemoryId, 2),
            (Kind::Recorded, 1),
            (Kind::SmbiosStrings, 1),
            (Kind::UserFolder, 5),
            (Kind::OneDrive, 2),
            (Kind::Account, 3),
            (Kind::Sid, 2),
            (Kind::Name, 2),
        ]);

        let back: TestRunResult = serde_json::from_str(&results).expect("TMR reads the copy");
        assert_eq!((back.metadata.run_id.as_deref(), back.identity.machine_id.as_str()), (Some(RUN), REMOVED));
        assert_eq!(back.identity.memory_modules[0].serial_number, REMOVED);
        assert_eq!(back.run_config.command_line[1], r"config=C:\Users\<removed>\OneDrive - <removed>\configs\x.cfg");
        let copy = pair(&done).0.to_str().unwrap();
        crate::commands::saved_run(copy).unwrap_or_else(|e| panic!("--show-results: {e}"));
        crate::results::compare_test_results(run.results.to_str().unwrap(), copy).unwrap_or_else(|e| panic!("--compare-results: {e}"));
        let text = done.text();
        assert!(text.contains(&pair(&done).0.display().to_string()) && text.contains(&pair(&done).1.display().to_string()), "{text}");
        assert!(text.contains("Replaced with <removed>: 4 DIMM serials, 3 system UUIDs, "), "{text}");
    }

    /// The log names the pair as well as the results file does, by its path or, in TMR's layout, from the results file's folder; and a
    /// pair copied into one folder is found there
    #[test]
    fn the_log_names_the_pair_too_and_a_pair_in_one_folder_is_found() {
        let dir = TempDir::new("sanitize-from-log");
        let run = Run::new(&dir, "TMR_2026-11-23_10-00-01", RUN, true, false);
        let done = sanitize(run.log.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((pair(&done).0.parent(), pair(&done).1.parent()), (run.results.parent(), run.log.parent()));
        assert!(read(pair(&done).1).contains("👤 User: <removed>\n"));

        let dir = TempDir::new("sanitize-one-folder");
        let run = Run::new(&dir, "TMR_2026-11-23_10-00-02", RUN, true, true);
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pair(&done).1, run.log.with_file_name("TMR_2026-11-23_10-00-02.sanitized.log"));
    }

    /// Nothing is written without its partner, with another run's, with no run ID, with a file that isn't TMR's, or with one that is
    /// neither kind: each says why, and where the pair isn't one run's, that one file can be sanitized alone
    #[test]
    fn nothing_is_written_without_the_pair_of_one_run() {
        let dir = TempDir::new("sanitize-refused");
        let refused = |file: &Path| sanitize(file.to_str().unwrap()).map(|done| done.text()).expect_err("refused");

        let alone = Run::new(&dir, "TMR_alone", RUN, true, false);
        std::fs::remove_file(&alone.log).unwrap();
        let e = refused(&alone.results);
        assert!(e.contains("'s log isn't there: looked for ") && e.contains("TMR_alone.log") && e.ends_with(ALONE_HINT), "{e}");
        let e = refused(&alone.log);
        assert!(e.contains("TMR_alone.log isn't there"), "{e}");

        let other = Run::new(&dir, "TMR_other", "ffffffffffffffffffffffffffffffff", true, false);
        let e = refused(&other.results);
        assert!(e.contains(&format!("the results file's run ID is {RUN}, the log's ffffffffffffffffffffffffffffffff")), "{e}");
        assert!(e.ends_with(ALONE_HINT), "{e}");

        let old = Run::new(&dir, "TMR_old", RUN, true, false);
        let log = read(&old.log).replace(&format!("# run id: {RUN}\n"), "");
        std::fs::write(&old.log, log).unwrap();
        let e = refused(&old.results);
        assert!(e.contains("TMR_old.log has no run ID") && e.ends_with(ALONE_HINT), "{e}");
        // Two empty run IDs are no run's
        let empty = Run::new(&dir, "TMR_empty", "", true, false);
        std::fs::write(&empty.results, read(&empty.results).replace(&format!(r#""run_id": "{RUN}""#), r#""run_id": """#)).unwrap();
        assert_eq!(TestRunResult::load_from_file(empty.results.to_str().unwrap()).unwrap().metadata.run_id.as_deref(), Some(""));
        let e = refused(&empty.log);
        assert!(e.contains("TMR_empty.log has an empty run ID") && e.ends_with(ALONE_HINT), "{e}");

        let foreign = Run::new(&dir, "TMR_foreign", RUN, true, false);
        std::fs::write(&foreign.results, r#"{"name": "not TMR's"}"#).unwrap();
        assert!(refused(&foreign.log).contains("isn't a results file TMR reads"));
        assert!(refused(Path::new(&dir.file("notes.txt"))).contains("neither a results file (.json) nor a log (.log)"));

        assert!(files(&dir).iter().all(|name| !name.contains(".sanitized")), "{:?}", files(&dir));
    }

    /// A run that left its serials out, as a run does unless `--serials`, and names no account or path: nothing to replace, its stand-ins
    /// kept, and the copy reads as TMR reads one
    #[test]
    fn a_run_that_left_its_serials_out_has_nothing_to_replace() {
        let dir = TempDir::new("sanitize-left-out");
        let run = Run::new(&dir, "TMR_2026-11-23_10-00-03", RUN, false, false);
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(done.replaced.is_empty(), "{:?}", done.replaced);
        assert_eq!((read(pair(&done).0), read(pair(&done).1)), (read(&run.results), read(&run.log)));
        let back: TestRunResult = serde_json::from_str(&read(pair(&done).0)).expect("TMR reads the copy");
        assert_eq!(back.identity.machine_id, crate::smbios::LEFT_OUT);
        let text = done.text();
        assert!(text.contains("   Nothing found to replace: no serial, UUID or fingerprint, and no account, SID, "), "{text}");
    }

    /// The copies are never the originals: each its own name beside its original, never written over (a second sanitize is refused,
    /// the copies as they were), and a copy is no pair to sanitize again
    #[test]
    fn the_copies_are_never_the_originals_nor_written_over() {
        let dir = TempDir::new("sanitize-twice");
        let run = Run::new(&dir, "TMR_2026-11-23_10-00-04", RUN, true, false);
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(pair(&done).0 != run.results && pair(&done).1 != run.log);
        let copies = (read(pair(&done).0), read(pair(&done).1));
        let e = sanitize(run.log.to_str().unwrap()).expect_err("the copies are there");
        assert!(e.contains("TMR_2026-11-23_10-00-04.sanitized.json is there already"), "{e}");
        assert_eq!((read(pair(&done).0), read(pair(&done).1)), copies);
        let e = sanitize(pair(&done).0.to_str().unwrap()).expect_err("a copy");
        assert!(e.ends_with("TMR_2026-11-23_10-00-04.sanitized.json is a sanitized copy already"), "{e}");
    }

    /// `--sanitize-single` of a results file whose log isn't there: its copy beside it, the original as it was and no log written, has
    /// none of the values it records nor its user folder's name, and reads as TMR reads one. What only the log names stays: a user's name
    /// in a folder outside `\Users\`, which the pair's sanitize removes, as the log's account names it.
    #[test]
    fn a_results_file_alone_loses_its_values_and_still_reads_as_tmr_reads_it() {
        let dir = TempDir::new("sanitize-single-results");
        let run = Run::new(&dir, "TMR_2026-11-23_11-00-00", RUN, true, false);
        std::fs::remove_file(&run.log).unwrap();
        let before = read(&run.results);
        let done = sanitize_single(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(done.copies, [run.results.with_file_name("TMR_2026-11-23_11-00-00.sanitized.json")]);
        assert_eq!(read(&run.results), before, "the original as it was");
        assert_eq!(files(&dir), ["TMR_2026-11-23_11-00-00.json", "TMR_2026-11-23_11-00-00.sanitized.json"]);
        let copy = read(&done.copies[0]);
        for gone in [SERIALS[0], SERIALS[1], UUID, MACHINE, MEMORY, "Jamie Doe", "Contoso"] {
            assert!(!copy.to_ascii_lowercase().contains(&gone.to_ascii_lowercase()), "{gone} is still there:\n{copy}");
        }
        assert_eq!(counts(&done), [
            (Kind::Serial, 2),
            (Kind::SystemUuid, 1),
            (Kind::MachineId, 1),
            (Kind::MemoryId, 1),
            (Kind::UserFolder, 1),
            (Kind::OneDrive, 1),
        ]);
        let back: TestRunResult = serde_json::from_str(&copy).expect("TMR reads the copy");
        assert_eq!((back.metadata.run_id.as_deref(), back.identity.memory_modules[1].serial_number.as_str()), (Some(RUN), REMOVED));
        crate::commands::saved_run(done.copies[0].to_str().unwrap()).unwrap_or_else(|e| panic!("--show-results: {e}"));
        let text = done.text();
        assert!(text.starts_with("🧹 Sanitized copy written, the original as it was:\n   "), "{text}");
        assert!(text.contains(&format!("\n   {RESULTS_ALONE}\n   Read it before you post it: ")), "{text}");

        // A config in a folder named for the user, outside `\Users\`: alone, the name stays; with its log, which names the account, it goes
        let run = Run::new(&dir, "TMR_2026-11-23_11-00-01", RUN, true, false);
        let folder = read(&run.results).replace(r#""--serials""#, r#""config=D:\\jamie\\y.cfg""#);
        std::fs::write(&run.results, folder).unwrap();
        let done = sanitize_single(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(read(&done.copies[0]).contains(r#""config=D:\\jamie\\y.cfg""#), "a name only the log gives");
        std::fs::remove_file(&done.copies[0]).unwrap();
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(read(pair(&done).0).contains(r#""config=D:\\<removed>\\y.cfg""#), "the log's account names it");
    }

    /// `--sanitize-single` of a log whose results file isn't there, from a run with `--serials`: every serial, the UUID and both
    /// fingerprints go, found by TMR's lines for them and then wherever else the log has them, as do the accounts, names, paths and
    /// SIDs, each counted as the pair's sanitize counts them in the log. A value only a line of another kind has stays: there is no
    /// results file to name it.
    #[test]
    fn a_log_alone_loses_every_serial_the_uuid_and_the_fingerprints() {
        let dir = TempDir::new("sanitize-single-log");
        let run = Run::new(&dir, "TMR_2026-11-23_11-00-02", RUN, true, false);
        std::fs::remove_file(&run.results).unwrap();
        let without = read(&run.log);
        // The memory fingerprint, as TMR's lines have it: a calibration identity check's `current=`
        let log = format!(
            "{without}[2026-11-23 10:00:00.008 WARN tmr::app_config] Seal calibration: memory configuration changed (DIMM swap, XMP \
             change, or capacity change): stored=0a0b0c0d0e0f1011, current={MEMORY}\n"
        );
        std::fs::write(&run.log, &log).unwrap();
        let done = sanitize_single(run.log.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(done.copies, [run.log.with_file_name("TMR_2026-11-23_11-00-02.sanitized.log")]);
        assert_eq!(read(&run.log), log, "the original as it was");
        assert_eq!(files(&dir), ["TMR_2026-11-23_11-00-02.log", "TMR_2026-11-23_11-00-02.sanitized.log"]);
        let copy = read(&done.copies[0]);
        for gone in GONE.into_iter().chain(["0a0b0c0d0e0f1011"]) {
            assert!(!copy.to_ascii_lowercase().contains(&gone.to_ascii_lowercase()), "{gone} is still there:\n{copy}");
        }
        for kept in KEPT.into_iter().chain([
            r"Log file: C:\Users\<removed>\tmr\logs\TMR_2026-11-23_11-00-02.log (here",
            "UUID=<removed>, Memory=2 module(s)\n",
            "🖥️  System: ASUS PRIME B650M-A · UUID <removed> · 2 modules, 32 GiB\n",
        ]) {
            assert!(copy.contains(kept), "{kept} isn't in the log:\n{copy}");
        }
        assert_eq!(counts(&done), [
            (Kind::Serial, 2),
            (Kind::SystemUuid, 2),
            (Kind::MachineId, 1),
            (Kind::MemoryId, 2),
            (Kind::Recorded, 2),
            (Kind::SmbiosStrings, 1),
            (Kind::UserFolder, 4),
            (Kind::OneDrive, 1),
            (Kind::Account, 3),
            (Kind::Sid, 2),
            (Kind::Name, 2),
        ]);
        assert!(done.text().contains(&format!("\n   {LOG_ALONE}\n")), "{}", done.text());

        // Without the check's line, the memory fingerprint is only in a line of another kind: it stays
        let scrubbed = Scrubber::of_log(&without).log(&without, &mut BTreeMap::new());
        assert!(scrubbed.contains(&format!("memory_id {MEMORY}\n")) && !scrubbed.contains(SERIALS[0]), "{scrubbed}");
        // A PC named for a word of the hardware's, as its lines describe it, goes as the account, not from the CPU's name
        let log = "👤 User: RYZEN\\jamie\nCPU           AMD Ryzen 7 5700X 8-Core Processor (AuthenticAMD)\nOn RYZEN, jamie's run\n";
        assert_eq!(
            Scrubber::of_log(log).log(log, &mut BTreeMap::new()),
            "👤 User: <removed>\nCPU           AMD Ryzen 7 5700X 8-Core Processor (AuthenticAMD)\nOn RYZEN, <removed>'s run\n"
        );
    }

    /// `--sanitize-single` of a log from a run that left its serials out and names no account or path: nothing to replace, and the copy
    /// is still written, as the original; its results file, there, is neither looked at nor copied
    #[test]
    fn a_log_alone_without_serials_has_nothing_to_replace_and_is_still_copied() {
        let dir = TempDir::new("sanitize-single-left-out");
        let run = Run::new(&dir, "TMR_2026-11-23_11-00-03", RUN, false, false);
        let done = sanitize_single(run.log.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(done.replaced.is_empty(), "{:?}", done.replaced);
        assert_eq!(read(&done.copies[0]), read(&run.log));
        let names = ["TMR_2026-11-23_11-00-03.json", "TMR_2026-11-23_11-00-03.log", "TMR_2026-11-23_11-00-03.sanitized.log"];
        assert_eq!(files(&dir), names);
        let text = done.text();
        assert!(text.contains(&format!("\n   Nothing to replace that the file shows by itself\n   {LOG_ALONE}\n")), "{text}");
        assert!(!text.contains("no user, account or PC"), "{text}");
    }

    /// `--sanitize-single` of a results file from a run that left its serials out, its config in a folder named for the PC: nothing the
    /// file shows by itself to replace, so the PC's name, which only the log gives, stays, and the outcome never says the file names no PC
    #[test]
    fn a_file_alone_with_nothing_to_replace_never_says_it_names_no_pc() {
        let dir = TempDir::new("sanitize-single-nothing");
        let run = Run::new(&dir, "TMR_2026-11-23_11-00-04", RUN, false, false);
        std::fs::remove_file(&run.log).unwrap();
        let folder = read(&run.results).replace(r#""plan=quick""#, r#""config=D:\\DESKTOP-J4M13\\y.cfg""#);
        assert!(folder.contains("DESKTOP-J4M13"), "{folder}");
        std::fs::write(&run.results, &folder).unwrap();
        let done = sanitize_single(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(done.replaced.is_empty(), "{:?}", done.replaced);
        assert_eq!(read(&done.copies[0]), folder, "a name only the log gives");
        let text = done.text();
        assert!(text.contains(&format!("\n   Nothing to replace that the file shows by itself\n   {RESULTS_ALONE}\n")), "{text}");
        assert!(!text.contains("no user, account or PC"), "{text}");
    }

    /// `--sanitize-single` refuses a file that is neither kind, isn't there, isn't one TMR reads, isn't text, or is a copy already, and
    /// a copy there already, which it never writes over; each says why, with nothing written. It doesn't refuse what only a pair's
    /// sanitize does: a log without a run ID, or a results file whose log is another run's.
    #[test]
    fn a_file_alone_is_refused_only_for_what_it_is() {
        let dir = TempDir::new("sanitize-single-refused");
        let refused = |file: &Path| sanitize_single(file.to_str().unwrap()).map(|done| done.text()).expect_err("refused");
        assert!(refused(Path::new(&dir.file("notes.txt"))).contains("neither a results file (.json) nor a log (.log)"));
        assert!(refused(Path::new(&dir.file("nowhere.log"))).contains("nowhere.log isn't there"));
        let foreign = Run::new(&dir, "TMR_foreign", RUN, true, false);
        std::fs::write(&foreign.results, r#"{"name": "not TMR's"}"#).unwrap();
        assert!(refused(&foreign.results).contains("isn't a results file TMR reads"));
        std::fs::write(&foreign.log, b"# TMR \xff\xfe a log that isn't text\n").unwrap();
        assert!(refused(&foreign.log).starts_with(&format!("{}: ", foreign.log.display())));
        assert!(files(&dir).iter().all(|name| !name.contains(".sanitized")), "{:?}", files(&dir));

        let run = Run::new(&dir, "TMR_twice", RUN, true, false);
        let done = sanitize_single(run.log.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let copy = read(&done.copies[0]);
        assert!(refused(&run.log).ends_with("TMR_twice.sanitized.log is there already: move or delete it to sanitize it again"));
        assert_eq!(read(&done.copies[0]), copy, "not written over");
        assert!(refused(&done.copies[0]).ends_with("TMR_twice.sanitized.log is a sanitized copy already"));

        // What a pair's sanitize refuses, each file alone takes: a log from before the run ID, and a results file whose log is another run's
        let old = Run::new(&dir, "TMR_old", "ffffffffffffffffffffffffffffffff", true, false);
        std::fs::write(&old.log, read(&old.log).replace("# run id: ffffffffffffffffffffffffffffffff\n", "")).unwrap();
        assert!(sanitize(old.results.to_str().unwrap()).is_err());
        for file in [&old.log, &old.results] {
            sanitize_single(file.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    /// The rules on their own: a machine's or a domain's SID goes, a well-known one stays; a user folder with a space goes whole, a shared
    /// one stays, and one at a path's end stops at the sentence; a value SMBIOS gives for none, or a stand-in, isn't one to replace
    #[test]
    fn the_rules_find_what_identifies_and_only_that() {
        let sids = |text: &str| machine_sids(text).into_iter().map(|(start, end)| text[start..end].to_string()).collect::<Vec<_>>();
        assert_eq!(sids(&format!("{SID}, S-1-5-32-544, S-1-5-18, S-1-12-1-11-22-33-44 and xS-1-5-21-1-2-3-4")), [
            SID, "S-1-12-1-11-22-33-44"
        ]);
        assert!(sids("S-1-5-21-1-2-3x").is_empty(), "a word runs into it");
        let folders = |text: &str| user_folders(text).into_iter().map(|(start, end)| text[start..end].to_string()).collect::<Vec<_>>();
        assert_eq!(folders(r"C:\Users\Jamie Doe\tmr, D:/Users/sam/x, C:\Users\Public\x, C:\\Users\\kim\\y, C:\Users\lee (here)"), [
            "Jamie Doe", "sam", "kim", "lee"
        ]);
        for none in
            ["", "00000000", "FFFF-FFFF", "Not Specified", "unknown", "abc", crate::smbios::LEFT_OUT, "(left out: --no-serials)", REMOVED]
        {
            assert!(!informative(none), "{none}");
        }
        assert!(informative("01-2539-77D71F29") && informative(UUID) && informative(MACHINE));
        let mut counts = BTreeMap::new();
        assert_eq!(
            replace_words("jamie, jamie2 and Jamie's", "jamie", Kind::Name, &mut counts).unwrap(),
            "<removed>, jamie2 and <removed>'s"
        );
        assert_eq!(counts[&Kind::Name], 2);
        // An account named as TMR's own lines are: replaced as the account and in its path, not in every `tmr::` line
        let log = "👤 User: PC7\\tmr\n[2026-11-23 10:00:00.001 INFO tmr::session] C:\\Users\\tmr\\x.cfg on PC7\n";
        let scrubber = Scrubber::new(&serde_json::Value::Null, &[log]);
        assert_eq!(
            scrubber.log(log, &mut BTreeMap::new()),
            "👤 User: <removed>\n[2026-11-23 10:00:00.001 INFO tmr::session] C:\\Users\\<removed>\\x.cfg on <removed>\n"
        );
    }

    /// A WHEA event's XML names the PC by its full DNS name (`<Computer>`), which goes whatever the log's account says: with none (a
    /// run whose large-page question was answered), with an Entra ID one, which names no PC, and with a NetBIOS name, cut at 15
    /// characters; its host and its DNS domain go wherever else the log has them, but a domain of one label
    #[test]
    fn an_events_pc_name_goes_whatever_account_the_log_names() {
        let event = |computer: &str| {
            format!(
                "[2026-11-23 10:00:01.000 WARN tmr::whea] WHEA raw event XML (no formatted message available): <Event xmlns='http://\
                 schemas.microsoft.com/win/2004/08/events/event'><System><EventID Qualifiers='0'>47</EventID><Channel>System</Channel>\
                 <Computer>{computer}</Computer><Security/></System></Event>\n"
            )
        };
        for (user, computer) in [
            ("", "Daddy-Petz"),
            ("👤 User: AzureAD\\JamieDoe\n", "Daddy-Petz"),
            ("👤 User: JAMIES-GAMING-R\\jamie\n", "JAMIES-GAMING-RIG-2024"),
            ("", "Daddy-Petz.corp.contoso.com"),
        ] {
            let log = format!("{user}{}", event(computer));
            let scrubbed = Scrubber::new(&serde_json::Value::Null, &[&log]).log(&log, &mut BTreeMap::new());
            assert!(scrubbed.contains("<Computer><removed></Computer>"), "{scrubbed}");
            let lower = scrubbed.to_ascii_lowercase();
            assert!(!lower.contains("petz") && !lower.contains("jamie"), "{scrubbed}");
        }
        let log = format!("{}A config from \\\\Daddy-Petz\\share and files.corp.contoso.com\n", event("Daddy-Petz.corp.contoso.com"));
        let scrubbed = Scrubber::new(&serde_json::Value::Null, &[&log]).log(&log, &mut BTreeMap::new());
        assert!(scrubbed.ends_with("A config from \\\\<removed>\\share and files.<removed>\n"), "{scrubbed}");
        let log = format!("{}Each thread's memory is local to its node\n", event("Daddy-Petz.local"));
        let scrubbed = Scrubber::new(&serde_json::Value::Null, &[&log]).log(&log, &mut BTreeMap::new());
        assert!(
            scrubbed.ends_with("<Computer><removed></Computer><Security/></System></Event>\nEach thread's memory is local to its node\n")
        );
    }

    /// A PC named for a word of the machine's description (`RYZEN`, on a Ryzen) goes as the account and in its path, not as a word: the
    /// CPU's name stays in both files, and the copy, without its fingerprints, compares with the original by its parts as the same machine
    #[test]
    fn a_name_the_hardware_has_stays_in_the_hardwares_names() {
        let brand = "AMD Ryzen 7 5700X 8-Core Processor";
        let mut result = TestRunResult::new(Default::default());
        result.identity.machine_id = MACHINE.to_string();
        result.identity.memory_id = MEMORY.to_string();
        result.identity.machine.cpu_brand = brand.to_string();
        result.run_config.command_line = vec!["tmr.exe".to_string(), r"config=D:\ryzen\x.cfg".to_string()];
        let text = serde_json::to_string_pretty(&result).unwrap();
        let log = format!("👤 User: RYZEN\\jamie\nCPU: {brand}, the run's config C:\\Users\\jamie\\x.cfg\n");
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let scrubber = Scrubber::new(&json, &[&text, &log]);
        let mut counts = BTreeMap::new();
        let (copy, log) = (scrubber.json(&text, &mut counts).unwrap(), scrubber.log(&log, &mut counts));
        assert_eq!(log, format!("👤 User: <removed>\nCPU: {brand}, the run's config C:\\Users\\<removed>\\x.cfg\n"));
        let copy: TestRunResult = serde_json::from_str(&copy).unwrap();
        assert_eq!((copy.identity.machine.cpu_brand.as_str(), copy.identity.machine_id.as_str()), (brand, REMOVED));
        assert_eq!(copy.run_config.command_line[1], r"config=D:\ryzen\x.cfg", "a word the hardware has, not a path's user folder");
        let differences = crate::run_context::compare_identity(&result.identity, &copy.identity);
        assert!(differences.is_empty(), "{:?}", differences.iter().map(|d| &d.field).collect::<Vec<_>>());
    }

    /// A labeled value goes by its label whatever its length, in each file by that file's own label: a DIMM serial too short to be found
    /// as a word elsewhere (`A7`) in both files, a UUID only the log has (the results file's empty), which a pair's sanitize removes as a
    /// log's alone does, and the account the large-page check's line names, one of a name that identifies no one (`tmr`) among them. A
    /// stand-in stays (an empty UUID).
    #[test]
    fn a_labeled_value_goes_whatever_its_length_and_each_file_gets_every_rule() {
        let dir = TempDir::new("sanitize-labeled");
        let run = Run::new(&dir, "TMR_2026-11-23_12-00-00", RUN, true, false);
        let mut json: serde_json::Value = serde_json::from_str(&read(&run.results)).unwrap();
        json["identity"]["memory_modules"][0]["serial_number"] = "A7".into();
        json["identity"]["system_uuid"] = "".into();
        std::fs::write(&run.results, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        let log = read(&run.log).replace(SERIALS[0], "A7").replace(r"👤 User: DESKTOP-J4M13\jamie", r"👤 User: PC7\tmr");
        std::fs::write(&run.log, log).unwrap();
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let (results, log) = (read(pair(&done).0), read(pair(&done).1));
        let back: TestRunResult = serde_json::from_str(&results).expect("TMR reads the copy");
        let serials: Vec<&str> = back.identity.memory_modules.iter().map(|module| module.serial_number.as_str()).collect();
        assert_eq!((serials, back.identity.system_uuid.as_deref()), (vec![REMOVED, REMOVED], Some("")));
        for gone in ["serial A7", UUID, r"PC7\tmr"] {
            assert!(!log.contains(gone), "{gone} is still there:\n{log}");
        }
        for kept in ["A-2: 16 GiB DDR5 6000 MT/s · G Skill F5-6000J3038F16G · serial <removed>\n", "👤 User: <removed>\n"] {
            assert!(log.contains(kept), "{kept} isn't in the log:\n{log}");
        }
        assert_eq!((done.replaced[&Kind::Serial], done.replaced[&Kind::SystemUuid]), (4, 2));

        std::fs::remove_file(pair(&done).1).unwrap();
        let done = sanitize_single(run.log.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let alone = read(&done.copies[0]);
        assert!(!alone.contains("serial A7") && alone.contains("· serial <removed>\n    B-2"), "{alone}");
    }

    /// A path's user folder and OneDrive organisation go whole: a name with a space where no separator follows it (a path's end, a quoted
    /// path, a comma after it), and an organisation with the punctuation a folder's name may have. A sentence's aside after a path's end
    /// stays, as does what a user folder ends at that no user's name has.
    #[test]
    fn a_paths_user_folder_and_organisation_go_whole() {
        let rules_only = Scrubber::with(Vec::new(), &BTreeSet::new(), &[]);
        for (text, scrubbed) in [
            (r"home C:\Users\Jamie Doe", r"home C:\Users\<removed>"),
            (r#"config "C:\Users\Jamie Doe" given"#, r#"config "C:\Users\<removed>" given"#),
            (r"C:\Users\Jamie Doe, then", r"C:\Users\<removed>, then"),
            (r"C:\Users\lee (here).", r"C:\Users\<removed> (here)."),
            (r"C:\Users\x\OneDrive - Smith, Jones & Co\y.cfg", r"C:\Users\<removed>\OneDrive - <removed>\y.cfg"),
            (r"C:\Users\x\OneDrive - Contoso Ltd", r"C:\Users\<removed>\OneDrive - <removed>"),
            (r"C:\Users\x\OneDrive - Contoso; Fabrikam\y", r"C:\Users\<removed>\OneDrive - <removed>\y"),
            (r"C:\Users\x\OneDrive - [Contoso]+Fabrikam\y", r"C:\Users\<removed>\OneDrive - <removed>\y"),
            (r"C:\Users\x\OneDrive - Contoso Ltd (synced).", r"C:\Users\<removed>\OneDrive - <removed> (synced)."),
            // A SharePoint library OneDrive syncs, `<organisation>\<site> - <library>` in the profile (review), escaped and with `/`
            // too; not a folder of the profile's own, nor OneDrive's, nor one deeper in
            (r"C:\Users\x\Contoso Pty Ltd\IT Team - Documents\z.cfg", r"C:\Users\<removed>\<removed>\IT Team - Documents\z.cfg"),
            (r"C:\Users\x\Smith, Jones & Co\IT - Documents.", r"C:\Users\<removed>\<removed>\IT - Documents."),
            (r#""C:\\Users\\x\\Contoso\\IT - Docs\\z.cfg""#, r#""C:\\Users\\<removed>\\<removed>\\IT - Docs\\z.cfg""#),
            ("C:/Users/x/Contoso/IT - Docs/z.cfg", "C:/Users/<removed>/<removed>/IT - Docs/z.cfg"),
            (r"C:\Users\x\Documents\TMR - copy\z.cfg", r"C:\Users\<removed>\Documents\TMR - copy\z.cfg"),
            (r"C:\Users\x\OneDrive - Contoso\Site - Docs\z.cfg", r"C:\Users\<removed>\OneDrive - <removed>\Site - Docs\z.cfg"),
            (r"C:\Users\x\tmr\logs\TMR - 1.log", r"C:\Users\<removed>\tmr\logs\TMR - 1.log"),
            (r"C:\Users\x\Contoso\z.cfg", r"C:\Users\<removed>\Contoso\z.cfg"),
            // Escaped, as a debug line has a path: the folder after one replaced is no share's PC
            (r#""C:\\Users\\x\\Documents\\y.cfg""#, r#""C:\\Users\\<removed>\\Documents\\y.cfg""#),
        ] {
            assert_eq!(rules_only.log(text, &mut BTreeMap::new()), scrubbed, "{text}");
        }
    }

    /// A share's path names its PC (`\\PC\share`), which goes in either file and wherever else they have it, though no account names it
    /// (a run whose large-page question was answered); escaped, as a debug line has a path, too, and after a device path's `UNC`. A
    /// device's path, a drive's, an escaped relative one, a URL's and this PC's own (`localhost`, WSL's) name none. A pair with nothing to
    /// replace says what TMR looked for, never that the files name no one.
    #[test]
    fn a_shares_pc_goes_and_nothing_found_says_what_was_looked_for() {
        let hosts = |text: &str| share_hosts(text).into_iter().map(|(start, end)| text[start..end].to_string()).collect::<Vec<_>>();
        let named = r#"config=\\NAS-7\cfg\y.cfg, "\\\\DESKTOP-J4M13\\share\\y.cfg", \\?\UNC\files.corp.example\x (\\192.168.7.20\x)"#;
        assert_eq!(hosts(named), ["NAS-7", "DESKTOP-J4M13", "files.corp.example", "192.168.7.20"]);
        // With `/`, as Windows takes a share's path too (review: `config=//NAS-7/cfg/y.cfg` kept its PC in both copies)
        assert_eq!(hosts(r"config=//NAS-8/cfg/y.cfg, (//files.example/x) //10.0.0.9\x"), ["NAS-8", "files.example", "10.0.0.9"]);
        for none in [
            r"\\?\C:\x",
            r#""\\\\?\\C:\\x""#,
            r#""C:\\Users\\x\\y""#,
            r#"".\\configs\\x.cfg""#,
            r"\\.\PhysicalDrive0",
            r"\\localhost\c$\x",
            r"\\wsl$\Ubuntu\x",
            "xmlns='http://schemas.microsoft.com/win'",
            r"a\\b",
            "file://NAS/x",
            "// a comment",
            "//TODO: x",
            "a//b/c",
            "//localhost/c$/x",
            "///x/y",
        ] {
            assert!(hosts(none).is_empty(), "{none}: {:?}", hosts(none));
        }
        let log = "  Command Line: tmr.exe config=//NAS-7/cfg/y.cfg\n  Explicit Parameters: config=//NAS-7/cfg/y.cfg\nOn NAS-7\n";
        let scrubbed = Scrubber::new(&serde_json::Value::Null, &[log]).log(log, &mut BTreeMap::new());
        assert_eq!(scrubbed, log.replace("NAS-7", REMOVED), "the PC, and the name wherever else the log has it");

        let dir = TempDir::new("sanitize-share");
        let run = Run::new(&dir, "TMR_2026-11-23_12-00-01", RUN, false, false);
        let share = read(&run.results).replace(r#""plan=quick""#, r#""config=\\\\DESKTOP-J4M13\\share\\y.cfg""#);
        std::fs::write(&run.results, &share).unwrap();
        std::fs::write(&run.log, read(&run.log) + r"On DESKTOP-J4M13, a config from \\DESKTOP-J4M13\share\y.cfg" + "\n").unwrap();
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let (results, log) = (read(pair(&done).0), read(pair(&done).1));
        assert!(results.contains(r#""config=\\\\<removed>\\share\\y.cfg""#), "{results}");
        assert!(log.ends_with(&format!("{}\n", r"On <removed>, a config from \\<removed>\share\y.cfg")), "{log}");
        assert_eq!(counts(&done), [(Kind::Name, 3)]);
        let alone = run.results.with_file_name("TMR_2026-11-23_12-00-01.sanitized.json");
        std::fs::remove_file(&alone).unwrap();
        sanitize_single(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert!(read(&alone).contains(r#""config=\\\\<removed>\\share\\y.cfg""#), "alone too");

        let run = Run::new(&dir, "TMR_2026-11-23_12-00-02", RUN, false, false);
        let text = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}")).text();
        let nothing = "   Nothing found to replace: no serial, UUID or fingerprint, and no account, SID, user folder, OneDrive \
                       organisation or PC name\n   Read them before you post them: ";
        assert!(text.contains(nothing) && !text.contains("name no user"), "{text}");
    }

    /// A name learned from an account is replaced in the results file only where a person's text can be, its command line among them:
    /// never in TMR's own values. Accounts named `failed` and `huge` leave the verdict, which `--show-results` reads, and the page size as
    /// they were, and go from the config's path.
    #[test]
    fn a_learned_name_never_replaces_tmrs_own_values() {
        let dir = TempDir::new("sanitize-verdict");
        let run = Run::new(&dir, "TMR_2026-11-23_12-00-03", RUN, false, false);
        let mut json: serde_json::Value = serde_json::from_str(&read(&run.results)).unwrap();
        let stats = &mut json["overall_stats"];
        (stats["outcome"], stats["status"], stats["total_errors"]) = ("completed".into(), "failed".into(), 6.into());
        json["run_config"]["memory"]["max_page_size"] = "huge".into();
        json["run_config"]["command_line"][1] = r"config=D:\failed\huge.cfg".into();
        std::fs::write(&run.results, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        let accounts = format!("{}\nGranting 'Lock pages in memory' to {} ({SID})\n", r"👤 User: PC7\failed", r"PC7\huge");
        std::fs::write(&run.log, read(&run.log) + &accounts).unwrap();
        let head = |path: &Path| crate::commands::saved_run(path.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}")).head;
        assert!(head(&run.results).starts_with("❌ The run finished: failed, 6 errors"), "{}", head(&run.results));

        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let back: TestRunResult = serde_json::from_str(&read(pair(&done).0)).expect("TMR reads the copy");
        assert_eq!((back.overall_stats.status.as_deref(), back.run_config.memory.max_page_size.as_str()), (Some("failed"), "huge"));
        assert_eq!(back.run_config.command_line[1], r"config=D:\<removed>\<removed>.cfg");
        assert!(head(pair(&done).0).starts_with("❌ The run finished: failed, 6 errors"), "{}", head(pair(&done).0));
    }

    /// A partner of another run's where TMR's layout has it (`..\logs\`, `..\results\`) doesn't hide this run's beside the file (a pair
    /// copied together), from either file; with neither this run's, each is said
    #[test]
    fn another_runs_file_never_hides_this_runs_partner() {
        let other = "ffffffffffffffffffffffffffffffff";
        let dir = TempDir::new("sanitize-partner-log");
        let run = Run::new(&dir, "TMR_2026-11-23_12-00-04", RUN, true, false);
        let beside = run.results.with_extension("log");
        std::fs::copy(&run.log, &beside).unwrap();
        std::fs::write(&run.log, read(&run.log).replace(RUN, other)).unwrap();
        let done = sanitize(run.results.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pair(&done).1, beside.with_file_name("TMR_2026-11-23_12-00-04.sanitized.log"));

        let dir = TempDir::new("sanitize-partner-results");
        let run = Run::new(&dir, "TMR_2026-11-23_12-00-05", RUN, true, false);
        let beside = run.log.with_extension("json");
        std::fs::copy(&run.results, &beside).unwrap();
        std::fs::write(&run.results, read(&run.results).replace(RUN, other)).unwrap();
        let done = sanitize(run.log.to_str().unwrap()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pair(&done).0, beside.with_file_name("TMR_2026-11-23_12-00-05.sanitized.json"));

        std::fs::write(&beside, r#"{"name": "not TMR's"}"#).unwrap();
        std::fs::remove_file(pair(&done).0).unwrap();
        std::fs::remove_file(pair(&done).1).unwrap();
        let e = sanitize(run.log.to_str().unwrap()).expect_err("neither is this run's");
        assert!(e.contains(&format!("the results file's run ID is {other}, the log's {RUN}")), "{e}");
        assert!(e.contains(&format!("{} isn't a results file TMR reads", beside.display())) && e.ends_with(ALONE_HINT), "{e}");
    }
}
