//! Checker phases — the entry points that drive rules over a parsed file.
//!
//! Each phase consumes the representation it needs (matching Ruff's phase model):
//! comments live in the **token stream**, structure in the **AST**, size in **physical
//! lines**, and project shape on the **filesystem**.

pub mod ast;
pub mod filesystem;
pub mod physical_lines;
pub mod tokens;
