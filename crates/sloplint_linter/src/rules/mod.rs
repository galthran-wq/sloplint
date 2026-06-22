//! Rules, grouped by category (cf. Ruff's `rules/<linter>/rules/<rule>.rs`).
//!
//! Each rule gets its own file with a violation struct + check function, registered in
//! its category's `mod.rs`.

pub mod cohesion;
pub mod comments;
pub mod crosslang;
pub mod duplication;
pub mod placeholders;
pub mod security;
pub mod structure;
