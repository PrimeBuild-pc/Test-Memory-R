//! SMBIOS table parser for system identity and memory module detection.
//!
//! Reads raw SMBIOS data via `GetSystemFirmwareTable(RSMB)` and parses:
//! - Type 0: BIOS Information (vendor, version)
//! - Type 1: System Information (UUID, manufacturer, product)
//! - Type 17: Memory Device (per-DIMM serial, part, manufacturer, speed, capacity)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

// ============================================================================
// Public data structures
// ============================================================================

/// BIOS information from SMBIOS Type 0
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BiosInfo {
    pub vendor: String,
    pub version: String,
    /// BIOS release date in ISO format (YYYY-MM-DD), converted from SMBIOS MM/DD/YYYY
    pub release_date: String,
}

/// System identity from SMBIOS Type 1
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemIdentity {
    pub manufacturer: String,
    pub product_name: String,
    /// System UUID (motherboard/VM instance identity)
    pub uuid: String,
}

/// Per-DIMM memory module info from SMBIOS Type 17
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryModule {
    /// The module's slot as TMR names it: the channel letter and the slot, `A-2`, where the board's strings say both (`assign_slots`),
    /// else the board's own name for it, made unique
    #[serde(default)]
    pub slot: String,
    /// SMBIOS Device Locator (Type 17 offset 0x10), the board's name for the socket: `DIMM 1`, `DIMMA2`, `Controller0-ChannelA-DIMM0`
    pub locator: String,
    /// SMBIOS Bank Locator (0x11): AMD's firmware puts the channel here (`P0 CHANNEL A`), many others a bank number or nothing
    #[serde(default)]
    pub bank_locator: String,
    pub manufacturer: String,
    pub serial_number: String,
    pub part_number: String,
    /// The speed it runs at, in MT/s: SMBIOS Configured Memory Speed (0x20, or 0x58 past 65,534), 0 when the firmware doesn't say
    #[serde(default)]
    pub configured_mts: u32,
    /// SMBIOS Speed (0x15, or 0x54), in MT/s: the module's "maximum capable" speed as the firmware fills it, its JEDEC rating on many
    /// boards (4800 for a DDR5-6000 EXPO kit), the configured speed on others, now and then nonsense; 0 unknown
    #[serde(default)]
    pub max_mts: u32,
    /// SMBIOS memory type (Type 17 offset 0x12): 0x1A DDR4, 0x22 DDR5, 0x23 LPDDR5; 0 if absent
    #[serde(default)]
    pub memory_type: u8,
    /// Module capacity in bytes
    pub capacity_bytes: u64,
    /// Human-readable capacity (e.g. "64.0 GB")
    #[serde(default)]
    pub capacity_human: String,
}

/// Whether the run leaves serial numbers out of what it shows and keeps (`--no-serials`, TODO 105, the owner 2026-11-04; the
/// `machine_id`/`memory_id` fingerprints too, 2026-11-15): the DIMMs' serials, the system's UUID, and those two fingerprints,
/// from the console, the log and the results file. `AppConfig` still reads the real values to know the machine when checking
/// calibration validity; only what a run displays or writes out is affected.
static SERIALS_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `--no-serials`, set as a session starts, before SMBIOS is read
pub fn hide_serials(hidden: bool) {
    SERIALS_HIDDEN.store(hidden, std::sync::atomic::Ordering::Relaxed);
}

pub fn serials_hidden() -> bool {
    SERIALS_HIDDEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// `modules` as the run shows and keeps them: without their serials under `--no-serials`
pub fn shown_modules(modules: &[MemoryModule]) -> Vec<MemoryModule> {
    modules_shown(modules, serials_hidden())
}

/// The system as the run shows and keeps it: without its UUID under `--no-serials`
pub fn shown_system(system: &SystemIdentity) -> SystemIdentity {
    system_shown(system, serials_hidden())
}

/// A system UUID or a `machine_id`/`memory_id` fingerprint, as the run shows, logs and keeps it: left out under `--no-serials`
pub fn shown_identifier(id: &str) -> &str {
    identifier_shown(id, serials_hidden())
}

fn identifier_shown(id: &str, hidden: bool) -> &str {
    if hidden { "(left out: --no-serials)" } else { id }
}

/// A structure's string table as the parse's debug line shows it: under `--no-serials` only how many, since it holds the serials (Type
/// 1's, 2's, 3's and 17's; on EC2 Type 1's serial is the system UUID)
fn strings_shown(strings: &[String], hidden: bool) -> String {
    if hidden { format!("({} left out: --no-serials)", strings.len()) } else { format!("{strings:?}") }
}

fn modules_shown(modules: &[MemoryModule], hidden: bool) -> Vec<MemoryModule> {
    modules.iter().cloned().map(|module| if hidden { MemoryModule { serial_number: String::new(), ..module } } else { module }).collect()
}

fn system_shown(system: &SystemIdentity, hidden: bool) -> SystemIdentity {
    if hidden { SystemIdentity { uuid: String::new(), ..system.clone() } } else { system.clone() }
}

/// The SMBIOS memory type's name (Type 17 offset 0x12), for the ones a DDR5 tester meets. SMBIOS has no LPDDR5X code (its table
/// ends at 0x24, HBM3), so an LPDDR5X system reports LPDDR5's, and the name says it may be either (TODO 132).
pub fn memory_type_name(code: u8) -> Option<&'static str> {
    match code {
        0x1A => Some("DDR4"),
        0x1E => Some("LPDDR4"),
        0x22 => Some("DDR5"),
        0x23 => Some("LPDDR5/5X"),
        _ => None,
    }
}

