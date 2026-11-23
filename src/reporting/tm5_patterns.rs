//! TM5 SimpleTest pattern provenance: report the effective plan, without feeding the source fields back into a generator.

use serde::{Deserialize, Serialize};

use crate::config::Tm5PatternSource;
use crate::runner::{PatternId, TestDefinition};

/// An imported SimpleTest's original fields and their disposition in this run. Kept in saved results beside the effective program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversion {
    pub source: Tm5PatternSource,
    pub disposition: Disposition,
    /// The actual numbered-mode inputs, including defaults or overrides; native programs replace these fields altogether.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_numbered: Option<Tm5PatternSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    ReplacedByNativeProgram,
    RetainedForNumberedMode,
    OverriddenForNumberedMode,
}

impl Conversion {
    /// Only selected SimpleTests carry a conversion. An exported config later edited into another test must not report one.
    pub(crate) fn capture(def: &TestDefinition) -> Option<Self> {
        if !def.actual_name.starts_with("Mem-SimpleV2") {
            return None;
        }
        let source = def.tm5_pattern.clone()?;
        let effective_numbered = def.config.native_program().is_none().then(|| {
            let pattern = PatternId::from_config(&def.config);
            Tm5PatternSource { mode: pattern.mode, param0: pattern.param0, param1: pattern.param1, written: None }
        });
        let disposition = match &effective_numbered {
            None => Disposition::ReplacedByNativeProgram,
            Some(effective) if (effective.mode, effective.param0, effective.param1) == (source.mode, source.param0, source.param1) => {
                Disposition::RetainedForNumberedMode
            }
            Some(_) => Disposition::OverriddenForNumberedMode,
        };
        Some(Self { source, disposition, effective_numbered })
    }

    fn detail(&self, label: &str, effective: &str) -> String {
        let Tm5PatternSource { mode, param0, param1, written } = &self.source;
        let [param0, param1] = written.clone().unwrap_or_else(|| [format!("{param0:#x}"), format!("{param1:#x}")]);
        let status = match self.disposition {
            Disposition::ReplacedByNativeProgram => "replaced by native program",
            Disposition::RetainedForNumberedMode => "retained for TMR's numbered adaptation",
            Disposition::OverriddenForNumberedMode => "overridden by effective numbered-mode inputs",
        };
        let inputs = self
            .effective_numbered
            .as_ref()
            .map_or(String::new(), |p| format!("; effective Param0={:#x}, Param1={:#x}", p.param0, p.param1));
        format!("TM5 import {label}: Pattern Mode={mode}, Param0={param0}, Param1={param1}: {status}; {effective}{inputs}")
    }
}

fn effective_name(def: &TestDefinition) -> String {
    match def.config.native_program() {
        Some(program) => format!("TMR program: {}", program.names().join(" → ")),
        None => match def.config.pattern_mode.unwrap_or(10) {
            mode @ 0..=2 => format!("TMR legacy adaptation (mode {mode})"),
            mode => format!("TMR numbered mode {mode}"),
        },
    }
}

/// Each selected test once, even when its ID appears repeatedly in the cycle order.
fn selected(defs: &[TestDefinition]) -> impl Iterator<Item = (&TestDefinition, Conversion)> {
    let mut seen = std::collections::HashSet::new();
    defs.iter().filter_map(move |def| {
        let conversion = Conversion::capture(def)?;
        seen.insert(def.test_index).then_some((def, conversion))
    })
}

/// Group by source mode and effective program, not by the original arithmetic parameters (those stay in details).
pub(crate) fn notes(defs: &[TestDefinition]) -> Vec<String> {
    let mut groups: Vec<(u32, String, Vec<String>)> = Vec::new();
    let (mut native, mut numbered) = (false, false);
    for (def, conversion) in selected(defs) {
        native |= conversion.disposition == Disposition::ReplacedByNativeProgram;
        numbered |= conversion.effective_numbered.is_some();
        let effective = effective_name(def);
        let label = def.id.clone().unwrap_or_else(|| def.display_name.clone());
        match groups.iter_mut().find(|(mode, name, _)| *mode == conversion.source.mode && *name == effective) {
            Some((_, _, labels)) => labels.push(label),
            None => groups.push((conversion.source.mode, effective, vec![label])),
        }
    }
    if groups.is_empty() {
        return Vec::new();
    }
    let mut notes = vec![
        match (native, numbered) {
            (true, false) => "ℹ  TM5 import: SimpleTest data patterns converted to TMR native programs.",
            (true, true) => "ℹ  TM5 import: SimpleTests use TMR native programs and numbered adaptations.",
            _ => "ℹ  TM5 import: SimpleTests use TMR's numbered adaptations, not exact TM5 reproduction.",
        }
        .to_string(),
    ];
    for (mode, effective, labels) in groups {
        let noun = if labels.len() == 1 { "Test" } else { "Tests" };
        notes.push(format!("   {noun} {}: TM5 Pattern Mode {mode} → {effective}", labels.join(", ")));
    }
    if native {
        notes.push(
            "   Each native program entry uses a verified P/~P pair; four write rounds advance two entries. Longer programs continue \
             across suite cycles. Verify-only steps span a pair over two suite cycles."
                .to_string(),
        );
    }
    if native && numbered {
        notes.push("   Numbered modes use TMR's adaptations, not exact TM5 reproduction.".to_string());
    }
    notes
}

/// Detailed provenance goes to the log once with the plan, not to the default console/window view or the hot loop: a line a distinct
/// conversion, its tests listed (a `1usmus_v3.cfg` import's 13 SimpleTests share one). The results file keeps each test's.
pub(crate) fn log_details(defs: &[TestDefinition]) {
    let mut groups: Vec<(Conversion, String, Vec<String>)> = Vec::new();
    for (def, conversion) in selected(defs) {
        let effective = effective_name(def);
        let label = def.id.clone().unwrap_or_else(|| def.display_name.clone());
        match groups.iter_mut().find(|(c, e, _)| *c == conversion && *e == effective) {
            Some((_, _, labels)) => labels.push(label),
            None => groups.push((conversion, effective, vec![label])),
        }
    }
    for (conversion, effective, labels) in groups {
        let noun = if labels.len() == 1 { "Test" } else { "Tests" };
        log::info!(target: crate::logfile::FILE_ONLY_TARGET, "{}", conversion.detail(&format!("{noun} {}", labels.join(", ")), &effective));
    }
}
