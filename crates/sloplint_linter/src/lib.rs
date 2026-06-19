//! The linter crate — all sloplint rules and the core run logic.
//!
//! Layout deliberately mirrors Ruff's `ruff_linter` so anyone who has contributed to Ruff
//! already knows their way around:
//!
//! - [`codes`]    — `SLP` code → rule mapping.
//! - [`registry`] — the `Rule` enum, preview/stable groups, default severity.
//! - [`checkers`] — phase entry points (token / AST / physical-line / filesystem).
//! - [`rules`]    — one file per rule, grouped by category.
//!
//! The PRs that follow fill these in; today they establish the structure.

pub mod checkers;
pub mod codes;
pub mod config;
pub mod lint;
pub mod registry;
pub mod rules;
pub mod testing;
pub mod words;