/// All SMBIOS-derived system information + supplementary CPU/BIOS data from registry
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmbiosData {
    pub bios: BiosInfo,
    pub system: SystemIdentity,
    pub memory_modules: Vec<MemoryModule>,
    /// CPU microcode revision (e.g. "0x010003E0") — from registry, not SMBIOS
    pub cpu_microcode: String,
    /// CPU base frequency in MHz from registry ~MHz — fallback when CPUID leaf 0x16 returns 0
    pub cpu_base_mhz: u32,
}

// ============================================================================
// Detection
// ============================================================================

impl SmbiosData {
    /// Read and parse SMBIOS tables from firmware, plus CPU registry data. Returns Default on failure.
    pub fn detect() -> Self {
        let mut data = match read_smbios_raw() {
            Some(raw) => parse_smbios(&raw),
            None => {
                log::warn!("Failed to read SMBIOS tables — using defaults");
                Self::default()
            }
        };

        // Supplement with CPU data from registry (microcode revision, base frequency)
        let (microcode, base_mhz) = read_cpu_registry_info();
        data.cpu_microcode = microcode;
        data.cpu_base_mhz = base_mhz;

        data
    }

    /// Generate a hash of memory module configuration.
    /// Detects DIMM swaps, XMP changes, capacity changes.
    pub fn memory_fingerprint(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.memory_modules.len().hash(&mut hasher);
        for module in &self.memory_modules {
            module.manufacturer.hash(&mut hasher);
            module.serial_number.hash(&mut hasher);
            module.part_number.hash(&mut hasher);
            // The configured speed, so an XMP/EXPO change shows, and the maximum the firmware reports
            module.configured_mts.hash(&mut hasher);
            module.max_mts.hash(&mut hasher);
            module.capacity_bytes.hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }

    /// Total memory capacity across all modules
    pub fn total_memory_bytes(&self) -> u64 {
        self.memory_modules.iter().map(|m| m.capacity_bytes).sum()
    }
}

static SMBIOS: OnceLock<SmbiosData> = OnceLock::new();

/// The machine's SMBIOS data, read from firmware on first use. The tables cannot change while we run, and both the calibration identity
/// check and the result file's identity need them, so they are read once rather than once per caller.
pub fn get_smbios() -> &'static SmbiosData {
    SMBIOS.get_or_init(SmbiosData::detect)
}

/// Format capacity in bytes to human-readable string (e.g. "64.0 GB")
pub fn format_capacity(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// Raw SMBIOS reading via GetSystemFirmwareTable
// ============================================================================

/// Read raw SMBIOS data from firmware table
fn read_smbios_raw() -> Option<Vec<u8>> {
    use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};

    // First call: get required buffer size
    // SAFETY: no buffer, so it only returns the size the table needs
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
    if size == 0 {
        log::warn!("GetSystemFirmwareTable returned 0 size");
        return None;
    }

    // Second call: read the data
    let mut buffer = vec![0u8; size as usize];
    // SAFETY: the slice carries its length, and Windows writes within it
    let written = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(&mut buffer)) };
    if written == 0 {
        log::warn!("GetSystemFirmwareTable failed to read data");
        return None;
    }

    buffer.truncate(written as usize);
    log::debug!("Read {} bytes of SMBIOS data", buffer.len());
    Some(buffer)
}

// ============================================================================
// CPU registry data (microcode, base frequency)
// ============================================================================

/// Read CPU microcode revision and base MHz from Windows registry.
/// Returns (microcode_string, base_mhz). Falls back to empty/0 on failure.
fn read_cpu_registry_info() -> (String, u32) {
    use windows::Win32::System::Registry::*;
    use windows::core::PCSTR;

    let mut microcode = String::new();
    let mut base_mhz: u32 = 0;

    let subkey = b"HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\0";

    let mut hkey = HKEY::default();
    // SAFETY: a NUL-terminated name that outlives the call, and the key's out-param on the stack
    let result = unsafe { RegOpenKeyExA(HKEY_LOCAL_MACHINE, PCSTR::from_raw(subkey.as_ptr()), Some(0), KEY_READ, &mut hkey) };

    if result.is_err() {
        log::debug!("Failed to open CPU registry key: {:?}", result);
        return (microcode, base_mhz);
    }

    // Read ~MHz (DWORD)
    let mhz_name = b"~MHz\0";
    let mut mhz_value: u32 = 0;
    let mut mhz_size = std::mem::size_of::<u32>() as u32;
    // SAFETY: the key opened above, a NUL-terminated name, and a u32 on the stack with its size, which Windows writes within (a larger
    // value fails instead)
    let result = unsafe {
        RegQueryValueExA(
            hkey,
            PCSTR::from_raw(mhz_name.as_ptr()),
            None,
            None,
            Some(&mut mhz_value as *mut u32 as *mut u8),
            Some(&mut mhz_size),
        )
    };
    if result.is_ok() {
        base_mhz = mhz_value;
        log::debug!("Registry CPU ~MHz: {}", base_mhz);
    }

    // Read "Update Revision" (REG_BINARY, 8 bytes — microcode revision)
    let rev_name = b"Update Revision\0";
    let mut rev_buf = [0u8; 8];
    let mut rev_size = 8u32;
    // SAFETY: as for ~MHz, into an 8-byte array with its length
    let result =
        unsafe { RegQueryValueExA(hkey, PCSTR::from_raw(rev_name.as_ptr()), None, None, Some(rev_buf.as_mut_ptr()), Some(&mut rev_size)) };
    if result.is_ok() && rev_size >= 4 {
        // Microcode revision is stored as a little-endian value.
        // The high DWORD (bytes 4-7) contains the actual revision.
        let revision = if rev_size >= 8 {
            u32::from_le_bytes([rev_buf[4], rev_buf[5], rev_buf[6], rev_buf[7]])
        } else {
            u32::from_le_bytes([rev_buf[0], rev_buf[1], rev_buf[2], rev_buf[3]])
        };
        microcode = format!("0x{:08X}", revision);
        log::debug!("Registry CPU microcode: {} (raw bytes: {:02X?})", microcode, &rev_buf[..rev_size as usize]);
    }

    // SAFETY: the key `RegOpenKeyExA` opened (failing, it returned early), closed once
    let _ = unsafe { RegCloseKey(hkey) };

    (microcode, base_mhz)
}

