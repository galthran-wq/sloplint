//! Security rules.
//!
//! - `SLP210` phantom security-guard call/decorator — a call to / decorator of a known
//!   security-guard name (`validate_token`, `@requires_auth`, …) that is never defined or imported
//!   in the module (preview — heuristic, behind a curated dictionary).

pub mod phantom_guard;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp210_phantom_guard,
        phantom_guard::PhantomGuard,
        "security",
        "SLP210"
    );
}
