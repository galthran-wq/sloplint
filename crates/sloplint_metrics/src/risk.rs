//! Risk-tier classification and the shared four-band histogram.
//!
//! Each metric (cyclomatic, cognitive, WMC, NOC, CBO, module NLOC, function arity) maps a raw
//! value into a `{low, moderate, high, very_high}` tier; [`RiskHistogram`] counts how many units
//! fall in each band. Bands are descriptive, calibrated against the cohort — never a pass/fail gate.

use crate::badge::Color;

/// McCabe's cyclomatic-complexity risk tiers — the canonical interpretation from McCabe
/// (1976): the higher the decision count, the harder a function is to test and reason about.
/// Boundaries (inclusive): **1–10 low**, **11–20 moderate**, **21–50 high**, **>50 very
/// high**. McCabe recommends prohibiting functions above 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl RiskTier {
    /// Classify a cyclomatic-complexity value into its McCabe risk tier.
    pub fn from_cyclomatic(cyclomatic: usize) -> Self {
        match cyclomatic {
            0..=10 => RiskTier::Low,
            11..=20 => RiskTier::Moderate,
            21..=50 => RiskTier::High,
            _ => RiskTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables, JSON, and badges.
    pub fn label(self) -> &'static str {
        match self {
            RiskTier::Low => "low",
            RiskTier::Moderate => "moderate",
            RiskTier::High => "high",
            RiskTier::VeryHigh => "very high",
        }
    }

    /// Badge color keyed to the tier: low is green, moderate is yellow, high and very-high
    /// are red (both exceed McCabe's recommended ceiling of 10 by a wide margin).
    pub fn color(self) -> Color {
        match self {
            RiskTier::Low => Color::Green,
            RiskTier::Moderate => Color::Yellow,
            RiskTier::High | RiskTier::VeryHigh => Color::Red,
        }
    }
}

/// Cognitive-complexity bands, anchored on SonarSource's per-function guidance of **15**.
/// Cognitive complexity is the better *readability* signal than cyclomatic — it adds a nesting
/// penalty and charges for breaks in linear flow — so these bands track "how hard is this to read".
/// Boundaries (inclusive): **≤5 low** (trivial), **6–15 moderate** (SonarSource's ceiling), **16–40
/// high** (hard to follow), **>40 very high** (effectively unreadable). Descriptive bands calibrated
/// against the cohort, never a pass/fail gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CognitiveTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl CognitiveTier {
    /// Classify a function's cognitive complexity into its readability band.
    pub fn from_cognitive(cognitive: usize) -> Self {
        match cognitive {
            0..=5 => CognitiveTier::Low,
            6..=15 => CognitiveTier::Moderate,
            16..=40 => CognitiveTier::High,
            _ => CognitiveTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables, JSON, and badges.
    pub fn label(self) -> &'static str {
        match self {
            CognitiveTier::Low => "low",
            CognitiveTier::Moderate => "moderate",
            CognitiveTier::High => "high",
            CognitiveTier::VeryHigh => "very high",
        }
    }

    /// Badge color keyed to the band: low green, moderate yellow, high/very-high red (both exceed
    /// SonarSource's recommended ceiling of 15).
    pub fn color(self) -> Color {
        match self {
            CognitiveTier::Low => Color::Green,
            CognitiveTier::Moderate => Color::Yellow,
            CognitiveTier::High | CognitiveTier::VeryHigh => Color::Red,
        }
    }
}

/// WMC (Weighted Methods per Class) size bands for god-class prevalence. Unlike the
/// cyclomatic [`RiskTier`], WMC has **no** McCabe-equivalent canonical threshold, so these are
/// **descriptive** bands calibrated against the cohort, never a pass/fail standard. Boundaries
/// (inclusive): **≤20 low** (ordinary class), **21–50 moderate** (large but fine), **51–200
/// high** (god-class candidate), **>200 very high** (god-class). WMC is the sum of the cyclomatic
/// complexity of a class's methods, so these run higher than the per-function CC bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WmcTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl WmcTier {
    /// Classify a class's WMC into its size band.
    pub fn from_wmc(wmc: usize) -> Self {
        match wmc {
            0..=20 => WmcTier::Low,
            21..=50 => WmcTier::Moderate,
            51..=200 => WmcTier::High,
            _ => WmcTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            WmcTier::Low => "low",
            WmcTier::Moderate => "moderate",
            WmcTier::High => "high",
            WmcTier::VeryHigh => "very high",
        }
    }
}

