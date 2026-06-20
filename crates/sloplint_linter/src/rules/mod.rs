//! Rules, grouped by category (cf. Ruff's `rules/<linter>/rules/<rule>.rs`).
//!
//! Each rule gets its own file with a violation struct + check function, registered in
//! its category's `mod.rs`. Categories are filled in by later PRs.

pub mod comments;
pub mod duplication;
pub mod scaffolding;
pub mod structure;