// ============================================================================
// SMBIOS table parsing
// ============================================================================

/// Raw SMBIOS firmware table header (8 bytes before the structure table)
/// Layout: Used (1), Major (1), Minor (1), Revision (1), Length (4)
const SMBIOS_HEADER_SIZE: usize = 8;

/// Parse raw SMBIOS firmware table data
fn parse_smbios(raw: &[u8]) -> SmbiosData {
    let mut data = SmbiosData::default();
    let mut found_type0 = false;
    let mut found_type1 = false;
    let mut type16_capacity_bytes: u64 = 0;

    if raw.len() < SMBIOS_HEADER_SIZE {
        log::warn!("SMBIOS data too short ({} bytes)", raw.len());
        return data;
    }

    // The firmware table header (`RawSMBIOSData`): calling method, then the SMBIOS major and minor version
    let version = (raw[1], raw[2]);
    // Skip the firmware table header to get to the SMBIOS structure table
    let table = &raw[SMBIOS_HEADER_SIZE..];
    let mut offset = 0;

    while offset + 4 <= table.len() {
        let struct_type = table[offset];
        let struct_len = table[offset + 1] as usize;

        if struct_len < 4 {
            break; // Invalid structure
        }

        // End-of-table marker (Type 127)
        if struct_type == 127 {
            break;
        }

        // Ensure we have enough data for the formatted section
        if offset + struct_len > table.len() {
            break;
        }

        // Extract the formatted (fixed-length) part
        let formatted = &table[offset..offset + struct_len];

        // Parse the string table (follows the formatted section)
        let string_start = offset + struct_len;
        let (strings, next_offset) = parse_string_table(table, string_start);

        let end_preview = (next_offset + 8).min(table.len());
        log::debug!(
            "SMBIOS: Type {} at offset {}, formatted_len={}, strings={}, next_offset={}, bytes_at_next={:02X?}",
            struct_type,
            offset,
            struct_len,
            strings_shown(&strings, serials_hidden()),
            next_offset,
            &table[next_offset..end_preview]
        );

        match struct_type {
            0 if !found_type0 => {
                found_type0 = true;
                parse_type0_bios(formatted, &strings, &mut data.bios);
            }
            1 if !found_type1 => {
                found_type1 = true;
                parse_type1_system(formatted, &strings, &mut data.system);
            }
            16 => {
                // Type 16: Physical Memory Array — fallback for when Type 17 is absent
                if formatted.len() >= 0x0F {
                    // Offset 0x07: Maximum Capacity in KB (u32), 0x80000000 means use extended
                    let max_cap_kb = u32::from_le_bytes([formatted[0x07], formatted[0x08], formatted[0x09], formatted[0x0A]]);
                    if max_cap_kb == 0x80000000 && formatted.len() >= 0x17 {
                        // Extended Maximum Capacity at offset 0x0F (u64, in bytes)
                        type16_capacity_bytes = u64::from_le_bytes([
                            formatted[0x0F], formatted[0x10], formatted[0x11], formatted[0x12], formatted[0x13], formatted[0x14],
                            formatted[0x15], formatted[0x16],
                        ]);
                    } else if max_cap_kb != 0x80000000 {
                        type16_capacity_bytes = (max_cap_kb as u64) * 1024;
                    }
                    // Offset 0x0D: Number of Memory Devices (u16)
                    let device_count = u16::from_le_bytes([formatted[0x0D], formatted[0x0E]]);
                    log::debug!("SMBIOS Type 16: capacity={} bytes, devices={}", type16_capacity_bytes, device_count);
                }
            }
            17 => {
                if struct_len >= 0x0E {
                    let size_field = u16::from_le_bytes([formatted[0x0C], formatted[0x0D]]);
                    log::debug!(
                        "SMBIOS Type 17: len={}, size_field=0x{:04X}, bytes={:02X?}",
                        struct_len,
                        size_field,
                        &formatted[..struct_len]
                    );
                }
                if let Some(module) = parse_type17_memory(formatted, &strings, version) {
                    data.memory_modules.push(module);
                } else {
                    log::debug!("SMBIOS Type 17: skipped (empty slot or short struct, len={})", struct_len);
                }
            }
            _ => {} // Skip other types / duplicate Type 0/1
        }

        offset = next_offset;
    }

    // Fix up Type 17 modules that have capacity=0 but Type 16 has the real capacity.
    // This happens on VMs (e.g. EC2) with buggy firmware: Type 17 says size=0x7FFF (use extended) but extended size is 0. Type 16 has the
    // correct total.
    if !data.memory_modules.is_empty() && type16_capacity_bytes > 0 {
        let modules_needing_fixup: Vec<usize> =
            data.memory_modules.iter().enumerate().filter(|(_, m)| m.capacity_bytes == 0).map(|(i, _)| i).collect();
        if !modules_needing_fixup.is_empty() {
            let per_module = type16_capacity_bytes / modules_needing_fixup.len() as u64;
            for idx in modules_needing_fixup {
                data.memory_modules[idx].capacity_bytes = per_module;
                data.memory_modules[idx].capacity_human = format_capacity(per_module);
            }
            log::debug!(
                "SMBIOS: Fixed up {} module(s) with capacity from Type 16 ({})",
                data.memory_modules.len(),
                format_capacity(type16_capacity_bytes)
            );
        }
    }

    assign_slots(&mut data.memory_modules);

    // Set capacity_human for any modules that have it unset
    for module in &mut data.memory_modules {
        if module.capacity_human.is_empty() && module.capacity_bytes > 0 {
            module.capacity_human = format_capacity(module.capacity_bytes);
        }
    }

    let bios_str = if data.bios.vendor.is_empty() && data.bios.version.is_empty() {
        "(not available)".to_string()
    } else {
        format!("{} {}", data.bios.vendor, data.bios.version).trim().to_string()
    };
    let sys_str = if data.system.manufacturer.is_empty() && data.system.product_name.is_empty() {
        "(not available)".to_string()
    } else {
        format!("{} {}", data.system.manufacturer, data.system.product_name).trim().to_string()
    };
    let uuid_str = match (serials_hidden(), data.system.uuid.is_empty()) {
        (true, _) => "(left out: --no-serials)",
        (false, true) => "(none)",
        (false, false) => &data.system.uuid,
    };
    let mem_str = if data.memory_modules.is_empty() {
        "none detected".to_string()
    } else {
        let total_gb = data.total_memory_bytes() as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{} module(s), {:.1} GB total", data.memory_modules.len(), total_gb)
    };
    log::debug!("SMBIOS: BIOS={}, System={}, UUID={}, Memory={}", bios_str, sys_str, uuid_str, mem_str);

    data
}