/// NOC (Number of Children) breadth bands for fragile-base-class risk — how many direct
/// first-party subclasses a class has. No canonical CK threshold, so **descriptive** bands
/// calibrated against the cohort, never a pass/fail standard. Boundaries (inclusive): **≤1 low**
/// (a leaf or lightly-extended class), **2–5 moderate**, **6–20 high** (a well-used base),
/// **>20 very high** (a high-leverage hub — every change ripples widely; review carefully).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NocTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl NocTier {
    /// Classify a class's NOC into its breadth band.
    pub fn from_noc(noc: usize) -> Self {
        match noc {
            0..=1 => NocTier::Low,
            2..=5 => NocTier::Moderate,
            6..=20 => NocTier::High,
            _ => NocTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            NocTier::Low => "low",
            NocTier::Moderate => "moderate",
            NocTier::High => "high",
            NocTier::VeryHigh => "very high",
        }
    }
}

/// CBO (Coupling Between Objects) bands for hub-class prevalence — how many distinct
/// first-party classes a class is coupled to. No canonical CK threshold (literature cites ~14 as a
/// rough ceiling), so **descriptive** bands calibrated against the cohort, never a pass/fail
/// standard. Boundaries (inclusive): **≤4 low** (focused), **5–9 moderate**, **10–20 high** (a hub
/// to review before changing), **>20 very high** (a central god-class — a change ripples to dozens
/// of collaborators). CBO is a **lower bound** in dynamically-typed Python (see [`ClassMetrics::cbo`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CboTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl CboTier {
    /// Classify a class's CBO into its coupling band.
    pub fn from_cbo(cbo: usize) -> Self {
        match cbo {
            0..=4 => CboTier::Low,
            5..=9 => CboTier::Moderate,
            10..=20 => CboTier::High,
            _ => CboTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            CboTier::Low => "low",
            CboTier::Moderate => "moderate",
            CboTier::High => "high",
            CboTier::VeryHigh => "very high",
        }
    }
}

/// RFC (Response For a Class) bands for response-set size — own methods plus the distinct
/// methods they invoke. No canonical CK threshold, but the OO-metrics literature widely cites
/// **~50 as high** and **~100 as very high**, so these **descriptive** bands (calibrated against
/// the cohort, never a pass/fail standard) follow that rule of thumb. Boundaries (inclusive):
/// **≤20 low** (focused), **21–50 moderate**, **51–100 high** (a broad responder to review),
/// **>100 very high** (a class one message pulls dozens of collaborators into). A **lower bound**
/// in dynamically-typed Python (see [`ClassMetrics::rfc`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RfcTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl RfcTier {
    /// Classify a class's RFC into its response-set band.
    pub fn from_rfc(rfc: usize) -> Self {
        match rfc {
            0..=20 => RfcTier::Low,
            21..=50 => RfcTier::Moderate,
            51..=100 => RfcTier::High,
            _ => RfcTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            RfcTier::Low => "low",
            RfcTier::Moderate => "moderate",
            RfcTier::High => "high",
            RfcTier::VeryHigh => "very high",
        }
    }
}

/// Module (file) NLOC size bands for god-module prevalence. Like [`WmcTier`], file size has
/// **no** canonical hard threshold, so these are **descriptive** bands calibrated against the
/// cohort (SonarQube's ~750–1000-line guidance is the starting point), never a pass/fail standard.
/// Boundaries (inclusive), in NLOC (non-comment, non-blank lines): **≤250 low** (ordinary module),
/// **251–500 moderate**, **501–1000 high** (god-module candidate), **>1000 very high**
/// (god-module — a dumping-ground smell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleSizeTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl ModuleSizeTier {
    /// Classify a module's NLOC into its size band.
    pub fn from_nloc(nloc: usize) -> Self {
        match nloc {
            0..=250 => ModuleSizeTier::Low,
            251..=500 => ModuleSizeTier::Moderate,
            501..=1000 => ModuleSizeTier::High,
            _ => ModuleSizeTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            ModuleSizeTier::Low => "low",
            ModuleSizeTier::Moderate => "moderate",
            ModuleSizeTier::High => "high",
            ModuleSizeTier::VeryHigh => "very high",
        }
    }
}

