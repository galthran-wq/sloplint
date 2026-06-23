//! The linter crate — all sloplint rules and the core run logic.
//!
//! Layout deliberately mirrors Ruff's `ruff_linter` so anyone who has contributed to Ruff
//! already knows their way around:
//!
//! - [`registry`] — the `Rule` enum, preview/stable groups, default severity.
//! - [`rules`]    — one file per rule, grouped by category.

pub mod ast_util;
mod codes;
pub mod config;
pub mod detect;
pub mod imports;
pub mod lint;
pub mod registry;
pub mod rules;
pub mod stdlib;
pub mod suppression;
pub mod testing;
pub mod words;
