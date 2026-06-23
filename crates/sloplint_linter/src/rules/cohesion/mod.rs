//! Class-cohesion rules.
//!
//! - `SLP120` low-cohesion "god class" via LCOM4 (preview — heuristic).

pub mod god_class;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Limits;
    use crate::test_rule;

    test_rule!(slp120_god_class, god_class::GodClass, "cohesion", "SLP120");

    // Threshold: a two-concept class flags at the default ceiling but is spared once the
    // allowed component count is raised to cover it.
    test_rule!(
        slp120_threshold_default,
        god_class::GodClass,
        "cohesion",
        "SLP120_threshold"
    );
    test_rule!(
        slp120_threshold_raised,
        god_class::GodClass,
        "cohesion",
        "SLP120_threshold",
        Limits {
            lcom4_max_components: 2,
            ..Default::default()
        }
    );
}
