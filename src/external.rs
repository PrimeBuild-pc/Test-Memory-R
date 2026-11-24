//! Timings and temperatures from tools the user already runs (TODO 122). ZenTimings' live snapshot, and LibreHardwareMonitor's local
//! web server. Neither is required: a miss is nothing, and the run goes on. No driver, no BIOS write, no GPL code.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a run found beside itself, at its start. Absent from the results file when neither source had a fresh reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalReading {
    /// One line for the console and the window.
    pub line: String,
    /// Primaries, `CL-RCD-RP-RAS`, when ZenTimings had them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<String>,
    /// A DIMM temperature, °C, when a source had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimm_c: Option<f64>,
}

/// The reading now, or nothing when both sources are absent or stale. A closed port costs the connect timeout, then nothing.
pub fn sample() -> Option<ExternalReading> {
    let zen = zen_timings();
    let temps = libre_hardware();
    if zen.is_none() && temps.is_none() {
        return None;
    }
    let (timings, dimm_c) = zen.unwrap_or((None, None));
    let dimm_c = dimm_c.or(temps);
    let mut parts = Vec::new();
    if let Some(timings) = &timings {
        parts.push(format!("Timings {timings}"));
    }
    if let Some(temp) = dimm_c {
        parts.push(format!("DIMM {temp:.0}°C"));
    }
    Some(ExternalReading { line: parts.join(" · "), timings, dimm_c })
}

/// `sample`'s line, or empty. The window's header uses this.
pub fn line() -> String {
    sample().map(|reading| reading.line).unwrap_or_default()
}

fn zen_timings() -> Option<(Option<String>, Option<f64>)> {
    let text = std::fs::read_to_string(snapshot_path()?).ok()?;
    parse_zen(&text)
}

pub(crate) fn parse_zen(text: &str) -> Option<(Option<String>, Option<f64>)> {
    let value: Value = serde_json::from_str(text).ok()?;
    if value.get("readings").and_then(|r| r.get("stale")).and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let timings = value.pointer("/config/timings/per_dct/0/timings")?;
    let cl = number(timings, "CL")?;
    let rcd = number(timings, "RCDRD").or_else(|| number(timings, "RCD"))?;
    let rp = number(timings, "RP")?;
    let ras = number(timings, "RAS")?;
    let dimm_c = value.pointer("/readings/dimm_temp_c").and_then(Value::as_f64);
    Some((Some(format!("{cl}-{rcd}-{rp}-{ras}")), dimm_c))
}

fn number(timings: &Value, key: &str) -> Option<i64> {
    timings.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|n| n as i64)))
}

fn snapshot_path() -> Option<PathBuf> {
    if let Some(set) = std::env::var_os("TMR_TIMINGS") {
        let path = PathBuf::from(set);
        if path.is_file() {
            return Some(path);
        }
    }
    let name = "live_snapshot_device_config.json";
    let beside = crate::companions::dir().join("ZenTimings").join(name);
    if beside.is_file() {
        return Some(beside);
    }
    for var in ["LOCALAPPDATA", "ProgramData"] {
        let Some(root) = std::env::var_os(var) else { continue };
        let path = PathBuf::from(root).join("ZenTimings").join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn libre_hardware() -> Option<f64> {
    let body = http_get("127.0.0.1:8085", "/data.json")?;
    parse_libre(&body)
}

pub(crate) fn parse_libre(body: &str) -> Option<f64> {
    let value: Value = serde_json::from_str(body).ok()?;
    let mut found = None;
    walk(&value, &mut found);
    found
}

fn walk(value: &Value, found: &mut Option<f64>) {
    if found.is_some() {
        return;
    }
    let Some(obj) = value.as_object() else { return };
    let text = obj.get("Text").and_then(Value::as_str).unwrap_or("");
    let reading = obj.get("Value").and_then(Value::as_str).unwrap_or("");
    if text.contains("DIMM")
        && let Some(temp) = celsius(reading)
    {
        *found = Some(temp);
        return;
    }
    if let Some(children) = obj.get("Children").and_then(Value::as_array) {
        for child in children {
            walk(child, found);
        }
    }
}

fn celsius(reading: &str) -> Option<f64> {
    let number: String = reading.chars().take_while(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == '-').collect();
    number.parse().ok()
}

fn http_get(host: &str, path: &str) -> Option<String> {
    let addr: SocketAddr = host.parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(200))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(200))).ok()?;
    let request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8(buf).ok()?;
    let (_, body) = text.split_once("\r\n\r\n")?;
    body.starts_with('{').then(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zen_snapshot_is_the_primaries_and_a_stale_one_is_nothing() {
        let fresh = r#"{"readings":{"stale":false,"dimm_temp_c":42.4},"config":{"timings":{"per_dct":[{"timings":{"CL":30,"RCDRD":38,"RP":38,"RAS":56}}]}}}"#;
        assert_eq!(parse_zen(fresh), Some((Some("30-38-38-56".into()), Some(42.4))));
        let stale = r#"{"readings":{"stale":true},"config":{"timings":{"per_dct":[{"timings":{"CL":30,"RCDRD":38,"RP":38,"RAS":56}}]}}}"#;
        assert_eq!(parse_zen(stale), None);
    }

    #[test]
    fn libre_hardware_monitor_gives_the_first_dimm_temperature() {
        let body = r#"{"Text":"PC","Children":[{"Text":"CPU","Value":"70 °C","Children":[]},{"Text":"DIMM #1","Value":"46.5 °C"}]}"#;
        assert_eq!(parse_libre(body), Some(46.5));
    }
}