/// Parse the null-terminated string table after a structure's formatted section.
/// Returns (vector of strings, offset of next structure).
fn parse_string_table(table: &[u8], start: usize) -> (Vec<String>, usize) {
    let mut strings = Vec::new();
    let mut pos = start;

    // SMBIOS string table format:
    //   - Each string is null-terminated (0x00)
    //   - After the last string, an additional null terminates the set (double-null)
    //   - If NO strings: two consecutive null bytes (0x00, 0x00)
    //
    // The next structure begins immediately after the double-null.
    loop {
        if pos >= table.len() {
            break;
        }

        // Find the end of this string
        let str_start = pos;
        while pos < table.len() && table[pos] != 0 {
            pos += 1;
        }

        if pos == str_start {
            // Hit a null without any preceding string content — end of string table.
            pos += 1; // Skip this null
            break;
        }

        let s = String::from_utf8_lossy(&table[str_start..pos]).trim().to_string();
        strings.push(s);
        pos += 1; // Skip the null terminator after this string
    }

    // When there are NO strings, the string area is 0x00 0x00 (double-null).
    // The loop above only consumed the first null. Consume the second.
    // With strings present this is a no-op: the loop already ate "laststr\0" + "\0".
    if strings.is_empty() && pos < table.len() && table[pos] == 0 {
        pos += 1;
    }

    (strings, pos)
}

/// Get a string by 1-based index from the string table
fn get_string(strings: &[String], index: u8) -> String {
    if index == 0 {
        return String::new();
    }
    strings.get((index - 1) as usize).cloned().unwrap_or_default()
}

/// Parse Type 0: BIOS Information
fn parse_type0_bios(formatted: &[u8], strings: &[String], bios: &mut BiosInfo) {
    // Offset 0x04: Vendor (string index)
    // Offset 0x05: BIOS Version (string index)
    // Offset 0x08: BIOS Release Date (string index, typically MM/DD/YYYY)
    if formatted.len() >= 6 {
        bios.vendor = get_string(strings, formatted[0x04]);
        bios.version = get_string(strings, formatted[0x05]);
    }
    if formatted.len() >= 9 {
        let raw_date = get_string(strings, formatted[0x08]);
        bios.release_date = convert_bios_date_to_iso(&raw_date);
    }
}

/// Convert BIOS date from MM/DD/YYYY to ISO YYYY-MM-DD.
/// Falls back to the raw string if parsing fails.
fn convert_bios_date_to_iso(raw: &str) -> String {
    // SMBIOS spec says format is MM/DD/YYYY
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.len() == 3 {
        let month = parts[0];
        let day = parts[1];
        let year = parts[2];
        if !month.is_empty() && !day.is_empty() && year.len() == 4 {
            return format!("{}-{:0>2}-{:0>2}", year, month, day);
        }
    }
    raw.to_string()
}

