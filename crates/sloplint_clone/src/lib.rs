//! Clone-detection engine (the flagship feature).
//!
//! Two deterministic, no-LLM layers over each function's *normalized* AST: token
//! winnowing catches exact/renamed clones (Type-1/2); MinHash + LSH over subtree hashes
//! catches "same logic, slightly different" near-misses (Type-3). Implemented in the
//! clone-detection PR.
