//! Placeholder / mock-data rules.
//!
//! - `SLP230` mock / placeholder data left in production code — placeholder emails/phones/UUIDs,
//!   weak credentials, and dummy return values (preview — heuristic, non-test paths only).

pub mod mock_data;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp230_mock_data,
        mock_data::MockData,
        "placeholders",
        "SLP230"
    );
}
