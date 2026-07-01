//! Presentation for `RepoMetrics`: the GitHub-flavored markdown blocks and shields.io badges
//! for the `metrics` PR summary, plus the god-unit tail it reads off the risk histograms.

use crate::badge::{Badge, Color};
use crate::risk::{CognitiveTier, RiskTier};
use crate::{GodUnits, RepoMetrics};

impl RepoMetrics {
    /// The god-unit **tail**: how many units land in the worst (`very_high`) band of each
    /// distribution. Per-unit *averages* wash these outliers out — a repo can have a dozen
    /// god-modules and a cognitive-172 god-function yet a clean `avg_cognitive` because they're
    /// diluted across thousands of units — so the count of very-high-tier units is the signal that
    /// surfaces them. Reads the existing risk histograms; no extra computation.
    pub fn god_units(&self) -> GodUnits {
        GodUnits {
            cognitive_functions: self.cognitive_risk.very_high,
            cyclomatic_functions: self.cyclomatic_risk.very_high,
            wmc_classes: self.wmc_risk.very_high,
            size_modules: self.module_size_risk.very_high,
        }
    }

    /// A badge summarizing cyclomatic-complexity risk: the worst occupied tier plus the peak
    /// value, colored by that tier (`max complexity: 27 (high)`). Color follows the McCabe
    /// tiers, not arbitrary thresholds, so it stays meaningful as the suite grows.
    pub fn cyclomatic_badge(&self) -> Badge {
        match self.cyclomatic_risk.worst_tier() {
            Some(tier) => Badge::new(
                "max complexity",
                format!("{} ({})", self.max_cyclomatic, tier.label()),
                tier.color(),
            ),
            None => Badge::new("max complexity", "n/a", Color::Green),
        }
    }

