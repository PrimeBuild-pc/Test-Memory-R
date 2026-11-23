//! TMR Application Configuration
//!
//! Persistent configuration file (`tmr-cfg.json`) stored in TMR's home, next to the executable (`home.rs`).
//! Holds machine identity and calibration results; the window's settings are window.json's (tmr-gui's `settings.rs`).
//! All fields are optional: missing fields use defaults. Unknown fields are read past and not kept: `AppConfig` has no catch-all, so the
//! next save drops them.
//!
//! Written whole (`atomic_file`), as a crash while it was written left half of one (forum feedback, 2026-11-23). One that is there but
//! isn't a config is kept aside, as `tmr-cfg.json.unreadable`, before anything is saved over it; one that can't be read isn't saved over
//! (`AppConfig::load_for_setup`, `AppConfig::save`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cache::SystemInfo;
use crate::calibration::{CalibrationResults, SealCalibration};
use crate::frontend::UnreadableConfig;
use crate::smbios::{MemoryModule, SmbiosData};

/// Readable CPU + BIOS identity snapshot stored alongside the hash.
/// When machine_id mismatches, these fields let you see exactly what changed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MachineIdentity {
    pub cpu_vendor: String,
    pub cpu_brand: String,
    pub cpu_family: u32,
    pub cpu_model: u32,
    pub cpu_stepping: u32,
    /// CPU base frequency in MHz (CPUID leaf 0x16 preferred, registry ~MHz fallback)
    pub cpu_base_mhz: u32,
    /// CPU microcode revision (e.g. "0x010003E0")
    pub cpu_microcode: String,
    pub physical_cores: usize,
    pub bios_vendor: String,
    pub bios_version: String,
    /// BIOS release date in ISO format (YYYY-MM-DD)
    pub bios_date: String,
    pub system_manufacturer: String,
    pub system_product: String,
}

impl MachineIdentity {
    /// Snapshot the readable identity of the current machine.
    ///
    /// Shared by [`AppConfig::update_identity`] (which stores it to gate calibration reuse) and [`crate::run_context::RunIdentity`] (which
    /// stamps it onto result files) so the two can never disagree about what "this machine" means.
    pub fn detect(system_info: &SystemInfo, smbios: &SmbiosData) -> Self {
        Self {
            cpu_vendor: system_info.cpu_vendor.clone(),
            cpu_brand: system_info.cpu_brand.clone(),
            cpu_family: system_info.cpu_family,
            cpu_model: system_info.cpu_model,
            cpu_stepping: system_info.cpu_stepping,
            cpu_base_mhz: AppConfig::get_cpu_base_frequency_mhz(smbios),
            cpu_microcode: smbios.cpu_microcode.clone(),
            physical_cores: system_info.physical_cores,
            bios_vendor: smbios.bios.vendor.clone(),
            bios_version: smbios.bios.version.clone(),
            bios_date: smbios.bios.release_date.clone(),
            system_manufacturer: smbios.system.manufacturer.clone(),
            system_product: smbios.system.product_name.clone(),
        }
    }
}

/// The machine a calibration was measured on, as `AppConfig::is_calibration_valid` checks it: its machine ID
/// (`AppConfig::generate_machine_id`: CPU, cache, BIOS), system UUID, memory configuration (`AppConfig::generate_memory_id`) and TSC
/// frequency. The config's identity is the cache calibrations' (`AppConfig::fingerprint`); a seal calibration keeps its own (TODO 99,
/// `SealCalibration::fingerprint`), so saving one kind never makes the other, measured on other hardware, pass as this machine's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MachineFingerprint {
    pub machine_id: Option<String>,
    pub system_uuid: Option<String>,
    pub memory_id: Option<String>,
    pub tsc_frequency_ghz: Option<f64>,
}

impl MachineFingerprint {
    /// This machine's.
    pub fn detect(system_info: &SystemInfo, smbios: &SmbiosData) -> Self {
        MachineFingerprint {
            machine_id: Some(AppConfig::generate_machine_id(system_info, smbios)),
            system_uuid: (!smbios.system.uuid.is_empty()).then(|| smbios.system.uuid.clone()),
            memory_id: Some(AppConfig::generate_memory_id(smbios)),
            tsc_frequency_ghz: Some(system_info.cache_info.tsc_frequency_ghz),
        }
    }

