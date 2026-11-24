//! Optional timings tools (TODO 122): ZenTimings and LibreHardwareMonitor, downloaded from their official GitHub releases into
//! `companions\` in TMR's home. TMR does not install their drivers and does not start them unless `--launch` is given: each asks for that
//! itself. ZenTimings is GPL-3.0 and wants an administrator; LibreHardwareMonitor is MPL-2.0. Neither is part of TMR.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::runner::RunStatus;

const ZEN_ZIP: &str = "ZenTimings_v142.713.zip";
const ZEN_URL: &str = "https://github.com/irusanov/ZenTimings/releases/download/v142.713/ZenTimings_v142.713.zip";
const ZEN_SHA_URL: &str = "https://github.com/irusanov/ZenTimings/releases/download/v142.713/ZenTimings_v142.713.zip.sha256";
const ZEN_SHA: &str = "c49f5a4a66a2c438d8b2bddfde5338df67ad8bed835a5f2e3ad421b21c4d76a0";

const LHM_ZIP: &str = "LibreHardwareMonitor.zip";
const LHM_URL: &str = "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v0.9.6/LibreHardwareMonitor.zip";
/// SHA-256 of that zip, measured from the official asset. The release does not publish one.
const LHM_SHA: &str = "086d9f1b5a99e643edc2cfaaac16051685b551e4c5ac0b32a57c58c0e529c001";

/// `companions\` in TMR's home, where the two tools are unpacked.
pub fn dir() -> PathBuf {
    crate::home::dir().join("companions")
}

/// Download both tools, check the hashes, unpack them, and say how to turn on the readings TMR uses. One already unpacked is left as it is.
/// `--launch` starts the two programs; their own windows ask before a driver is installed.
pub fn get_readings(launch: bool, out: &mut dyn Write) -> io::Result<RunStatus> {
    let root = dir();
    std::fs::create_dir_all(&root).map_err(|e| io::Error::other(format!("{}: {e}", root.display())))?;
    writeln!(out, "Timings tools, from their official GitHub releases, into {}", root.display())?;
    writeln!(out, "TMR does not install their drivers. Each program asks before it does.")?;
    writeln!(out)?;

    let zen = present_or_fetch("ZenTimings", ZEN_URL, ZEN_ZIP, Some(ZEN_SHA_URL), ZEN_SHA, &root, out)?;
    let lhm = present_or_fetch("LibreHardwareMonitor", LHM_URL, LHM_ZIP, None, LHM_SHA, &root, out)?;
    writeln!(out)?;
    writeln!(out, "ZenTimings: {}", zen.display())?;
    writeln!(out, "  It wants an administrator. Enable Live Snapshot (export) so TMR can read CL-RCD-RP-RAS.")?;
    writeln!(out, "  The snapshot file it writes is read from beside it, or from TMR_TIMINGS.")?;
    writeln!(out, "LibreHardwareMonitor: {}", lhm.display())?;
    writeln!(out, "  Options: Remote web server, on this PC only, port 8085. TMR reads DIMM temperature from it.")?;
    writeln!(out, "  Without that server, the temperature line stays empty. The run does not need either tool.")?;

    if launch {
        writeln!(out)?;
        writeln!(out, "Starting them. Windows will ask if a program wants an administrator.")?;
        start_program(&zen, out)?;
        start_program(&lhm, out)?;
    } else {
        writeln!(out)?;
        writeln!(out, "Start them from that folder, or run tmr --get-readings --launch")?;
    }
    Ok(RunStatus::Passed)
}

/// The program already unpacked under `name`, or a fresh download checked against `expect`.
fn present_or_fetch(
    name: &str, url: &str, zip_name: &str, sha_url: Option<&str>, expect: &str, root: &Path, out: &mut dyn Write,
) -> io::Result<PathBuf> {
    if let Some(exe) = find_exe(&root.join(name)) {
        writeln!(out, "{name} is already in {}", exe.display())?;
        return Ok(exe);
    }
    fetch(name, url, zip_name, sha_url, expect, root, out)
}

