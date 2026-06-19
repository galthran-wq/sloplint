//! The `Rule` enum, preview/stable grouping, and default severities.
//!
//! Like Ruff, new rules land in a `Preview` group before being promoted to `Stable`,
//! so we can ship and gather feedback without enabling a rule by default. Implemented in
//! the diagnostics/registry PR.