/// Parse Type 1: System Information
fn parse_type1_system(formatted: &[u8], strings: &[String], system: &mut SystemIdentity) {
    // Offset 0x04: Manufacturer (string index)
    // Offset 0x05: Product Name (string index)
    // Offset 0x08-0x17: UUID (16 bytes)
    if formatted.len() >= 8 {
        system.manufacturer = get_string(strings, formatted[0x04]);
        system.product_name = get_string(strings, formatted[0x05]);
    }

    if formatted.len() >= 0x18 {
        let uuid = &formatted[0x08..0x18];
        // UUID format per SMBIOS spec: first 3 fields are little-endian
        system.uuid = format!(
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            uuid[3],
            uuid[2],
            uuid[1],
            uuid[0],
            uuid[5],
            uuid[4],
            uuid[7],
            uuid[6],
            uuid[8],
            uuid[9],
            uuid[10],
            uuid[11],
            uuid[12],
            uuid[13],
            uuid[14],
            uuid[15],
        );
    }
}

/// Parse Type 17: Memory Device — returns None for genuinely empty slots
fn parse_type17_memory(formatted: &[u8], strings: &[String], version: (u8, u8)) -> Option<MemoryModule> {
    // Need at least enough bytes for the size field
    if formatted.len() < 0x0E {
        return None;
    }

    // Offset 0x0C: Size (u16) — 0 means empty slot, 0xFFFF means unknown
    let size_field = u16::from_le_bytes([formatted[0x0C], formatted[0x0D]]);
    if size_field == 0 {
        return None; // Genuinely empty DIMM slot
    }

    // Size: bit 15 indicates KB (1) vs MB (0), 0x7FFF means use extended, 0xFFFF means unknown
    let capacity_bytes = if size_field == 0xFFFF {
        0 // Unknown size — still parse the module for speed/strings
    } else if size_field == 0x7FFF {
        // Extended size at offset 0x1C (u32, in MB) — for modules > 32GB
        // Some VMs set 0x7FFF but leave extended size as 0 (buggy firmware)
        if formatted.len() >= 0x20 {
            let ext_size_mb = u32::from_le_bytes([formatted[0x1C], formatted[0x1D], formatted[0x1E], formatted[0x1F]]);
            (ext_size_mb as u64) * 1024 * 1024
        } else {
            0
        }
    } else if size_field & 0x8000 != 0 {
        // Size in KB
        ((size_field & 0x7FFF) as u64) * 1024
    } else {
        // Size in MB
        (size_field as u64) * 1024 * 1024
    };

    // Offset 0x12: Memory Type (u8)
    let memory_type = formatted.get(0x12).copied().unwrap_or(0);

    // Offset 0x15: Speed, the maximum capable (2.3+), and 0x20: Configured Memory Speed (2.7+), each MT/s (MHz before 3.1), 0 unknown,
    // 0xFFFF for the DWORD at 0x54 / 0x58 (3.3+). Each read only where the structure's length covers it (DSP0134 6.1.1)
    let max_mts = speed_field(formatted, 0x15, 0x54);
    let configured_mts = as_data_rate(speed_field(formatted, 0x20, 0x58), memory_type, version);

    // String references (all guarded for short structs)
    // 0x10: Device Locator
    // 0x17: Manufacturer
    // 0x18: Serial Number
    // 0x19: Asset Tag
    // 0x1A: Part Number
    let locator = if formatted.len() > 0x10 { get_string(strings, formatted[0x10]) } else { String::new() };
    let bank_locator = if formatted.len() > 0x11 { get_string(strings, formatted[0x11]) } else { String::new() };
    let manufacturer = if formatted.len() > 0x17 { get_string(strings, formatted[0x17]) } else { String::new() };
    let serial_number = if formatted.len() > 0x18 { get_string(strings, formatted[0x18]) } else { String::new() };
    let part_number = if formatted.len() > 0x1A { get_string(strings, formatted[0x1A]) } else { String::new() };

    Some(MemoryModule {
        slot: String::new(),
        locator,
        bank_locator,
        manufacturer,
        serial_number,
        part_number,
        configured_mts,
        max_mts,
        memory_type,
        capacity_human: format_capacity(capacity_bytes),
        capacity_bytes,
    })
}

/// A Type 17 speed (DSP0134 Table 75): the WORD at `word`, or past 65,534 MT/s (0xFFFF) the DWORD at `dword`, bit 31 reserved. 0 when
/// unknown, including a field the structure is too short to have.
fn speed_field(formatted: &[u8], word: usize, dword: usize) -> u32 {
    let Some(&[lo, hi]) = formatted.get(word..word + 2) else {
        return 0;
    };
    match u16::from_le_bytes([lo, hi]) {
        0xFFFF => formatted.get(dword..dword + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & 0x7FFF_FFFF),
        speed => u32::from(speed),
    }
}

/// A configured speed as a data rate. Before SMBIOS 3.1 it was a clock in MHz, and some firmware of then writes the half-rate clock
/// (an MSI X370 BIOS of 2017: 1200 for DDR4-2400, TODO 132's research): a value below the type's slowest JEDEC data rate is taken for
/// one and doubled. From 3.1 it is MT/s as it reads.
fn as_data_rate(configured: u32, memory_type: u8, version: (u8, u8)) -> u32 {
    let slowest = match memory_type {
        0x18 => 800,         // DDR3
        0x1A | 0x1E => 1600, // DDR4, LPDDR4
        0x1D => 1066,        // LPDDR3
        _ => 0,
    };
    if version < (3, 1) && configured > 0 && configured < slowest { configured * 2 } else { configured }
}