    /// Whether it is this machine's now. Granular: the machine ID must be there and match; the system UUID, memory configuration and TSC
    /// frequency must match where they were recorded. Each mismatch is logged, `what` naming the calibration it is for.
    pub fn matches(&self, system_info: &SystemInfo, smbios: &SmbiosData, what: &str) -> bool {
        let mut valid = true;

        // Check machine_id (CPU + cache + BIOS)
        match &self.machine_id {
            Some(stored_id) => {
                let current_id = AppConfig::generate_machine_id(system_info, smbios);
                if stored_id != &current_id {
                    log::warn!(
                        "{what}: machine ID mismatch (CPU/cache/BIOS changed): stored={}, current={}",
                        crate::smbios::shown_identifier(stored_id),
                        crate::smbios::shown_identifier(&current_id)
                    );
                    valid = false;
                }
            }
            None => {
                log::info!("{what}: no machine ID recorded, so not validated");
                return false;
            }
        }

        // Check system UUID (motherboard/VM instance identity)
        if let (Some(stored_uuid), Some(current_uuid)) = (&self.system_uuid, &Some(smbios.system.uuid.clone()))
            && !current_uuid.is_empty()
            && stored_uuid != current_uuid
        {
            log::warn!(
                "{what}: system UUID mismatch (VM migration or motherboard swap): stored={}, current={}",
                crate::smbios::shown_identifier(stored_uuid),
                crate::smbios::shown_identifier(current_uuid)
            );
            valid = false;
        }

        // Check memory configuration (DIMM swaps, XMP changes)
        if let Some(stored_mem_id) = &self.memory_id {
            let current_mem_id = AppConfig::generate_memory_id(smbios);
            if stored_mem_id != &current_mem_id {
                log::warn!(
                    "{what}: memory configuration changed (DIMM swap, XMP change, or capacity change): stored={}, current={}",
                    crate::smbios::shown_identifier(stored_mem_id),
                    crate::smbios::shown_identifier(&current_mem_id)
                );
                valid = false;
            }
        }

        // Check TSC frequency — both values should be snapped to nearest MHz now, but older configs may have raw floats. Use 0.01%
        // tolerance (100ppm) to filter measurement noise while catching real frequency changes (e.g. VM migration).
        if let Some(stored_tsc) = self.tsc_frequency_ghz {
            let current_tsc = system_info.cache_info.tsc_frequency_ghz;
            if stored_tsc > 0.0 {
                let drift_pct = ((current_tsc - stored_tsc) / stored_tsc * 100.0).abs();
                if drift_pct > 0.01 {
                    log::warn!(
                        "{what}: TSC frequency changed: stored={:.3} GHz, current={:.3} GHz (drift: {:.4}%)",
                        stored_tsc,
                        current_tsc,
                        drift_pct
                    );
                    valid = false;
                }
            }
        }

        valid
    }
}

/// Application-wide persistent configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Machine fingerprint — invalidates calibration when hardware changes.
    /// Generated from CPU vendor + brand + family/model/stepping + cache sizes + base freq + BIOS.
    #[serde(default)]
    pub machine_id: Option<String>,

    /// Readable identity fields for diagnosing what changed when machine_id mismatches.
    #[serde(default)]
    pub machine_identity: Option<MachineIdentity>,

    /// SMBIOS Type 1 System UUID (motherboard/VM instance identity).
    /// Separate from machine_id for granular mismatch reporting (e.g. VM migration).
    #[serde(default)]
    pub system_uuid: Option<String>,

    /// Memory configuration fingerprint — hash of per-DIMM serial, part, manufacturer, speed, capacity. Detects DIMM swaps, XMP changes,
    /// capacity changes.
    #[serde(default)]
    pub memory_id: Option<String>,

    /// Per-DIMM memory module details from SMBIOS Type 17.
    /// Stored for display; the hash is in `memory_id`.
    #[serde(default)]
    pub memory_modules: Option<Vec<MemoryModule>>,

    /// TSC frequency at full precision (GHz) when calibration was performed.
    /// Any change from this value triggers a warning (clock drift, VM migration, etc.).
    #[serde(default)]
    pub tsc_frequency_ghz: Option<f64>,

    /// The cache calibration (from --calibrate-cache)
    #[serde(default)]
    pub calibration: Option<CalibrationResults>,

    /// The seal's width, measured (from --calibrate-seal or --calibrate-all, TODO 99): what
    /// `seal-width=auto` runs on the machine it was measured on, which it keeps its own fingerprint
    /// of (`valid_seal_calibration`); the identity above is the cache calibrations'
    #[serde(default)]
    pub seal_calibration: Option<SealCalibration>,
    // What the window keeps between sessions (its last parameters, the chart shown or not, starting on open, its size) is window.json's
    // (tmr-gui's settings.rs, TODO 109), not this file's: this one is the engine's record of the machine (TODO 7, the owner, 2026-11-04)
}

const CONFIG_FILENAME: &str = "tmr-cfg.json";