fn fetch(
    name: &str, url: &str, zip_name: &str, sha_url: Option<&str>, expect: &str, root: &Path, out: &mut dyn Write,
) -> io::Result<PathBuf> {
    let zip = root.join(zip_name);
    writeln!(out, "Downloading {name}…")?;
    download(url, &zip)?;
    if let Some(sha_url) = sha_url {
        let sha_file = root.join(format!("{zip_name}.sha256"));
        download(sha_url, &sha_file)?;
        let published = std::fs::read_to_string(&sha_file).map_err(|e| io::Error::other(e.to_string()))?;
        let published = hex_token(&published).ok_or_else(|| io::Error::other("the published hash is not a SHA-256"))?;
        if !published.eq_ignore_ascii_case(expect) {
            return Err(io::Error::other(format!("{name}: the published hash is not the one TMR knows ({expect})")));
        }
    }
    let got = file_sha256(&zip)?;
    if !got.eq_ignore_ascii_case(expect) {
        let _ = std::fs::remove_file(&zip);
        return Err(io::Error::other(format!("{name}: hash {got}, expected {expect}. The file was not unpacked.")));
    }
    writeln!(out, "  hash {got}")?;
    let into = root.join(name);
    if into.exists() {
        std::fs::remove_dir_all(&into).map_err(|e| io::Error::other(format!("{}: {e}", into.display())))?;
    }
    std::fs::create_dir_all(&into).map_err(|e| io::Error::other(e.to_string()))?;
    unpack(&zip, &into)?;
    Ok(find_exe(&into).unwrap_or(into))
}

fn download(url: &str, dest: &Path) -> io::Result<()> {
    let status = Command::new("curl.exe")
        .args(["--fail", "--location", "--silent", "--show-error", "--output"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| io::Error::other(format!("curl.exe: {e}")))?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("download failed ({status}): {url}"))) }
}

fn unpack(zip: &Path, into: &Path) -> io::Result<()> {
    let status = Command::new("tar.exe")
        .args(["-xf"])
        .arg(zip)
        .args(["-C"])
        .arg(into)
        .status()
        .map_err(|e| io::Error::other(format!("tar.exe: {e}")))?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("unpack failed ({status}): {}", zip.display()))) }
}

fn file_sha256(path: &Path) -> io::Result<String> {
    let output = Command::new("certutil.exe")
        .args(["-hashfile"])
        .arg(path)
        .arg("SHA256")
        .output()
        .map_err(|e| io::Error::other(format!("certutil.exe: {e}")))?;
    if !output.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    hex_token(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| io::Error::other("certutil did not print a SHA-256"))
}

fn hex_token(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|word| word.len() == 64 && word.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|word| word.to_ascii_lowercase())
}

fn find_exe(dir: &Path) -> Option<PathBuf> {
    let mut found = None;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
            if !name.to_ascii_lowercase().contains("unins") {
                return Some(path);
            }
        }
        if path.is_dir() && found.is_none() {
            found = find_exe(&path);
        }
    }
    found
}

fn start_program(exe: &Path, out: &mut dyn Write) -> io::Result<()> {
    if exe.extension().and_then(|ext| ext.to_str()).is_none_or(|ext| !ext.eq_ignore_ascii_case("exe")) {
        writeln!(out, "No program to start in {}", exe.display())?;
        return Ok(());
    }
    match Command::new(exe).spawn() {
        Ok(_) => writeln!(out, "Started {}", exe.display())?,
        Err(e) => writeln!(out, "Could not start {}: {e}", exe.display())?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sha256_line_is_its_hex_token() {
        assert_eq!(
            hex_token("c49f5a4a66a2c438d8b2bddfde5338df67ad8bed835a5f2e3ad421b21c4d76a0  ZenTimings_v142.713.zip").as_deref(),
            Some("c49f5a4a66a2c438d8b2bddfde5338df67ad8bed835a5f2e3ad421b21c4d76a0")
        );
        assert_eq!(
            hex_token("SHA256 hash of file:\r\n086d9f1b5a99e643edc2cfaaac16051685b551e4c5ac0b32a57c58c0e529c001\r\nCertUtil").as_deref(),
            Some("086d9f1b5a99e643edc2cfaaac16051685b551e4c5ac0b32a57c58c0e529c001")
        );
        assert!(hex_token("no hash here").is_none());
    }
}
