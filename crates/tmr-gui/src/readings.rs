//! Timings and temperatures the window can show without a driver of its own (TODO 122). ZenTimings' live snapshot, if the user already
//! runs it, and LibreHardwareMonitor's local web server, if that is running. Neither is required: a miss is an empty line, and the run
//! goes on. No BIOS write, no GPL code, no new crate.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

/// One line for the header, or empty when neither source had a fresh reading.
pub fn line() -> String {
    let mut parts = Vec::new();
    if let Some(timings) = zen_timings() {
        parts.push(timings);
    }
    if let Some(temps) = libre_hardware() {
        parts.push(temps);
    }
    parts.join(" · ")
}

fn zen_timings() -> Option<String> {
    let path = snapshot_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("readings").and_then(|r| r.get("stale")).and_then(|s| s.as_bool()) == Some(true) {
        return None;
    }
    let timings = value.pointer("/config/timings/per_dct/0/timings")?;
    let cl = number(timings, "CL")?;
    let rcd = number(timings, "RCDRD").or_else(|| number(timings, "RCD"))?;
    let rp = number(timings, "RP")?;
    let ras = number(timings, "RAS")?;
    let mut line = format!("Timings CL{cl}-RCD{rcd}-RP{rp}-RAS{ras}");
    if let Some(temp) = value.pointer("/readings/dimm_temp_c").and_then(Value::as_f64) {
        line.push_str(&format!(" · DIMM {temp:.0}°C"));
    }
    Some(line)
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
    for var in ["LOCALAPPDATA", "ProgramData"] {
        let Some(root) = std::env::var_os(var) else { continue };
        let path = PathBuf::from(root).join("ZenTimings").join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn libre_hardware() -> Option<String> {
    let body = http_get("127.0.0.1:8085", "/data.json")?;
    let value: Value = serde_json::from_str(&body).ok()?;
    let mut hits = Vec::new();
    walk(&value, &mut hits);
    if hits.is_empty() {
        return None;
    }
    Some(hits.join(", "))
}

fn walk(value: &Value, hits: &mut Vec<String>) {
    let Some(obj) = value.as_object() else { return };
    let text = obj.get("Text").and_then(Value::as_str).unwrap_or("");
    let reading = obj.get("Value").and_then(Value::as_str).unwrap_or("");
    let useful = text.contains("DIMM") || text.contains("Package") || text.contains("Tctl") || text.contains("CPU");
    if useful && reading.contains('C') && hits.len() < 4 {
        hits.push(format!("{text} {reading}"));
    }
    if let Some(children) = obj.get("Children").and_then(Value::as_array) {
        for child in children {
            walk(child, hits);
        }
    }
}

/// One HTTP/1.0 GET, 200 ms, localhost only. A closed port is a miss, not an error the run shows.
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