/// Function-arity bands for the Long Parameter List smell — Fowler's canonical signal that
/// parameters want bundling into an object. Counts caller-facing [`FunctionMetrics::arity`], not
/// raw params. No canonical hard threshold (Fowler/Martin suggest keeping arguments ≤3–4), so
/// **descriptive** bands, never a pass/fail standard. Boundaries (inclusive): **≤4 low**,
/// **5–6 moderate**, **7–10 high**, **>10 very high**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParamCountTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl ParamCountTier {
    /// Classify a function's caller-facing arity into its band.
    pub fn from_arity(arity: usize) -> Self {
        match arity {
            0..=4 => ParamCountTier::Low,
            5..=6 => ParamCountTier::Moderate,
            7..=10 => ParamCountTier::High,
            _ => ParamCountTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            ParamCountTier::Low => "low",
            ParamCountTier::Moderate => "moderate",
            ParamCountTier::High => "high",
            ParamCountTier::VeryHigh => "very high",
        }
    }
}

/// A four-band tier histogram: how many units fall into each `{low, moderate, high, very_high}`
/// band. Shared by the function cyclomatic tiers, the class WMC tiers, the module
/// NLOC tiers, and the function-arity tiers — the bands differ per metric (see
/// [`RiskTier`] / [`WmcTier`] / [`ModuleSizeTier`] / [`ParamCountTier`]); the bucket shape does
/// not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RiskHistogram {
    pub low: usize,
    pub moderate: usize,
    pub high: usize,
    pub very_high: usize,
}