/// Each module's `slot`, the board's slot as a reader names it, `A-2`: the channel letter, then the slot in it from 1. From the board's
/// strings where they say both, one scheme for the whole machine (TODO 132's research, about 250 boards' SMBIOS dumps):
/// - a letter and a number at the end of the Device Locator, as the board prints its slots: `DIMMA2`, `DIMM_A1`, `DDR5-B2`,
///   `Controller1-DIMMB2`, `A1` (newer MSI AM5 BIOSes, ASUS AM4, Gigabyte Z790, Supermicro, Dell servers);
/// - AMD's default: Device Locator `DIMM n`, the slot in its channel from 0, and Bank Locator `P0 CHANNEL X`: `X-(n + 1)` (most AM4 and
///   AM5 boards, AMD laptops; one MSI board seen under both its BIOS formats reads `DIMM 1`/`P0 CHANNEL A` on one and `DIMMA2` on the
///   other).
///
/// Every other scheme (Intel's `Controller0-ChannelA-DIMM0`, counted from 0 or 1 by vendor; servers' `CPU0 Channel0 DIMM0`; Dell
/// laptops' `Motherboard` on every channel) keeps the board's own names: the Device Locator where those differ, else with the Bank
/// Locator before it, else numbered in the table's order. So do all of them when any module doesn't fit or two labels would match: a
/// wrong slot is worse than the board's own words.
fn assign_slots(modules: &mut [MemoryModule]) {
    let slots: Option<Vec<String>> = modules.iter().map(|m| slot_label(&m.locator, &m.bank_locator)).collect();
    let names = match slots.filter(|slots| all_differ(slots)) {
        Some(slots) => slots,
        None => board_names(modules),
    };
    for (module, name) in modules.iter_mut().zip(names) {
        module.slot = name;
    }
}

/// The `A-2` label a module's two strings give, if they give one (`assign_slots`)
fn slot_label(device: &str, bank: &str) -> Option<String> {
    let device = device.trim().to_ascii_uppercase();
    // A letter and a number the board prints: at the end, after `DIMM`, `DDRn` and an optional separator, or the whole string
    let digits = device.len() - device.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if (1..=2).contains(&digits) {
        let head = &device[..device.len() - digits];
        let number = &device[device.len() - digits..];
        if let Some(letter) = head.chars().last().filter(char::is_ascii_uppercase) {
            let before = head[..head.len() - 1].trim_end_matches(['_', '-', ' ']);
            let named =
                before.is_empty() || before.ends_with("DIMM") || before.strip_suffix(char::is_numeric).is_some_and(|b| b.ends_with("DDR"));
            if named {
                return Some(format!("{letter}-{}", number.parse::<u32>().ok()?));
            }
        }
    }
    // AMD's default: `DIMM n`, counted from 0 in the channel the Bank Locator names
    let slot: u32 = device.strip_prefix("DIMM")?.trim().parse().ok()?;
    let bank = bank.trim().to_ascii_uppercase();
    let channel = bank.split_whitespace().skip_while(|word| *word != "CHANNEL").nth(1)?;
    let letter = channel.chars().next().filter(|c| c.is_ascii_uppercase() && channel.len() == 1)?;
    Some(format!("{letter}-{}", slot + 1))
}