/// A config file there but not taken (`AppConfig::parse`)
#[derive(Debug)]
enum ReadError {
    /// It couldn't be read: why
    Unread(String),
    /// What was read isn't a config: what the parse found
    NotAConfig(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unread(why) => write!(f, "couldn't be read ({why})"),
            Self::NotAConfig(why) => write!(f, "isn't a config ({why})"),
        }
    }
}

/// Why a config file that couldn't be read, `why`, isn't saved over (`AppConfig::save`, `load_to_save`)
fn unread(path: &Path, why: &str) -> String {
    format!("{} couldn't be read ({why}), so it isn't written over", path.display())
}

/// `path`, a file that isn't a config, copied to `<path>.unreadable`, over an older one, before a save writes over it: the user's to look
/// at, or delete. Where, or why it couldn't be: in an install for all users, a user can write tmr-cfg.json but make no file beside it
/// (`dist/tmr.iss`), and then the save writes over it. A copy, not a rename: a rename in its own folder needs no right to make a file
/// there, so it would work in that install, and leave no tmr-cfg.json the user could write, nor make, to save a calibration in.
fn keep_aside(path: &Path) -> Result<PathBuf, String> {
    let mut kept = path.as_os_str().to_owned();
    kept.push(".unreadable");
    let kept = PathBuf::from(kept);
    match std::fs::copy(path, &kept) {
        Ok(_) => {
            log::warn!("{} isn't a config: kept as {}", path.display(), kept.display());
            Ok(kept)
        }
        Err(e) => {
            log::warn!("{} isn't a config, and couldn't be kept as {} ({e}): a save writes over it", path.display(), kept.display());
            Err(e.to_string())
        }
    }
}

impl AppConfig {
    /// The config at `path`, for a look at it (`session::stored_calibration`, `--show-calibration`): the defaults when there is none, or
    /// when the file can't be read or isn't a config, which the log says. A run's setup, which may save over it, has `load_for_setup`, and
    /// a save from any other, `load_to_save`.
    pub fn load(path: &Path) -> Self {
        Self::read(path).unwrap_or_default()
    }

    /// The config at `path`, to be changed and saved back (`--calibrate-cache`, `--calibrate-seal`, the calibration a run measures):
    /// `load`'s, but a file that can't be read is the error `save` would give, not the defaults. A save from those would write them over
    /// the file once it could be read again (another program let go of it, during the save's question): its other calibration and the
    /// machine's identity lost, with no copy kept (review). One that isn't a config is the defaults, as for `save`, which keeps it aside.
    pub fn load_to_save(path: &Path) -> Result<Self, String> {
        match Self::read(path) {
            Err(ReadError::Unread(why)) => Err(unread(path, &why)),
            read => Ok(read.unwrap_or_default()),
        }
    }

    /// The config at `path` for a run's setup, which may save a calibration over it: `load`'s, and, when the file is there but couldn't be
    /// read or isn't a config, what was wrong with it, for the setup to say. One that isn't a config is kept aside first (`keep_aside`),
    /// so a calibration saved since loses nothing of it.
    pub fn load_for_setup(path: &Path) -> (Self, Option<UnreadableConfig>) {
        let unreadable = match Self::read(path) {
            Ok(config) => return (config, None),
            Err(ReadError::Unread(why)) => UnreadableConfig::Unread { path: path.to_path_buf(), why },
            Err(ReadError::NotAConfig(why)) => UnreadableConfig::NotAConfig { path: path.to_path_buf(), why, kept: keep_aside(path) },
        };
        (Self::default(), Some(unreadable))
    }

    /// `parse`, logged: the config, the defaults when there is none
    fn read(path: &Path) -> Result<Self, ReadError> {
        match Self::parse(path) {
            Ok(Some(config)) => {
                log::info!("Loaded app config from {}", path.display());
                Ok(config)
            }
            Ok(None) => {
                log::info!("No config file at {}: the defaults", path.display());
                Ok(Self::default())
            }
            Err(e) => {
                log::warn!("{} {e}: the defaults", path.display());
                Err(e)
            }
        }
    }