    /// A one-line-plus-table markdown block for the PR summary: headline aggregates and the
    /// risk-tier histogram. Reproducible from the same inputs; pairs with the badge.
    pub fn cyclomatic_markdown(&self) -> String {
        let risk = self.cyclomatic_risk;
        format!(
            "**Cyclomatic complexity** — mean {:.1}, p95 {}, max {} (worst tier: {}).\n\n\
             | Risk tier | Functions |\n| --- | ---: |\n\
             | low (1–10) | {} |\n| moderate (11–20) | {} |\n\
             | high (21–50) | {} |\n| very high (>50) | {} |\n",
            self.avg_cyclomatic,
            self.p95_cyclomatic,
            self.max_cyclomatic,
            risk.worst_tier().map(RiskTier::label).unwrap_or("n/a"),
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The arity counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max parameters plus
    /// the Long-Parameter-List histogram. Caller-facing arity (`self`/`cls` excluded). Descriptive
    /// bands ([`ParamCountTier`]) — high `high`/`very high` counts flag *functions to read*, never
    /// defects.
    pub fn params_markdown(&self) -> String {
        let risk = self.param_count_risk;
        format!(
            "**Parameter count** — mean {:.1}, p95 {}, max {}.\n\n\
             | Arity band | Functions |\n| --- | ---: |\n\
             | low (≤4) | {} |\n| moderate (5–6) | {} |\n\
             | high (7–10) | {} |\n| very high (>10) | {} |\n",
            self.avg_params,
            self.p95_params,
            self.max_params,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// A badge summarizing cognitive-complexity risk: the worst occupied band plus the peak
    /// value, colored by that band (`max cognitive: 145 (very high)`). The cognitive counterpart to
    /// [`Self::cyclomatic_badge`] — and the more readability-relevant of the two.
    pub fn cognitive_badge(&self) -> Badge {
        match self.cognitive_risk.worst_cognitive_tier() {
            Some(tier) => Badge::new(
                "max cognitive",
                format!("{} ({})", self.max_cognitive, tier.label()),
                tier.color(),
            ),
            None => Badge::new("max cognitive", "n/a", Color::Green),
        }
    }

    /// The cognitive counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max cognitive plus
    /// the readability-band histogram, anchored on SonarSource's 15/function guidance. Descriptive
    /// bands ([`CognitiveTier`]) — high `high`/`very high` counts flag functions to *read*, not
    /// defects.
    pub fn cognitive_markdown(&self) -> String {
        let risk = self.cognitive_risk;
        format!(
            "**Cognitive complexity** — mean {:.1}, p95 {}, max {} (worst tier: {}).\n\n\
             | Risk tier | Functions |\n| --- | ---: |\n\
             | low (≤5) | {} |\n| moderate (6–15) | {} |\n\
             | high (16–40) | {} |\n| very high (>40) | {} |\n",
            self.avg_cognitive,
            self.p95_cognitive,
            self.max_cognitive,
            risk.worst_cognitive_tier()
                .map(CognitiveTier::label)
                .unwrap_or("n/a"),
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The class-size counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max WMC plus
    /// the god-class-prevalence histogram. Descriptive bands ([`WmcTier`]) — high `high`/`very
    /// high` counts flag *candidates to read*, never defects.
    pub fn wmc_markdown(&self) -> String {
        let risk = self.wmc_risk;
        format!(
            "**Class weight (WMC)** — mean {:.1}, p95 {}, max {}.\n\n\
             | WMC band | Classes |\n| --- | ---: |\n\
             | low (≤20) | {} |\n| moderate (21–50) | {} |\n\
             | high (51–200) | {} |\n| very high (>200) | {} |\n",
            self.avg_wmc,
            self.p95_wmc,
            self.max_wmc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The inheritance-breadth counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max
    /// NOC plus the fragile-base-class histogram. Descriptive bands ([`NocTier`]) — high
    /// `high`/`very high` counts flag *bases to review before changing*, never defects.
    pub fn noc_markdown(&self) -> String {
        let risk = self.noc_risk;
        format!(
            "**Inheritance breadth (NOC)** — mean {:.1}, p95 {}, max {}.\n\n\
             | NOC band | Classes |\n| --- | ---: |\n\
             | low (≤1) | {} |\n| moderate (2–5) | {} |\n\
             | high (6–20) | {} |\n| very high (>20) | {} |\n",
            self.avg_noc,
            self.p95_noc,
            self.max_noc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The class-coupling counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max CBO plus
    /// the hub-class histogram. Descriptive bands ([`CboTier`]) — high `high`/`very high` counts flag
    /// *hubs to review before changing*, never defects. A **lower bound** in dynamically-typed code
    /// (duck-typed coupling is invisible), so the caption says so.
    pub fn cbo_markdown(&self) -> String {
        let risk = self.cbo_risk;
        format!(
            "**Class coupling (CBO)** — mean {:.1}, p95 {}, max {} _(approximate — \
             misses duck-typed coupling)_.\n\n\
             | CBO band | Classes |\n| --- | ---: |\n\
             | low (≤4) | {} |\n| moderate (5–9) | {} |\n\
             | high (10–20) | {} |\n| very high (>20) | {} |\n",
            self.avg_cbo,
            self.p95_cbo,
            self.max_cbo,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The response-set counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max RFC plus the
    /// broad-responder histogram. Descriptive bands ([`RfcTier`]) — high `high`/`very high` counts
    /// flag *broad responders to review before changing*, never defects. A **lower bound** in
    /// dynamically-typed code (see [`Self::cbo_markdown`]), so the caption says so.
    pub fn rfc_markdown(&self) -> String {
        let risk = self.rfc_risk;
        format!(
            "**Response for a class (RFC)** — mean {:.1}, p95 {}, max {} _(approximate — \
             lower bound in dynamic code)_.\n\n\
             | RFC band | Classes |\n| --- | ---: |\n\
             | low (≤20) | {} |\n| moderate (21–50) | {} |\n\
             | high (51–100) | {} |\n| very high (>100) | {} |\n",
            self.avg_rfc,
            self.p95_rfc,
            self.max_rfc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The module-size counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max NLOC plus
    /// the god-module-prevalence histogram. Descriptive NLOC bands ([`ModuleSizeTier`]) — high
    /// `high`/`very high` counts flag *files to read*, never defects.
    pub fn module_size_markdown(&self) -> String {
        let risk = self.module_size_risk;
        format!(
            "**Module size (NLOC)** — mean {:.1}, p95 {}, max {}.\n\n\
             | NLOC band | Files |\n| --- | ---: |\n\
             | low (≤250) | {} |\n| moderate (251–500) | {} |\n\
             | high (501–1000) | {} |\n| very high (>1000) | {} |\n",
            self.avg_module_nloc,
            self.p95_module_nloc,
            self.max_module_nloc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// A one-line markdown summary of exception-handling hygiene: the broad/swallow rates
    /// with the underlying counts. Descriptive cohort signal — broad except is sometimes correct
    /// (daemon loops, plugin boundaries), so it's never a gate.
    pub fn exception_markdown(&self) -> String {
        let exc = self.exception;
        format!(
            "**Exception handling** — broad-except rate {:.2} ({} of {} handlers), swallow rate \
             {:.2} ({} `pass`/`continue`/`...`), {} bare. Descriptive, never a gate.\n",
            self.broad_except_rate,
            exc.broad,
            exc.handlers,
            self.swallow_except_rate,
            exc.swallow,
            exc.bare,
        )
    }
}