impl RiskHistogram {
    pub(crate) fn record(&mut self, cyclomatic: usize) {
        match RiskTier::from_cyclomatic(cyclomatic) {
            RiskTier::Low => self.low += 1,
            RiskTier::Moderate => self.moderate += 1,
            RiskTier::High => self.high += 1,
            RiskTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a function by its cognitive band — the readability counterpart to
    /// [`Self::record`] (which buckets by cyclomatic).
    pub(crate) fn record_cognitive(&mut self, cognitive: usize) {
        match CognitiveTier::from_cognitive(cognitive) {
            CognitiveTier::Low => self.low += 1,
            CognitiveTier::Moderate => self.moderate += 1,
            CognitiveTier::High => self.high += 1,
            CognitiveTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a class by its WMC band — the class-side counterpart to [`Self::record`].
    pub(crate) fn record_wmc(&mut self, wmc: usize) {
        match WmcTier::from_wmc(wmc) {
            WmcTier::Low => self.low += 1,
            WmcTier::Moderate => self.moderate += 1,
            WmcTier::High => self.high += 1,
            WmcTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a class by its NOC band — inheritance breadth (fragile-base-class risk).
    pub(crate) fn record_noc(&mut self, noc: usize) {
        match NocTier::from_noc(noc) {
            NocTier::Low => self.low += 1,
            NocTier::Moderate => self.moderate += 1,
            NocTier::High => self.high += 1,
            NocTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a class by its CBO band — class-to-class coupling (hub-class risk).
    pub(crate) fn record_cbo(&mut self, cbo: usize) {
        match CboTier::from_cbo(cbo) {
            CboTier::Low => self.low += 1,
            CboTier::Moderate => self.moderate += 1,
            CboTier::High => self.high += 1,
            CboTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a class by its RFC band — response-set size (broad-responder risk).
    pub(crate) fn record_rfc(&mut self, rfc: usize) {
        match RfcTier::from_rfc(rfc) {
            RfcTier::Low => self.low += 1,
            RfcTier::Moderate => self.moderate += 1,
            RfcTier::High => self.high += 1,
            RfcTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a module by its NLOC band — the file-side counterpart to [`Self::record`].
    pub(crate) fn record_module_size(&mut self, nloc: usize) {
        match ModuleSizeTier::from_nloc(nloc) {
            ModuleSizeTier::Low => self.low += 1,
            ModuleSizeTier::Moderate => self.moderate += 1,
            ModuleSizeTier::High => self.high += 1,
            ModuleSizeTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a function by its arity band — the Long-Parameter-List counterpart to
    /// [`Self::record`].
    pub(crate) fn record_arity(&mut self, arity: usize) {
        match ParamCountTier::from_arity(arity) {
            ParamCountTier::Low => self.low += 1,
            ParamCountTier::Moderate => self.moderate += 1,
            ParamCountTier::High => self.high += 1,
            ParamCountTier::VeryHigh => self.very_high += 1,
        }
    }

    /// The worst tier that actually has a function in it (the headline risk for a badge).
    /// `None` only when there are no functions at all.
    pub fn worst_tier(self) -> Option<RiskTier> {
        if self.very_high > 0 {
            Some(RiskTier::VeryHigh)
        } else if self.high > 0 {
            Some(RiskTier::High)
        } else if self.moderate > 0 {
            Some(RiskTier::Moderate)
        } else if self.low > 0 {
            Some(RiskTier::Low)
        } else {
            None
        }
    }

    /// The worst occupied band as a [`CognitiveTier`] — the cognitive counterpart to
    /// [`Self::worst_tier`], for the cognitive badge/markdown. `None` only when empty.
    pub fn worst_cognitive_tier(self) -> Option<CognitiveTier> {
        if self.very_high > 0 {
            Some(CognitiveTier::VeryHigh)
        } else if self.high > 0 {
            Some(CognitiveTier::High)
        } else if self.moderate > 0 {
            Some(CognitiveTier::Moderate)
        } else if self.low > 0 {
            Some(CognitiveTier::Low)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    /// Render a tier's band map over its boundary values: each value with the band it lands in.
    /// One snapshot pins every threshold — an off-by-one drift in any `from_*` cutoff flips a
    /// label and fails the test. Replaces the per-tier `assert_eq!` ladders; the boundary values
    /// (each band's last-in and first-out) are the same the asserts checked.
    fn bands(name: &str, values: &[usize], label: impl Fn(usize) -> &'static str) -> String {
        let mut out = format!("{name}\n");
        for &v in values {
            writeln!(out, "  {v:>4} => {}", label(v)).unwrap();
        }
        out
    }

    #[test]
    fn tier_band_boundaries() {
        let mut report = String::new();
        // McCabe cyclomatic: ≤10 low, 11–20 moderate, 21–50 high, >50 very high.
        report.push_str(&bands(
            "RiskTier::from_cyclomatic",
            &[1, 10, 11, 20, 21, 50, 51],
            |v| RiskTier::from_cyclomatic(v).label(),
        ));
        // Cognitive: ≤5 low, 6–15 moderate, 16–40 high, >40 very high.
        report.push_str(&bands(
            "CognitiveTier::from_cognitive",
            &[0, 5, 6, 15, 16, 40, 41],
            |v| CognitiveTier::from_cognitive(v).label(),
        ));
        // WMC: ≤20 low, 21–50 moderate, 51–200 high, >200 very high.
        report.push_str(&bands(
            "WmcTier::from_wmc",
            &[0, 20, 21, 50, 51, 200, 201],
            |v| WmcTier::from_wmc(v).label(),
        ));
        // NOC breadth: ≤1 low, 2–5 moderate, 6–20 high, >20 very high.
        report.push_str(&bands("NocTier::from_noc", &[0, 1, 2, 5, 6, 20, 21], |v| {
            NocTier::from_noc(v).label()
        }));
        // CBO: ≤4 low, 5–9 moderate, 10–20 high, >20 very high.
        report.push_str(&bands(
            "CboTier::from_cbo",
            &[0, 4, 5, 9, 10, 20, 21],
            |v| CboTier::from_cbo(v).label(),
        ));
        // RFC: ≤20 low, 21–50 moderate, 51–100 high, >100 very high.
        report.push_str(&bands(
            "RfcTier::from_rfc",
            &[0, 20, 21, 50, 51, 100, 101],
            |v| RfcTier::from_rfc(v).label(),
        ));
        // Module NLOC: ≤250 low, 251–500 moderate, 501–1000 high, >1000 very high.
        report.push_str(&bands(
            "ModuleSizeTier::from_nloc",
            &[0, 250, 251, 500, 501, 1000, 1001],
            |v| ModuleSizeTier::from_nloc(v).label(),
        ));
        // Arity: ≤4 low, 5–6 moderate, 7–10 high, >10 very high.
        report.push_str(&bands(
            "ParamCountTier::from_arity",
            &[0, 4, 5, 6, 7, 10, 11],
            |v| ParamCountTier::from_arity(v).label(),
        ));
        insta::assert_snapshot!(report);
    }
}