/// The board's own names, made to differ: the Device Locator, else the Bank Locator with it, else either numbered in table order
fn board_names(modules: &[MemoryModule]) -> Vec<String> {
    let device: Vec<String> = modules.iter().map(|m| m.locator.trim().to_string()).collect();
    if device.iter().all(|d| !d.is_empty()) && all_differ(&device) {
        return device;
    }
    let both: Vec<String> = modules
        .iter()
        .map(|m| [m.bank_locator.trim(), m.locator.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
        .collect();
    if both.iter().all(|b| !b.is_empty()) && all_differ(&both) {
        return both;
    }
    both.iter().enumerate().map(|(i, name)| if name.is_empty() { format!("#{}", i + 1) } else { format!("{name} #{}", i + 1) }).collect()
}

fn all_differ(names: &[String]) -> bool {
    let mut seen = std::collections::HashSet::new();
    names.iter().all(|name| seen.insert(name.as_str()))
}

impl MemoryModule {
    /// The speed it runs at, else the maximum the firmware reports: what to show first
    pub fn speed_mts(&self) -> u32 {
        if self.configured_mts > 0 { self.configured_mts } else { self.max_mts }
    }

    /// Its speed for a reader: `6000 MT/s`, the SMBIOS maximum after it where it differs, `6000 MT/s (SMBIOS max 4800)`; empty when
    /// neither is known
    pub fn speed_label(&self) -> String {
        match (self.configured_mts, self.max_mts) {
            (0, 0) => String::new(),
            (0, max) => format!("{max} MT/s"),
            (configured, max) if max == 0 || max == configured => format!("{configured} MT/s"),
            (configured, max) => format!("{configured} MT/s (SMBIOS max {max})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TODO 105: `--no-serials` leaves out the DIMMs' serials, the system's UUID, and the `machine_id`/`memory_id` fingerprints,
    /// and nothing else; without it, all three stay
    #[test]
    fn no_serials_leaves_out_the_serials_the_uuid_and_the_fingerprints_only() {
        let module = MemoryModule {
            locator: "DIMM_A2".to_string(),
            manufacturer: "G Skill".to_string(),
            serial_number: "12345678".to_string(),
            part_number: "F5-6000J3038F16G".to_string(),
            configured_mts: 6000,
            capacity_bytes: 16 << 30,
            ..Default::default()
        };
        let system =
            SystemIdentity { manufacturer: "ASUS".to_string(), product_name: "PRIME".to_string(), uuid: "03000200-0400".to_string() };
        let hidden = modules_shown(std::slice::from_ref(&module), true);
        assert_eq!(
            (hidden[0].serial_number.as_str(), hidden[0].part_number.as_str(), hidden[0].configured_mts),
            ("", "F5-6000J3038F16G", 6000)
        );
        assert_eq!(modules_shown(std::slice::from_ref(&module), false)[0].serial_number, "12345678");
        let without = system_shown(&system, true);
        assert_eq!((without.uuid.as_str(), without.product_name.as_str()), ("", "PRIME"));
        assert_eq!(system_shown(&system, false).uuid, "03000200-0400");
        // And the calibration's UUID/machine_id/memory_id mismatch lines (`MachineFingerprint::matches`)
        assert_eq!(identifier_shown("03000200-0400", true), "(left out: --no-serials)");
        assert_eq!(identifier_shown("03000200-0400", false), "03000200-0400");
        assert_eq!(identifier_shown("a1b2c3d4", true), "(left out: --no-serials)");
        assert_eq!(identifier_shown("a1b2c3d4", false), "a1b2c3d4");
    }

    /// TODO 105: the parse's debug line (`RUST_LOG=debug`) shows a structure's string table, Type 17's serial among it, only without
    /// `--no-serials`; with it, only how many strings there are
    #[test]
    fn no_serials_leaves_the_string_tables_out_of_the_debug_lines() {
        let strings = vec!["DIMM_A2".to_string(), "G Skill".to_string(), "12345678".to_string()];
        assert_eq!(strings_shown(&strings, true), "(3 left out: --no-serials)");
        assert_eq!(strings_shown(&strings, false), r#"["DIMM_A2", "G Skill", "12345678"]"#);
    }

    /// A Type 17 formatted section `len` bytes long, its Speed (0x15) and Configured Memory Speed (0x20) WORDs and their DWORDs set
    fn type17(len: usize, speed: u16, configured: u16, extended: (u32, u32)) -> Vec<u8> {
        let mut f = vec![0u8; len];
        let mut put = |at: usize, bytes: &[u8]| {
            if at + bytes.len() <= len {
                f[at..at + bytes.len()].copy_from_slice(bytes);
            }
        };
        put(0x15, &speed.to_le_bytes());
        put(0x20, &configured.to_le_bytes());
        put(0x54, &extended.0.to_le_bytes());
        put(0x58, &extended.1.to_le_bytes());
        f
    }

    /// TODO 132's speed fix: the configured speed at 0x20, the maximum at 0x15, each read only where the structure's length covers it,
    /// 0xFFFF sending it to the DWORD (bit 31 reserved), and a pre-3.1 half-rate clock doubled
    #[test]
    fn the_speeds_come_from_their_fields() {
        let f = type17(0x5C, 4800, 6000, (0, 0));
        assert_eq!((speed_field(&f, 0x15, 0x54), speed_field(&f, 0x20, 0x58)), (4800, 6000));
        let f = type17(0x5C, 0xFFFF, 0xFFFF, (70_000 | 1 << 31, 72_000));
        assert_eq!((speed_field(&f, 0x15, 0x54), speed_field(&f, 0x20, 0x58)), (70_000, 72_000), "the DWORDs, bit 31 masked");
        let short = type17(0x1B, 2400, 0, (0, 0));
        assert_eq!((speed_field(&short, 0x15, 0x54), speed_field(&short, 0x20, 0x58)), (2400, 0), "an SMBIOS 2.3 structure has no 0x20");
        let no_dword = type17(0x54, 0xFFFF, 6000, (0, 0));
        assert_eq!(speed_field(&no_dword, 0x15, 0x54), 0, "0xFFFF in a 3.2 structure, with no 0x54: unknown");
        // MSI X370, SMBIOS 3.0: DDR4-2400's configured speed as its 1200 MHz clock
        assert_eq!(as_data_rate(1200, 0x1A, (3, 0)), 2400);
        assert_eq!(as_data_rate(1200, 0x1A, (3, 1)), 1200, "MT/s from 3.1");
        assert_eq!(as_data_rate(2400, 0x1A, (3, 0)), 2400, "a data rate already");
        assert_eq!(as_data_rate(3000, 0x22, (3, 0)), 3000, "no rule for a type without a pre-3.1 era");
    }

    /// The current speed first, the SMBIOS maximum only where it differs
    #[test]
    fn the_speed_reads_current_first() {
        let speeds = |configured_mts, max_mts| MemoryModule { configured_mts, max_mts, ..Default::default() };
        assert_eq!(speeds(6000, 4800).speed_label(), "6000 MT/s (SMBIOS max 4800)");
        assert_eq!(speeds(6000, 6000).speed_label(), "6000 MT/s");
        assert_eq!(speeds(0, 7200).speed_label(), "7200 MT/s", "an EC2 VM: no configured speed");
        assert_eq!(speeds(0, 0).speed_label(), "");
        assert_eq!((speeds(6000, 4800).speed_mts(), speeds(0, 7200).speed_mts()), (6000, 7200));
    }

    /// TODO 132's labels, from real boards' strings (the research's dumps): the channel letter and the slot from 1, where the strings say
    /// both
    #[test]
    fn the_board_strings_give_the_slot() {
        let cases = [
            // AMD's default, the slot counted from 0 in its channel: ASUS and ASRock AM5, MSI AM4, AMD laptops
            ("DIMM 1", "P0 CHANNEL A", Some("A-2")),
            ("DIMM 0", "P0 CHANNEL B", Some("B-1")),
            ("DIMM 0", "P0 CHANNEL D", Some("D-1")),
            // The slot as the board prints it: MSI AM5's newer BIOSes, ASUS AM4, Gigabyte Z790, MSI Z790, Supermicro, Dell servers
            ("DIMMA2", "P0 CHANNEL A", Some("A-2")),
            ("DIMM_B1", "BANK 2", Some("B-1")),
            ("DDR5-B2", "BANK 0", Some("B-2")),
            ("Controller1-DIMMB2", "BANK 0", Some("B-2")),
            ("P1-DIMMA1", "P0_Node0_Channel0_Dimm0", Some("A-1")),
            ("A16", "", Some("A-16")),
            // Schemes that don't say both, or count by vendor: the board's own names
            ("Controller0-ChannelA-DIMM0", "BANK 0", None),
            ("ChannelA-DIMM1", "BANK 1", None),
            ("CPU0 Channel0 DIMM0", "CPU0 Channel0", None),
            ("PROC 1 DIMM 1", "", None),
            ("DIMM_P0_A0", "", None),
            ("Motherboard", "BANK 0", None),
            ("DIMM 1", "BANK 0", None),
            ("", "", None),
        ];
        for (device, bank, slot) in cases {
            assert_eq!(slot_label(device, bank).as_deref(), slot, "{device:?} / {bank:?}");
        }
    }

    /// One scheme a machine: labels where every module has one and they differ, else the board's names, the Bank Locator with them
    /// where the Device Locators repeat, else numbered in table order
    #[test]
    fn every_module_gets_a_slot_of_its_own() {
        let slots = |pairs: &[(&str, &str)]| {
            let mut modules: Vec<MemoryModule> = pairs
                .iter()
                .map(|(device, bank)| MemoryModule { locator: device.to_string(), bank_locator: bank.to_string(), ..Default::default() })
                .collect();
            assign_slots(&mut modules);
            modules.into_iter().map(|m| m.slot).collect::<Vec<_>>()
        };
        // The forum reporter's ASUS board, and the ASRock B650M: "DIMM 1" twice
        assert_eq!(slots(&[("DIMM 1", "P0 CHANNEL A"), ("DIMM 1", "P0 CHANNEL B")]), ["A-2", "B-2"]);
        assert_eq!(
            slots(&[("DIMM 0", "P0 CHANNEL A"), ("DIMM 1", "P0 CHANNEL A"), ("DIMM 0", "P0 CHANNEL B"), ("DIMM 1", "P0 CHANNEL B")]),
            ["A-1", "A-2", "B-1", "B-2"]
        );
        // Intel: unique Device Locators, kept as they are
        assert_eq!(slots(&[("Controller0-ChannelA-DIMM0", "BANK 0"), ("Controller1-ChannelA-DIMM0", "BANK 0")]), [
            "Controller0-ChannelA-DIMM0", "Controller1-ChannelA-DIMM0"
        ]);
        // A module that doesn't fit puts the whole machine on the board's names
        assert_eq!(slots(&[("DIMM 1", "P0 CHANNEL A"), ("DIMM 1", "BANK 1")]), ["P0 CHANNEL A DIMM 1", "BANK 1 DIMM 1"]);
        // Dell laptops: every channel "Motherboard", the bank repeated
        assert_eq!(slots(&[("Motherboard", "BANK 0"), ("Motherboard", "BANK 0")]), ["BANK 0 Motherboard #1", "BANK 0 Motherboard #2"]);
        assert_eq!(slots(&[("", ""), ("", "")]), ["#1", "#2"], "a VM's empty strings");
    }

    #[test]
    fn test_detect_smbios() {
        // Integration test: should succeed on any Windows machine
        let data = SmbiosData::detect();
        // UUID should be non-empty on real hardware or VMs
        assert!(!data.system.uuid.is_empty() || data.system.manufacturer.is_empty(), "Expected UUID or empty SMBIOS on this system");
        println!("BIOS: {} {}", data.bios.vendor, data.bios.version);
        println!("System: {} {} UUID={}", data.system.manufacturer, data.system.product_name, data.system.uuid);
        println!("Memory modules: {}", data.memory_modules.len());
        for m in &data.memory_modules {
            println!(
                "  {} {} {} {} {}MT/s {:.1}GB",
                m.locator,
                m.manufacturer,
                m.serial_number,
                m.part_number,
                m.speed_label(),
                m.capacity_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
            );
        }
        println!("Memory fingerprint: {}", data.memory_fingerprint());
    }

    #[test]
    fn test_memory_fingerprint_deterministic() {
        let data = SmbiosData::detect();
        let fp1 = data.memory_fingerprint();
        let fp2 = data.memory_fingerprint();
        assert_eq!(fp1, fp2, "Fingerprint should be deterministic");
    }
}