    /// The config at `path`; none when there is no file. Read as bytes, so a file that isn't text (what a crash can leave) isn't a
    /// config, rather than unread.
    fn parse(path: &Path) -> Result<Option<Self>, ReadError> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| ReadError::NotAConfig(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ReadError::Unread(e.to_string())),
        }
    }

    /// Saves the config, pretty-printed, whole (`atomic_file::write`). The file there is checked first, as a run's setup checks it
    /// (`load_for_setup`) and `load_to_save` checks it only in part: one that isn't a config is kept aside, and one that can't be read
    /// isn't written over. Only a calibration saves it: no cost to a test.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }
        match Self::parse(path) {
            Err(ReadError::Unread(why)) => return Err(unread(path, &why)),
            Err(ReadError::NotAConfig(_)) => drop(keep_aside(path)),
            Ok(_) => {}
        }

        let json = serde_json::to_string_pretty(self).map_err(|e| format!("Failed to serialize config: {}", e))?;
        crate::atomic_file::write(path, json.as_bytes())?;

        log::info!("Saved app config to {}", path.display());
        Ok(())
    }

    /// The config file's path: in TMR's home, beside the real exe or in `TMR_HOME` (`home.rs`)
    pub fn default_path() -> PathBuf {
        crate::home::dir().join(CONFIG_FILENAME)
    }

    /// Generate a machine ID from system information + BIOS info.
    /// Includes CPU vendor, brand, family/model/stepping, microcode, base frequency, cache sizes, core count, and BIOS vendor/version/date.
    /// Deterministic: same hardware always produces the same ID.
    pub fn generate_machine_id(system_info: &SystemInfo, smbios: &SmbiosData) -> String {
        let mut hasher = DefaultHasher::new();
        // CPU identity
        system_info.cpu_vendor.hash(&mut hasher);
        system_info.cpu_brand.hash(&mut hasher);
        system_info.cpu_family.hash(&mut hasher);
        system_info.cpu_model.hash(&mut hasher);
        system_info.cpu_stepping.hash(&mut hasher);
        smbios.cpu_microcode.hash(&mut hasher);
        // Cache topology
        system_info.cache_info.per_core_l1d.hash(&mut hasher);
        system_info.cache_info.per_core_l2.hash(&mut hasher);
        system_info.cache_info.l3_cache.hash(&mut hasher);
        system_info.physical_cores.hash(&mut hasher);
        // CPU base frequency: CPUID leaf 0x16 preferred, registry ~MHz fallback
        let base_mhz = Self::get_cpu_base_frequency_mhz(smbios);
        base_mhz.hash(&mut hasher);
        // BIOS identity (firmware updates change behavior)
        smbios.bios.vendor.hash(&mut hasher);
        smbios.bios.version.hash(&mut hasher);
        smbios.bios.release_date.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Get CPU base frequency in MHz.
    /// Tries CPUID leaf 0x16 first (nominal, power-state independent).
    /// Falls back to registry ~MHz (which reflects the OS-detected frequency).
    fn get_cpu_base_frequency_mhz(smbios: &SmbiosData) -> u32 {
        use raw_cpuid::CpuId;
        let cpuid = CpuId::new();
        let cpuid_mhz = cpuid.get_processor_frequency_info().map(|f| f.processor_base_frequency() as u32).unwrap_or(0);
        if cpuid_mhz > 0 {
            cpuid_mhz
        } else {
            // Fallback: registry ~MHz (set by Windows from TSC/ACPI during boot)
            smbios.cpu_base_mhz
        }
    }

    /// Generate memory configuration fingerprint from SMBIOS data.
    /// Hash of per-DIMM serial, part, manufacturer, speed, capacity.
    pub fn generate_memory_id(smbios: &SmbiosData) -> String {
        smbios.memory_fingerprint()
    }

    /// The machine the config's identity names, which the cache calibrations were measured on.
    pub fn fingerprint(&self) -> MachineFingerprint {
        MachineFingerprint {
            machine_id: self.machine_id.clone(),
            system_uuid: self.system_uuid.clone(),
            memory_id: self.memory_id.clone(),
            tsc_frequency_ghz: self.tsc_frequency_ghz,
        }
    }

    /// Check if stored calibration is valid for the current system: whether the config's identity is this machine's
    /// (`MachineFingerprint::matches`: machine_id, system_uuid, memory_id, TSC frequency, each mismatch logged). The cache calibrations go
    /// by it; a seal calibration by its own (`valid_seal_calibration`).
    pub fn is_calibration_valid(&self, system_info: &SystemInfo, smbios: &SmbiosData) -> bool {
        self.fingerprint().matches(system_info, smbios, "Calibration")
    }

    /// Update all identity fields to match the current system
    pub fn update_identity(&mut self, system_info: &SystemInfo, smbios: &SmbiosData) {
        let MachineFingerprint { machine_id, system_uuid, memory_id, tsc_frequency_ghz } = MachineFingerprint::detect(system_info, smbios);
        self.machine_id = machine_id;
        self.machine_identity = Some(MachineIdentity::detect(system_info, smbios));
        self.system_uuid = system_uuid;
        self.memory_id = memory_id;
        self.memory_modules = if smbios.memory_modules.is_empty() { None } else { Some(smbios.memory_modules.clone()) };
        self.tsc_frequency_ghz = tsc_frequency_ghz;
    }

    /// Update the machine ID to match the current system (legacy compat — prefer update_identity)
    pub fn update_machine_id(&mut self, system_info: &SystemInfo, smbios: &SmbiosData) {
        self.update_identity(system_info, smbios);
    }

    /// Stores a seal calibration measured on this machine, stamped with this machine's fingerprint (`valid_seal_calibration`). The config's
    /// identity is left alone: it is the cache calibrations', and taking this machine's would pass theirs, measured on other hardware, off
    /// as this machine's (TODO 99).
    pub fn set_seal_calibration(&mut self, mut seal: SealCalibration, system_info: &SystemInfo, smbios: &SmbiosData) {
        seal.fingerprint = MachineFingerprint::detect(system_info, smbios);
        self.seal_calibration = Some(seal);
    }

    /// The seal calibration a run on this machine takes, if any: one whose own fingerprint is this machine's now, whatever the config's
    /// identity (which a cache calibration's save rewrites).
    pub fn valid_seal_calibration(&self, system_info: &SystemInfo, smbios: &SmbiosData) -> Option<&SealCalibration> {
        self.seal_calibration.as_ref().filter(|seal| seal.fingerprint.matches(system_info, smbios, "Seal calibration"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.machine_id.is_none());
        assert!(config.calibration.is_none());
    }

    #[test]
    fn test_roundtrip_empty_config() {
        let config = AppConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(loaded.machine_id.is_none());
        assert!(loaded.calibration.is_none());
    }

    #[test]
    fn test_roundtrip_with_fields() {
        let config = AppConfig { machine_id: Some("abc123".to_string()), ..Default::default() };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.machine_id.as_deref(), Some("abc123"));
    }

    /// An unknown field is read past, and not kept: the next save drops it (the module's doc)
    #[test]
    fn unknown_fields_are_read_past_and_dropped() {
        let json = r#"{"machine_id": "test", "some_future_field": 42}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.machine_id.as_deref(), Some("test"));
        assert!(!serde_json::to_string(&config).unwrap().contains("some_future_field"));
    }

    #[test]
    fn test_load_missing_file() {
        let config = AppConfig::load(std::path::Path::new("nonexistent_tmr_cfg.json"));
        assert!(config.machine_id.is_none());
        let (config, unreadable) = AppConfig::load_for_setup(Path::new("nonexistent_tmr_cfg.json"));
        assert!(config.machine_id.is_none() && unreadable.is_none(), "not there is the defaults, and nothing to say");
    }

    /// A config saved, and a temp a crashed save left: the save replaces the file whole, past the temp, and leaves none
    #[test]
    fn a_save_is_written_whole() {
        let dir = crate::results::TempDir::new("app-config-save");
        let path = PathBuf::from(dir.file("tmr-cfg.json"));
        std::fs::write(dir.file("tmr-cfg.json.tmp"), b"{\"machine_id\": \"half of a").unwrap();
        AppConfig { machine_id: Some("saved".to_string()), ..Default::default() }.save(&path).unwrap();
        let (config, unreadable) = AppConfig::load_for_setup(&path);
        assert_eq!((config.machine_id.as_deref(), unreadable), (Some("saved"), None));
        assert!(!Path::new(&dir.file("tmr-cfg.json.tmp")).exists());
    }

    /// A tmr-cfg.json that isn't a config, as a crash while it was written in place leaves one: a run's setup says so, and keeps a copy
    /// as tmr-cfg.json.unreadable, over an older one, so the calibration it saves loses nothing of it; a save it didn't read for
    /// (`--calibrate-cache`) keeps it aside too. One that can't be read is said as such, not as no file, and isn't written over; a command
    /// that saves is refused at its load (`load_to_save`), so it has no defaults to save once the file can be read again.
    #[test]
    fn a_config_that_isnt_one_is_kept_aside_and_one_unread_isnt_written_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = crate::results::TempDir::new("app-config-unreadable");
        let path = PathBuf::from(dir.file("tmr-cfg.json"));
        let kept = PathBuf::from(dir.file("tmr-cfg.json.unreadable"));
        let half = b"{\n  \"machine_id\": \"0123456789abcdef\",\n  \"seal_calibration\": {\n    \"width_bits\": 2";
        std::fs::write(&path, half).unwrap();
        std::fs::write(&kept, b"an older one").unwrap();
        let (config, unreadable) = AppConfig::load_for_setup(&path);
        assert!(config.machine_id.is_none() && config.seal_calibration.is_none(), "the defaults");
        let Some(UnreadableConfig::NotAConfig { why, kept: Ok(at), .. }) = &unreadable else { panic!("{unreadable:?}") };
        assert!(why.contains("EOF while parsing"), "{why}");
        assert_eq!((at, std::fs::read(&kept).unwrap(), path.exists()), (&kept, half.to_vec(), true), "kept, over the older one");
        let text = unreadable.as_ref().unwrap().text();
        assert_eq!(
            text,
            format!("tmr-cfg.json was unreadable ({why}), so its calibrations are lost; it is kept as tmr-cfg.json.unreadable"),
            "the setup's line"
        );
        AppConfig { machine_id: Some("measured".to_string()), ..Default::default() }.save(&path).unwrap();
        assert_eq!((AppConfig::load(&path).machine_id.as_deref(), std::fs::read(&kept).unwrap()), (Some("measured"), half.to_vec()));

        // Not text at all, as a crash can leave the file: not a config, and a save it wasn't read for keeps it aside first
        std::fs::write(&path, [0u8, 0, 0xFF, 0xFE]).unwrap();
        assert!(AppConfig::load_to_save(&path).unwrap().machine_id.is_none(), "the defaults, for the save to keep it aside");
        AppConfig { machine_id: Some("again".to_string()), ..Default::default() }.save(&path).unwrap();
        assert_eq!((AppConfig::load(&path).machine_id.as_deref(), std::fs::read(&kept).unwrap()), (Some("again"), vec![0, 0, 0xFF, 0xFE]));

        // Held by a program that shares it with no one: unread, said so, left as it is and not written over
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let (_, unreadable) = AppConfig::load_for_setup(&path);
        let Some(UnreadableConfig::Unread { why, .. }) = &unreadable else { panic!("{unreadable:?}") };
        assert!(unreadable.as_ref().unwrap().text().starts_with(&format!("tmr-cfg.json couldn't be read ({why})")));
        let refused = AppConfig::default().save(&path).unwrap_err();
        assert!(refused.contains("isn't written over"), "{refused}");
        assert_eq!(AppConfig::load_to_save(&path).unwrap_err(), refused, "refused at the load");
        drop(held);
        assert_eq!(AppConfig::load(&path).machine_id.as_deref(), Some("again"), "as it was");
        assert_eq!(AppConfig::load_to_save(&path).unwrap().machine_id.as_deref(), Some("again"));
    }

    /// In an install for all users, a user may write tmr-cfg.json but make no file beside it (`dist/tmr.iss`): a save writes it in place,
    /// and one that isn't a config can't be kept aside, which the setup says, and is saved over. A rename in the folder would have kept it,
    /// as it needs no right to make a file there, and left no tmr-cfg.json to save in (`keep_aside`).
    #[test]
    fn where_no_file_can_be_made_beside_it_the_config_is_written_in_place() {
        let dir = crate::results::TempDir::new("app-config-program-files");
        let folder = PathBuf::from(dir.file("TMR"));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("tmr-cfg.json");
        std::fs::write(&path, b"{}").unwrap();
        let _locked = crate::atomic_file::NoNewFiles::on(&folder);
        AppConfig { machine_id: Some("in place".to_string()), ..Default::default() }.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).machine_id.as_deref(), Some("in place"));

        std::fs::write(&path, b"{\"machine_id\": ").unwrap();
        let (_, unreadable) = AppConfig::load_for_setup(&path);
        let Some(UnreadableConfig::NotAConfig { kept: Err(_), .. }) = &unreadable else { panic!("{unreadable:?}") };
        assert!(unreadable.as_ref().unwrap().text().ends_with("so the new one is saved over it"));
        AppConfig { machine_id: Some("over it".to_string()), ..Default::default() }.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).machine_id.as_deref(), Some("over it"));
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "no temp, nothing kept aside");
    }

    #[test]
    fn test_roundtrip_identity_fields() {
        use crate::smbios::MemoryModule;

        let config = AppConfig {
            machine_id: Some("abc123def456".to_string()),
            system_uuid: Some("12345678-1234-1234-1234-123456789ABC".to_string()),
            memory_id: Some("fedcba9876543210".to_string()),
            tsc_frequency_ghz: Some(2.496012345),
            memory_modules: Some(vec![{
                let cap: u64 = 32 * 1024 * 1024 * 1024;
                MemoryModule {
                    locator: "DIMM_A1".to_string(),
                    manufacturer: "UnknownMemoryMaker".to_string(),
                    serial_number: "12345678".to_string(),
                    part_number: "123456789-12345".to_string(),
                    configured_mts: 4800,
                    memory_type: 0x22,
                    capacity_bytes: cap,
                    capacity_human: crate::smbios::format_capacity(cap),
                    ..Default::default()
                }
            }]),
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.system_uuid.as_deref(), Some("12345678-1234-1234-1234-123456789ABC"));
        assert_eq!(loaded.memory_id.as_deref(), Some("fedcba9876543210"));
        assert_eq!(loaded.tsc_frequency_ghz, Some(2.496012345));
        assert_eq!(loaded.memory_modules.as_ref().unwrap().len(), 1);
        assert_eq!(loaded.memory_modules.as_ref().unwrap()[0].locator, "DIMM_A1");
        assert_eq!(loaded.memory_modules.as_ref().unwrap()[0].configured_mts, 4800);
    }

    use crate::calibration::test_seal_calibration;

    fn this_machine() -> (&'static SystemInfo, &'static SmbiosData) {
        (crate::tests::get_system_info(), crate::smbios::get_smbios())
    }

    /// The width `auto` resolves to with `config`'s seal calibration, on a CPU with AVX-512 (where the rule is 256 and every width runs),
    /// and where it came from.
    fn resolve(config: &AppConfig, width: crate::seal::SealWidth) -> (crate::seal::SealWidth, crate::seal::SealWidthSource) {
        let (system_info, smbios) = this_machine();
        let calibrated = config.valid_seal_calibration(system_info, smbios).and_then(SealCalibration::width);
        width.resolve_on(calibrated, true, true, "seal-width=").unwrap()
    }

    /// TODO 99: `seal-width=auto` runs the seal calibration measured on this machine, stamped with this machine's fingerprint when it is
    /// saved, else the rule. A width set wins over a calibrated one.
    #[test]
    fn auto_takes_this_machines_seal_calibration_and_a_setting_wins() {
        use crate::seal::SealWidth::{Auto, W128, W256, W512};
        use crate::seal::SealWidthSource::{Calibrated, Rule, Setting};
        let (system_info, smbios) = this_machine();

        // Absent: the rule, on a config that is this machine's
        let mut config = AppConfig::default();
        config.update_identity(system_info, smbios);
        assert_eq!(resolve(&config, Auto), (W256, Rule));
        assert_eq!(resolve(&config, W512), (W512, Setting("seal-width=")));

        // Present, measured here: the calibrated width, which a setting overrides
        config.set_seal_calibration(test_seal_calibration(128), system_info, smbios);
        assert_eq!(config.seal_calibration.as_ref().unwrap().fingerprint, MachineFingerprint::detect(system_info, smbios));
        assert_eq!(resolve(&config, Auto), (W128, Calibrated));
        assert_eq!(resolve(&config, W256), (W256, Setting("seal-width=")));
        // A cache calibration saved since, on the same machine, keeps it
        config.update_machine_id(system_info, smbios);
        assert_eq!(resolve(&config, Auto), (W128, Calibrated));
        // ...as does a config whose identity names no machine (no cache calibration saved yet)
        let fresh = AppConfig { seal_calibration: config.seal_calibration.clone(), ..Default::default() };
        assert_eq!(resolve(&fresh, Auto), (W128, Calibrated));
    }

    /// TODO 99: saving a seal calibration leaves the config's identity, the cache calibrations', alone: one measured on other hardware
    /// stays stale when the seal is calibrated here (the reviewer's case: a CPU swap, then only `--calibrate-seal`, or `--calibrate-all`
    /// with the cache save declined).
    #[test]
    fn a_seal_save_leaves_a_stale_cache_calibration_stale() {
        use crate::calibration::{CalibrationResults, CpuSignature};
        let (system_info, smbios) = this_machine();
        let mut config = AppConfig { calibration: Some(CalibrationResults::new(CpuSignature::default())), ..Default::default() };
        config.update_identity(system_info, smbios);
        config.machine_id = Some("another machine".to_string());
        assert!(!config.is_calibration_valid(system_info, smbios));
        let identity = serde_json::to_value(&config).unwrap();

        config.set_seal_calibration(test_seal_calibration(128), system_info, smbios);
        assert!(config.valid_seal_calibration(system_info, smbios).is_some(), "the seal's, measured here");
        assert!(!config.is_calibration_valid(system_info, smbios), "the cache calibration from other hardware revived");
        let mut after = serde_json::to_value(&config).unwrap();
        after.as_object_mut().unwrap().insert("seal_calibration".to_string(), serde_json::Value::Null);
        assert_eq!(after, identity, "the seal's save touched the rest of the config");
    }

    /// TODO 99: a seal calibration is this machine's only while all of its fingerprint is (each of what the cache calibration's check
    /// compares: machine ID, memory, system UUID, TSC); a cache calibration saved here, which rewrites the config's identity, doesn't
    /// revive one measured on other hardware, or one saved without a fingerprint.
    #[test]
    fn a_cache_save_doesnt_revive_a_seal_calibration_from_other_hardware() {
        use crate::calibration::{CalibrationResults, CpuSignature};
        use crate::seal::SealWidth::{Auto, W128, W256};
        use crate::seal::SealWidthSource::{Calibrated, Rule};
        let (system_info, smbios) = this_machine();
        let here = MachineFingerprint::detect(system_info, smbios);
        let tsc = here.tsc_frequency_ghz.unwrap();
        let mut elsewhere = vec![
            ("machine ID", MachineFingerprint { machine_id: Some("another machine".to_string()), ..here.clone() }),
            ("memory", MachineFingerprint { memory_id: Some("other DIMMs".to_string()), ..here.clone() }),
            ("TSC", MachineFingerprint { tsc_frequency_ghz: Some(tsc + 0.1), ..here.clone() }),
            ("none", MachineFingerprint::default()),
        ];
        // A system UUID is compared only where this machine reports one
        if here.system_uuid.is_some() {
            elsewhere.push(("system UUID", MachineFingerprint { system_uuid: Some("another board".to_string()), ..here.clone() }));
        }
        for (what, fingerprint) in elsewhere {
            let mut config = AppConfig::default();
            config.set_seal_calibration(test_seal_calibration(128), system_info, smbios);
            assert_eq!(resolve(&config, Auto), (W128, Calibrated), "{what}: measured here");
            config.seal_calibration.as_mut().unwrap().fingerprint = fingerprint;
            assert_eq!(resolve(&config, Auto), (W256, Rule), "{what}: from other hardware");

            config.update_machine_id(system_info, smbios);
            config.calibration = Some(CalibrationResults::new(CpuSignature::default()));
            assert!(config.is_calibration_valid(system_info, smbios));
            assert_eq!(resolve(&config, Auto), (W256, Rule), "{what}: revived by a cache save");
        }
    }

    /// TODO 99: a config written before seal calibrations loads, with none; one with a seal calibration round-trips it. A seal calibration
    /// saved without a fingerprint (one that kept only a machine ID) loads, keeping the rest of the config, and is stale.
    #[test]
    fn a_config_without_a_seal_calibration_loads_and_one_round_trips() {
        use crate::calibration::{CalibrationResults, CpuSignature};
        let (system_info, smbios) = this_machine();
        let mut config = AppConfig { calibration: Some(CalibrationResults::new(CpuSignature::default())), ..Default::default() };
        config.update_identity(system_info, smbios);
        config.set_seal_calibration(test_seal_calibration(256), system_info, smbios);
        let mut json: serde_json::Value = serde_json::to_value(&config).unwrap();
        let loaded: AppConfig = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(loaded.seal_calibration, config.seal_calibration);
        assert_eq!(loaded.valid_seal_calibration(system_info, smbios).and_then(SealCalibration::width), Some(crate::seal::SealWidth::W256));

        // A seal calibration without a fingerprint
        let seal = json["seal_calibration"].as_object_mut().unwrap();
        seal.remove("fingerprint");
        seal.insert("machine_id".to_string(), serde_json::Value::String(AppConfig::generate_machine_id(system_info, smbios)));
        let unstamped: AppConfig = serde_json::from_str(&json.to_string()).unwrap();
        assert_eq!(unstamped.seal_calibration.as_ref().map(|s| &s.fingerprint), Some(&MachineFingerprint::default()));
        assert!(unstamped.valid_seal_calibration(system_info, smbios).is_none());
        assert!(unstamped.is_calibration_valid(system_info, smbios) && unstamped.calibration.is_some());

        // Before seal calibrations: no such key
        json.as_object_mut().unwrap().remove("seal_calibration");
        let older: AppConfig = serde_json::from_str(&json.to_string()).unwrap();
        assert!(older.seal_calibration.is_none() && older.calibration.is_some());
        assert!(older.is_calibration_valid(system_info, smbios));
        assert!(older.valid_seal_calibration(system_info, smbios).is_none());
    }

    #[test]
    fn test_backward_compat_old_config_no_identity_fields() {
        // Simulates loading a config file from before identity fields were added
        let json = r#"{"machine_id": "old_style_id", "calibration": null}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.machine_id.as_deref(), Some("old_style_id"));
        assert!(config.system_uuid.is_none());
        assert!(config.memory_id.is_none());
        assert!(config.memory_modules.is_none());
        assert!(config.tsc_frequency_ghz.is_none());
    }
}
